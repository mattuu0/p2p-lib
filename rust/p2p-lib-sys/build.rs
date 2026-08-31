use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// GitHub repo hosting the prebuilt tailcat_cgo release assets that
/// build-native.yml (.github/workflows) publishes. Override with
/// P2P_LIB_RELEASE_REPO for forks that publish their own releases.
const DEFAULT_RELEASE_REPO: &str = "mattuu0/p2p-lib";

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    let (lib_filename, link_kind) = match target_os.as_str() {
        "windows" => ("tailcat_cgo.dll", "dylib"),
        "linux" => ("libtailcat_cgo.so", "dylib"),
        "macos" => ("libtailcat_cgo.dylib", "dylib"),
        other => panic!("p2p-lib-sys: unsupported target OS `{other}`"),
    };
    let lib_path = out_dir.join(lib_filename);

    // Default: fetch the prebuilt binary GitHub Actions already built and
    // published for this exact crate version, so consumers don't need Go
    // or a cgo toolchain installed at all. Set P2P_LIB_BUILD_FROM_SOURCE=1
    // to always build tailcat-cgo locally instead (e.g. for platforms the
    // release workflow doesn't cover yet, or while developing tailcat-cgo
    // itself against a local submodule change).
    let build_from_source = env::var("P2P_LIB_BUILD_FROM_SOURCE").is_ok();

    if !build_from_source {
        match try_download_prebuilt(&out_dir, lib_filename, &target_os, &target_arch) {
            Ok(()) => {
                finish_link(&out_dir, &lib_path, lib_filename, link_kind, &target_os, &target_arch);
                return;
            }
            Err(e) => {
                println!(
                    "cargo:warning=p2p-lib-sys: prebuilt binary download failed ({e}); \
                     falling back to building tailcat-cgo from source. Set \
                     P2P_LIB_BUILD_FROM_SOURCE=1 to skip the download attempt entirely."
                );
            }
        }
    }

    build_from_go_source(&lib_path, &target_os, &target_arch);
    finish_link(&out_dir, &lib_path, lib_filename, link_kind, &target_os, &target_arch);
}

/// Shared tail end of both code paths: copy the built/downloaded shared
/// library next to the final binary, generate an import lib appropriate for
/// the active Windows ABI (MSVC vs GNU/MinGW), and emit the link directives.
fn finish_link(
    out_dir: &Path,
    lib_path: &Path,
    lib_filename: &str,
    link_kind: &str,
    target_os: &str,
    target_arch: &str,
) {
    copy_to_target_dirs(out_dir, lib_path, lib_filename);

    if target_os == "windows" {
        // rustc's `x86_64-pc-windows-msvc` links with link.exe and needs an
        // MSVC-format .lib; `x86_64-pc-windows-gnu` links with GNU ld and
        // needs a GNU-format .dll.a instead -- neither linker accepts the
        // other's import library format, so the target_env this crate is
        // actually being built for decides which one to generate.
        let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
        match target_env.as_str() {
            "msvc" => generate_msvc_import_lib(out_dir, lib_path, target_arch),
            "gnu" => generate_gnu_import_lib(out_dir, lib_path),
            other => panic!("p2p-lib-sys: unsupported Windows target_env `{other}`"),
        }
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib={link_kind}=tailcat_cgo");
}

/// Downloads the prebuilt tailcat_cgo archive for this crate's version and
/// the current (target_os, target_arch) from GitHub Releases, verifies it
/// against the release's SHA256SUMS.txt, and extracts lib_filename (plus
/// tailcat_cgo.h) into out_dir. Returns Err with a human-readable reason on
/// any failure (network, missing release, checksum mismatch, ...) so the
/// caller can fall back to building from source.
fn try_download_prebuilt(
    out_dir: &Path,
    lib_filename: &str,
    target_os: &str,
    target_arch: &str,
) -> Result<(), String> {
    let version = env::var("CARGO_PKG_VERSION").map_err(|e| e.to_string())?;
    let repo = env::var("P2P_LIB_RELEASE_REPO").unwrap_or_else(|_| DEFAULT_RELEASE_REPO.to_string());
    let asset_name = release_asset_name(target_os, target_arch)?;

    let base_url = format!("https://github.com/{repo}/releases/download/v{version}");
    let zip_url = format!("{base_url}/{asset_name}.zip");
    let sums_url = format!("{base_url}/SHA256SUMS.txt");

    println!("cargo:rerun-if-env-changed=P2P_LIB_BUILD_FROM_SOURCE");
    println!("cargo:rerun-if-env-changed=P2P_LIB_RELEASE_REPO");

    let sums_text = http_get(&sums_url)?;
    let expected_hash = sums_text
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?.trim_start_matches('*');
            (name == format!("{asset_name}.zip")).then(|| hash.to_string())
        })
        .ok_or_else(|| format!("{asset_name}.zip not listed in {sums_url}"))?;

    let zip_bytes = http_get_bytes(&zip_url)?;

    let actual_hash = sha256_hex(&zip_bytes);
    if actual_hash != expected_hash {
        return Err(format!(
            "SHA256 mismatch for {asset_name}.zip: expected {expected_hash}, got {actual_hash}"
        ));
    }

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| format!("invalid zip archive: {e}"))?;
    let mut found_lib = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let entry_name = entry
            .enclosed_name()
            .ok_or_else(|| "zip entry with unsafe path".to_string())?;
        let file_name = entry_name
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if file_name == lib_filename || file_name == "tailcat_cgo.h" || file_name == "tailcat_cgo.lib" {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            fs::write(out_dir.join(file_name), &buf).map_err(|e| e.to_string())?;
            if file_name == lib_filename {
                found_lib = true;
            }
        }
    }
    if !found_lib {
        return Err(format!("{lib_filename} not found inside {asset_name}.zip"));
    }
    println!("cargo:warning=p2p-lib-sys: using prebuilt {asset_name} v{version} from GitHub Releases");
    Ok(())
}

fn release_asset_name(target_os: &str, target_arch: &str) -> Result<String, String> {
    Ok(match (target_os, target_arch) {
        ("windows", "x86_64") => "tailcat_cgo-windows-amd64".to_string(),
        ("linux", "x86_64") => "tailcat_cgo-linux-amd64".to_string(),
        ("macos", "aarch64") => "tailcat_cgo-darwin-arm64".to_string(),
        ("macos", "x86_64") => "tailcat_cgo-darwin-amd64".to_string(),
        (os, arch) => {
            return Err(format!(
                "no prebuilt release asset published for {os}/{arch} yet"
            ))
        }
    })
}

fn http_get(url: &str) -> Result<String, String> {
    let bytes = http_get_bytes(url)?;
    String::from_utf8(bytes).map_err(|e| format!("{url}: response was not valid UTF-8: {e}"))
}

fn http_get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("GET {url} failed: {e}"))?;
    let mut buf = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| format!("GET {url}: failed to read body: {e}"))?;
    Ok(buf)
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Builds tailcat-cgo from the local Go source (the original behavior,
/// still used when P2P_LIB_BUILD_FROM_SOURCE=1 is set or the prebuilt
/// download fails for any reason).
fn build_from_go_source(lib_path: &Path, target_os: &str, target_arch: &str) {
    let go_os = match target_os {
        "windows" => "windows",
        "linux" => "linux",
        "macos" => "darwin",
        other => panic!("p2p-lib-sys: unsupported target OS `{other}`"),
    };
    let go_arch = match target_arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => panic!("p2p-lib-sys: unsupported target arch `{other}`"),
    };

    // The cgo wrapper module lives at ../../tailcat-cgo relative to this
    // crate (p2p-lib-sys), inside the parent p2p-lib repo.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let cgo_dir = manifest_dir.join("../../tailcat-cgo");
    println!("cargo:rerun-if-changed={}", cgo_dir.display());

    let mut cmd = Command::new("go");
    cmd.current_dir(&cgo_dir)
        .env("GOOS", go_os)
        .env("GOARCH", go_arch)
        .env("CGO_ENABLED", "1")
        .args([
            "build",
            "-ldflags=-s -w", // strip symbols/DWARF debug info; roughly halves the shared library's size
            "-buildmode=c-shared",
            "-o",
            lib_path.to_str().unwrap(),
            ".",
        ]);

    // Windows requires a mingw-w64 gcc as the cgo C compiler; if one isn't
    // already selected via CC, fall back to the conventional name.
    if target_os == "windows" && env::var("CC").is_err() {
        if let Some(cc) = find_mingw_gcc() {
            cmd.env("CC", cc);
        }
    }

    let status = cmd
        .status()
        .expect("p2p-lib-sys: failed to invoke `go build` -- is Go installed and on PATH?");
    if !status.success() {
        panic!("p2p-lib-sys: `go build -buildmode=c-shared` failed (see output above)");
    }
}

/// Copies the shared library next to wherever the dynamic loader will
/// actually look for it at runtime (OUT_DIR isn't on the default search
/// path). OUT_DIR is target/<profile>/build/<crate>-<hash>/out, so
/// target/<profile> is three levels up; binaries, integration tests, and
/// examples all end up in that directory or its `examples`/`deps`
/// subdirectories depending on how they're invoked, so cover all three.
fn copy_to_target_dirs(out_dir: &Path, lib_path: &Path, lib_filename: &str) {
    if let Some(target_dir) = out_dir.ancestors().nth(3).map(Path::to_path_buf) {
        for dest_dir in [
            target_dir.clone(),
            target_dir.join("examples"),
            target_dir.join("deps"),
        ] {
            if fs::create_dir_all(&dest_dir).is_err() {
                continue;
            }
            let dest = dest_dir.join(lib_filename);
            if let Err(e) = fs::copy(lib_path, &dest) {
                println!(
                    "cargo:warning=p2p-lib-sys: could not copy {} to {}: {e}",
                    lib_path.display(),
                    dest.display()
                );
            }
        }
    }
}

/// The Go toolchain's cgo -buildmode=c-shared on Windows uses mingw-w64 gcc
/// and only emits the .dll itself, not an MSVC-compatible .lib import
/// library. rustc's default `x86_64-pc-windows-msvc` target links with
/// link.exe, which requires that .lib to resolve the DLL's symbols at link
/// time. We regenerate it here from the DLL's export table via `dumpbin`
/// (to list exports) and `lib.exe /def` (to build the .lib), both from the
/// MSVC Build Tools -- the same toolchain rustc itself needs on this
/// target, so if rustc can build here, they're expected to be on PATH
/// (typically via a "Developer Command Prompt" or vswhere-located tools).
///
/// Skipped entirely if a prebuilt tailcat_cgo.lib was already extracted
/// from the downloaded release zip (see try_download_prebuilt) -- CI
/// already generated one against the exact DLL it published.
fn generate_msvc_import_lib(out_dir: &Path, dll_path: &Path, target_arch: &str) {
    if out_dir.join("tailcat_cgo.lib").is_file() {
        return;
    }

    let msvc_bin = find_msvc_bin_dir();
    let tool = |name: &str| -> Command {
        match &msvc_bin {
            Some(dir) => Command::new(dir.join(format!("{name}.exe"))),
            None => Command::new(name),
        }
    };

    let dumpbin_out = tool("dumpbin")
        .args(["/exports", dll_path.to_str().unwrap()])
        .output()
        .expect("p2p-lib-sys: failed to run `dumpbin` -- run from an MSVC developer prompt, or ensure MSVC Build Tools are on PATH");
    if !dumpbin_out.status.success() {
        panic!(
            "p2p-lib-sys: `dumpbin /exports` failed:\n{}",
            String::from_utf8_lossy(&dumpbin_out.stderr)
        );
    }
    let dumpbin_text = String::from_utf8_lossy(&dumpbin_out.stdout);

    let mut found_any = false;
    let exports: Vec<&str> = dumpbin_text
        .lines()
        .filter_map(|line| {
            // Export table rows look like:
            //   "          1    0 00846C80 tailcat_client_close"
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields.len() == 4 && fields[0].parse::<u32>().is_ok() && fields[3].starts_with("tailcat_"))
                .then(|| {
                    found_any = true;
                    fields[3]
                })
        })
        .collect();
    if !found_any {
        panic!("p2p-lib-sys: found no tailcat_* exports in dumpbin output; DLL build may have failed silently");
    }

    let def_path = write_def_file(out_dir, &exports);

    let msvc_machine = match target_arch {
        "x86_64" => "X64",
        "aarch64" => "ARM64",
        other => panic!("p2p-lib-sys: unsupported MSVC target arch `{other}`"),
    };
    let lib_out = out_dir.join("tailcat_cgo.lib");
    let status = tool("lib")
        .current_dir(out_dir)
        .args([
            &format!("/def:{}", def_path.display()),
            &format!("/out:{}", lib_out.display()),
            &format!("/machine:{msvc_machine}"),
        ])
        .status()
        .expect("p2p-lib-sys: failed to run `lib.exe` -- run from an MSVC developer prompt, or ensure MSVC Build Tools are on PATH");
    if !status.success() {
        panic!("p2p-lib-sys: `lib.exe /def` failed (see output above)");
    }
}

/// Writes a minimal `.def` file (just an `EXPORTS` section listing the given
/// symbol names) shared by both the MSVC (`lib.exe /def`) and GNU (`dlltool
/// --def`) import-lib generation paths.
fn write_def_file(out_dir: &Path, exports: &[&str]) -> PathBuf {
    let mut def_contents = String::from("EXPORTS\n");
    for name in exports {
        def_contents.push_str(name);
        def_contents.push('\n');
    }
    let def_path = out_dir.join("tailcat_cgo.def");
    fs::write(&def_path, def_contents).expect("p2p-lib-sys: failed to write .def file");
    def_path
}

/// On the GNU (MinGW) target, `ld` needs a GNU-format `.dll.a` import
/// library instead of MSVC's `.lib` -- neither format is accepted by the
/// other linker. We list the DLL's exports with `objdump` (part of the
/// mingw-w64 toolchain rustc itself needs on this target) and hand them to
/// `dlltool` to produce `libtailcat_cgo.dll.a`.
fn generate_gnu_import_lib(out_dir: &Path, dll_path: &Path) {
    let lib_out = out_dir.join("libtailcat_cgo.dll.a");
    if lib_out.is_file() {
        return;
    }

    let objdump_out = Command::new("objdump")
        .args(["-p", dll_path.to_str().unwrap()])
        .output()
        .expect("p2p-lib-sys: failed to run `objdump` -- ensure the mingw-w64 toolchain (which ships with the GNU Rust target) is on PATH");
    if !objdump_out.status.success() {
        panic!(
            "p2p-lib-sys: `objdump -p` failed:\n{}",
            String::from_utf8_lossy(&objdump_out.stderr)
        );
    }
    let objdump_text = String::from_utf8_lossy(&objdump_out.stdout);

    // `objdump -p` prints several tables; the DLL's own exports are listed
    // under the "[Ordinal/Name Pointer] Table" header (a different,
    // earlier-printed "[Ordinal/Name Pointer] Table" also appears per
    // imported system DLL, but those entries are e.g. "GetProcAddress", not
    // "tailcat_*", so filtering by prefix is sufficient without needing to
    // track which section we're in). Rows look like:
    //   "\t[   0] +base[   1]  0000 tailcat_client_close"
    // One unrelated line also ends in a `tailcat_*`-prefixed token: the
    // Export Table header's own DLL name, e.g.
    //   "Name    000000000335810e tailcat_cgo.dll"
    // -- exclude it explicitly, since it isn't a real exported symbol and
    // corrupts the .def file if included (dlltool then miscounts ordinals).
    let mut found_any = false;
    let exports: Vec<&str> = objdump_text
        .lines()
        .filter(|line| !line.trim_start().starts_with("Name "))
        .filter_map(|line| {
            let name = line.trim().rsplit(' ').next()?;
            (!name.is_empty() && name.starts_with("tailcat_") && !name.ends_with(".dll")).then(|| {
                found_any = true;
                name
            })
        })
        .collect();
    if !found_any {
        panic!("p2p-lib-sys: found no tailcat_* exports in objdump output; DLL build may have failed silently");
    }

    let def_path = write_def_file(out_dir, &exports);

    let status = Command::new("dlltool")
        .args([
            "--input-def",
            def_path.to_str().unwrap(),
            "--dllname",
            dll_path.file_name().unwrap().to_str().unwrap(),
            "--output-lib",
            lib_out.to_str().unwrap(),
        ])
        .status()
        .expect("p2p-lib-sys: failed to run `dlltool` -- ensure the mingw-w64 toolchain is on PATH");
    if !status.success() {
        panic!("p2p-lib-sys: `dlltool --input-def` failed (see output above)");
    }
}

/// Locates the MSVC Build Tools `HostX64\x64` (or `HostX86\x86`) bin
/// directory containing `dumpbin.exe`/`lib.exe`, by searching the
/// conventional VS/BuildTools install roots. Returns None if `dumpbin`
/// already resolves on PATH (nothing extra to add) or nothing is found,
/// in which case callers fall back to bare command names.
fn find_msvc_bin_dir() -> Option<PathBuf> {
    if Command::new("dumpbin")
        .arg("/?")
        .output()
        .map(|o| o.status.success() || o.status.code() == Some(1)) // dumpbin /? exits nonzero but runs
        .unwrap_or(false)
    {
        return None;
    }
    let roots = [
        r"C:\Program Files\Microsoft Visual Studio",
        r"C:\Program Files (x86)\Microsoft Visual Studio",
    ];
    for root in roots {
        let root = Path::new(root);
        if !root.is_dir() {
            continue;
        }
        // .../<year>/<edition>/VC/Tools/MSVC/<version>/bin/HostX64/x64
        if let Ok(years) = fs::read_dir(root) {
            for year in years.flatten() {
                let vc_tools = year.path().join("BuildTools/VC/Tools/MSVC");
                let vc_tools = if vc_tools.is_dir() {
                    vc_tools
                } else {
                    // also check non-BuildTools editions (Community, etc.)
                    match fs::read_dir(year.path()).ok().and_then(|mut it| {
                        it.find_map(|e| {
                            let p = e.ok()?.path().join("VC/Tools/MSVC");
                            p.is_dir().then_some(p)
                        })
                    }) {
                        Some(p) => p,
                        None => continue,
                    }
                };
                if let Ok(versions) = fs::read_dir(&vc_tools) {
                    for version in versions.flatten() {
                        let bin = version.path().join("bin/HostX64/x64");
                        if bin.join("dumpbin.exe").is_file() {
                            return Some(bin);
                        }
                    }
                }
            }
        }
    }
    None
}

fn find_mingw_gcc() -> Option<String> {
    for candidate in ["x86_64-w64-mingw32-gcc", "gcc"] {
        if Command::new(candidate)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(candidate.to_string());
        }
    }
    None
}

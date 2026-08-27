use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    let go_os = match target_os.as_str() {
        "windows" => "windows",
        "linux" => "linux",
        "macos" => "darwin",
        other => panic!("p2p-lib-sys: unsupported target OS `{other}`"),
    };
    let go_arch = match target_arch.as_str() {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => panic!("p2p-lib-sys: unsupported target arch `{other}`"),
    };

    let (lib_filename, link_kind) = match target_os.as_str() {
        "windows" => ("tailcat_cgo.dll", "dylib"),
        "linux" => ("libtailcat_cgo.so", "dylib"),
        "macos" => ("libtailcat_cgo.dylib", "dylib"),
        _ => unreachable!(),
    };
    let lib_path = out_dir.join(lib_filename);

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

    // Copy the shared library next to wherever the dynamic loader will
    // actually look for it at runtime (OUT_DIR isn't on the default search
    // path). OUT_DIR is target/<profile>/build/<crate>-<hash>/out, so
    // target/<profile> is three levels up; binaries, integration tests,
    // and examples all end up in that directory or its `examples`/`deps`
    // subdirectories depending on how they're invoked, so cover all three.
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
            if let Err(e) = fs::copy(&lib_path, &dest) {
                println!(
                    "cargo:warning=p2p-lib-sys: could not copy {} to {}: {e}",
                    lib_path.display(),
                    dest.display()
                );
            }
        }
    }

    if target_os == "windows" {
        generate_msvc_import_lib(&out_dir, &lib_path, &target_arch);
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib={link_kind}=tailcat_cgo");
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
fn generate_msvc_import_lib(out_dir: &Path, dll_path: &Path, target_arch: &str) {
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

    let mut def_contents = String::from("EXPORTS\n");
    let mut found_any = false;
    for line in dumpbin_text.lines() {
        // Export table rows look like:
        //   "          1    0 00846C80 tailcat_client_close"
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() == 4 && fields[0].parse::<u32>().is_ok() && fields[3].starts_with("tailcat_") {
            def_contents.push_str(fields[3]);
            def_contents.push('\n');
            found_any = true;
        }
    }
    if !found_any {
        panic!("p2p-lib-sys: found no tailcat_* exports in dumpbin output; DLL build may have failed silently");
    }

    let def_path = out_dir.join("tailcat_cgo.def");
    fs::write(&def_path, def_contents).expect("p2p-lib-sys: failed to write .def file");

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

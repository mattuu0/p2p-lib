# The Flutter tooling requires that developers have CMake 3.10 or later
# installed. You should not increase this version, as doing so will cause
# the plugin to fail to compile for some customers of the plugin.
cmake_minimum_required(VERSION 3.10)

project(p2p_lib_flutter_library VERSION 0.0.1 LANGUAGES C)

# tailcat-cgo is a Go module (cgo, buildmode=c-shared) that lives in the
# parent p2p-lib repo at ../../../tailcat-cgo. Rather than vendoring its Go
# dependency tree as C sources, this plugin shells out to `go build` to
# produce a shared library and repackages it as the CMake target Flutter
# expects to bundle. This mirrors how rust/p2p-lib-sys/build.rs does the
# same thing for Rust.
#
# Flutter's tooling reaches this file through a symlink
# (windows/flutter/ephemeral/.plugin_symlinks/p2p_lib_flutter -> the real
# package directory), and CMAKE_CURRENT_SOURCE_DIR preserves that symlinked
# path verbatim rather than resolving it. Both get_filename_component(...
# REALPATH) and file(REAL_PATH ...) collapse the trailing "../../../"
# lexically *before* resolving the symlink, so they land inside
# .../ephemeral/ (which has no tailcat-cgo) instead of following the
# symlink out to the real package directory first. Resolving just the
# symlinked prefix on its own (with no ".." after it) sidesteps that:
# once CMAKE_CURRENT_SOURCE_DIR itself is a real, symlink-free path, the
# ordinary "../../../" walk from there lands in the right place on both
# the plugin's own example app and any app that depends on this plugin
# normally via pub.
# Flutter's tooling reaches this file through a symlink
# (windows/flutter/ephemeral/.plugin_symlinks/p2p_lib_flutter -> the real
# package directory this pub package was fetched to). Nothing that merely
# *normalizes* a path -- get_filename_component(... REALPATH),
# file(REAL_PATH ...), CMAKE_CURRENT_LIST_DIR, or even PowerShell's own
# Resolve-Path -- actually follows an NTFS symlink/reparse point; they all
# return CMAKE_CURRENT_SOURCE_DIR verbatim, so walking "../../../" from it
# lands inside .../ephemeral/ (which has no tailcat-cgo) instead of the
# real repo root. What *does* follow the reparse point is reading the
# symlink's own recorded target, via .NET's DirectoryInfo.LinkTarget (or
# equivalently, dir's own /A:L-aware `Get-Item ... | Select
# -Expand Target`) -- so walk up from CMAKE_CURRENT_SOURCE_DIR ourselves
# looking for the first symlinked path component and resolve only that one
# hop, in case the exact symlink is renamed/relocated in a future Flutter
# tooling version.
if (WIN32)
  execute_process(
    COMMAND powershell -NoProfile -Command
      "$p = '${CMAKE_CURRENT_SOURCE_DIR}' -replace '/','\\\\'; \
       $parts = $p -split '\\\\'; $cur = $parts[0]; \
       for ($i = 1; $i -lt $parts.Length; $i++) { \
         $cur = Join-Path $cur $parts[$i]; \
         $item = Get-Item -LiteralPath $cur -ErrorAction SilentlyContinue; \
         if ($item -and $item.Target) { $cur = $item.Target } \
       }; \
       $cur"
    OUTPUT_VARIABLE TAILCAT_CGO_REAL_SRC_DIR
    OUTPUT_STRIP_TRAILING_WHITESPACE
  )
  file(TO_CMAKE_PATH "${TAILCAT_CGO_REAL_SRC_DIR}" TAILCAT_CGO_REAL_SRC_DIR)
else ()
  get_filename_component(TAILCAT_CGO_REAL_SRC_DIR "${CMAKE_CURRENT_SOURCE_DIR}" REALPATH)
endif ()
get_filename_component(TAILCAT_CGO_DIR "${TAILCAT_CGO_REAL_SRC_DIR}/../../../tailcat-cgo" ABSOLUTE)

if (ANDROID)
  set(TAILCAT_LIB_NAME "libtailcat_cgo.so")
elseif (WIN32)
  set(TAILCAT_LIB_NAME "tailcat_cgo.dll")
else ()
  set(TAILCAT_LIB_NAME "libtailcat_cgo.dylib")
endif ()

set(TAILCAT_CGO_BUILT_LIB "${CMAKE_CURRENT_BINARY_DIR}/go-build/${TAILCAT_LIB_NAME}")
set(TAILCAT_CGO_OUT_LIB "${CMAKE_CURRENT_BINARY_DIR}/${TAILCAT_LIB_NAME}")

find_program(GO_EXECUTABLE go REQUIRED)

set(GO_ENV_ARGS CGO_ENABLED=1)
if (ANDROID)
  # Map Flutter/NDK's ANDROID_ABI to the GOARCH the Go toolchain expects.
  if (ANDROID_ABI STREQUAL "arm64-v8a")
    set(TAILCAT_GOARCH "arm64")
  elseif (ANDROID_ABI STREQUAL "armeabi-v7a")
    set(TAILCAT_GOARCH "arm")
  elseif (ANDROID_ABI STREQUAL "x86_64")
    set(TAILCAT_GOARCH "amd64")
  else ()
    message(FATAL_ERROR "p2p_lib_flutter: unsupported ANDROID_ABI ${ANDROID_ABI}")
  endif ()
  list(APPEND GO_ENV_ARGS
    GOOS=android
    GOARCH=${TAILCAT_GOARCH}
    CC=${CMAKE_C_COMPILER}
  )
  if (ANDROID_ABI STREQUAL "armeabi-v7a")
    list(APPEND GO_ENV_ARGS GOARM=7)
  endif ()
elseif (WIN32)
  # cgo needs a gcc-family C compiler; Flutter's Windows runner builds with
  # MSVC (cl.exe), which cgo can't drive, so force mingw-w64 gcc instead
  # (same approach as rust/p2p-lib-sys/build.rs's find_mingw_gcc).
  find_program(TAILCAT_MINGW_GCC NAMES x86_64-w64-mingw32-gcc gcc)
  if (NOT TAILCAT_MINGW_GCC)
    message(FATAL_ERROR "p2p_lib_flutter: no mingw-w64 gcc found on PATH (required for cgo on Windows)")
  endif ()
  list(APPEND GO_ENV_ARGS CC=${TAILCAT_MINGW_GCC})
endif ()

add_custom_command(
  OUTPUT "${TAILCAT_CGO_OUT_LIB}"
  COMMAND ${CMAKE_COMMAND} -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/go-build"
  COMMAND ${CMAKE_COMMAND} -E env ${GO_ENV_ARGS}
    ${GO_EXECUTABLE} build "-ldflags=-s -w" -buildmode=c-shared -o "${TAILCAT_CGO_BUILT_LIB}" .
  COMMAND ${CMAKE_COMMAND} -E copy "${TAILCAT_CGO_BUILT_LIB}" "${TAILCAT_CGO_OUT_LIB}"
  WORKING_DIRECTORY "${TAILCAT_CGO_DIR}"
  COMMENT "Building tailcat-cgo (Go cgo shared library)"
  VERBATIM
)
add_custom_target(tailcat_cgo_build ALL DEPENDS "${TAILCAT_CGO_OUT_LIB}")

add_library(p2p_lib_flutter SHARED IMPORTED GLOBAL)
add_dependencies(p2p_lib_flutter tailcat_cgo_build)
set_property(TARGET p2p_lib_flutter PROPERTY IMPORTED_LOCATION "${TAILCAT_CGO_OUT_LIB}")

if (WIN32)
  # mingw-w64 gcc's -buildmode=c-shared only emits the .dll, not an
  # MSVC-compatible .lib import library, but Flutter's Windows runner links
  # with link.exe (MSVC), which needs one to resolve tailcat_cgo's exported
  # symbols at link time. Regenerate it from the DLL's export table via
  # dumpbin (list exports) + lib.exe (build the .lib) -- the same approach
  # rust/p2p-lib-sys/build.rs uses, and both tools ship with the MSVC Build
  # Tools that CMake/Visual Studio itself requires on this platform.
  set(TAILCAT_CGO_DEF "${CMAKE_CURRENT_BINARY_DIR}/tailcat_cgo.def")
  set(TAILCAT_CGO_LIB "${CMAKE_CURRENT_BINARY_DIR}/tailcat_cgo.lib")
  find_program(TAILCAT_DUMPBIN dumpbin)
  find_program(TAILCAT_LIB_EXE lib)
  if (NOT TAILCAT_DUMPBIN OR NOT TAILCAT_LIB_EXE)
    # Not on PATH (the common case outside a "Developer Command Prompt"):
    # ask vswhere -- which ships with Visual Studio/Build Tools installs
    # and is what CMake itself uses to locate MSVC -- for the newest
    # installation, then look for both tools under its VC\Tools\MSVC tree.
    find_program(TAILCAT_VSWHERE vswhere
      PATHS "$ENV{ProgramFiles\(x86\)}/Microsoft Visual Studio/Installer"
            "$ENV{ProgramFiles}/Microsoft Visual Studio/Installer"
    )
    if (TAILCAT_VSWHERE)
      execute_process(
        COMMAND "${TAILCAT_VSWHERE}" -latest -find "VC/Tools/MSVC/*/bin/HostX64/x64/dumpbin.exe"
        OUTPUT_VARIABLE TAILCAT_VSWHERE_DUMPBIN
        OUTPUT_STRIP_TRAILING_WHITESPACE
      )
      execute_process(
        COMMAND "${TAILCAT_VSWHERE}" -latest -find "VC/Tools/MSVC/*/bin/HostX64/x64/lib.exe"
        OUTPUT_VARIABLE TAILCAT_VSWHERE_LIB
        OUTPUT_STRIP_TRAILING_WHITESPACE
      )
      if (TAILCAT_VSWHERE_DUMPBIN AND TAILCAT_VSWHERE_LIB)
        set(TAILCAT_DUMPBIN "${TAILCAT_VSWHERE_DUMPBIN}")
        set(TAILCAT_LIB_EXE "${TAILCAT_VSWHERE_LIB}")
      endif ()
    endif ()
  endif ()
  if (NOT TAILCAT_DUMPBIN OR NOT TAILCAT_LIB_EXE)
    message(FATAL_ERROR "p2p_lib_flutter: dumpbin/lib.exe not found (checked PATH and vswhere) -- ensure Visual Studio/Build Tools with the C++ workload is installed")
  endif ()

  add_custom_command(
    OUTPUT "${TAILCAT_CGO_LIB}"
    COMMAND powershell -NoProfile -Command
      "$exports = & '${TAILCAT_DUMPBIN}' /exports '${TAILCAT_CGO_OUT_LIB}'; \
       $names = $exports | Select-String '^\\\\s+\\\\d+\\\\s+[0-9A-F]+\\\\s+[0-9A-F]+\\\\s+(tailcat_\\\\w+)' | ForEach-Object { $_.Matches[0].Groups[1].Value }; \
       'EXPORTS' | Set-Content '${TAILCAT_CGO_DEF}'; \
       $names | Add-Content '${TAILCAT_CGO_DEF}'; \
       & '${TAILCAT_LIB_EXE}' /def:'${TAILCAT_CGO_DEF}' /out:'${TAILCAT_CGO_LIB}' /machine:X64"
    DEPENDS "${TAILCAT_CGO_OUT_LIB}"
    COMMENT "Generating MSVC import library for tailcat_cgo.dll"
    VERBATIM
  )
  add_custom_target(tailcat_cgo_implib ALL DEPENDS "${TAILCAT_CGO_LIB}")
  add_dependencies(p2p_lib_flutter tailcat_cgo_implib)
  set_property(TARGET p2p_lib_flutter PROPERTY IMPORTED_IMPLIB "${TAILCAT_CGO_LIB}")
endif ()

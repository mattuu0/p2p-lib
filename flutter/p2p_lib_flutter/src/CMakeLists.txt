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
set(TAILCAT_CGO_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../../tailcat-cgo")

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
endif ()

add_custom_command(
  OUTPUT "${TAILCAT_CGO_OUT_LIB}"
  COMMAND ${CMAKE_COMMAND} -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/go-build"
  COMMAND ${CMAKE_COMMAND} -E env ${GO_ENV_ARGS}
    ${GO_EXECUTABLE} build -buildmode=c-shared -o "${TAILCAT_CGO_BUILT_LIB}" .
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
  set_property(TARGET p2p_lib_flutter PROPERTY IMPORTED_IMPLIB "${TAILCAT_CGO_OUT_LIB}")
endif ()

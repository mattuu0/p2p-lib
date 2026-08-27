# The Flutter tooling requires that developers have CMake 3.10 or later
# installed. You should not increase this version, as doing so will cause
# the plugin to fail to compile for some customers of the plugin.
cmake_minimum_required(VERSION 3.10)

# Project-level configuration.
set(PROJECT_NAME "p2p_lib_flutter")
project(${PROJECT_NAME} LANGUAGES CXX)

# This value is used when generating builds using this plugin, so it must
# not be changed.
set(PLUGIN_NAME "p2p_lib_flutter_plugin")

# The actual tailcat-cgo build (shells out to `go build`) and the resulting
# p2p_lib_flutter IMPORTED library target are defined in ../src, shared with
# the Android CMake build (see ../src/CMakeLists.txt's WIN32 branch).
add_subdirectory("${CMAKE_CURRENT_SOURCE_DIR}/../src" "${CMAKE_CURRENT_BINARY_DIR}/tailcat_cgo")

# This plugin has no Dart-facing platform channel/method-channel code of its
# own (the whole API is dart:ffi calls straight into tailcat_cgo), so the
# "plugin" target Flutter's Windows runner links against/bundles IS the
# imported tailcat_cgo.dll from ../src -- an ALIAS (not a wrapper target)
# so generator expressions like $<TARGET_FILE:...> resolve to the real
# imported DLL rather than an interface library with no file of its own.
add_library(${PLUGIN_NAME} ALIAS p2p_lib_flutter)

# List of absolute paths to libraries that should be bundled with the plugin.
# This list could contain prebuilt libraries, or libraries created by an
# external build triggered from this CMakeLists.txt.
set(p2p_lib_flutter_bundled_libraries
  "$<TARGET_FILE:p2p_lib_flutter>"
  PARENT_SCOPE
)

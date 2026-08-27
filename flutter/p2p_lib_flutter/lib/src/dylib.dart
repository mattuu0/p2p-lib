import 'dart:ffi';
import 'dart:io';

import 'tailcat_cgo_bindings_generated.dart';

const String _libName = 'tailcat_cgo';

/// The dynamic library that exports the tailcat-cgo C ABI symbols.
///
/// On macOS this is a plain dylib built by [macos/p2p_lib_flutter.podspec]'s
/// prepare_command. On iOS the same symbols are statically linked into the
/// app binary by [ios/p2p_lib_flutter.podspec] (iOS forbids third-party
/// dynamic libraries), so we look them up in the process image itself
/// instead of opening a named library. On Android/Linux/Windows the shared
/// library built by [src/CMakeLists.txt] is loaded by name.
final DynamicLibrary tailcatDylib = () {
  if (Platform.isIOS) {
    return DynamicLibrary.process();
  }
  if (Platform.isMacOS) {
    final override = Platform.environment['TAILCAT_CGO_DYLIB_PATH'];
    if (override != null) return DynamicLibrary.open(override);
    // The dylib is bundled into Contents/Frameworks by the podspec's
    // vendored_libraries, and the app binary carries an
    // `@executable_path/../Frameworks` rpath entry -- but that rpath is
    // only consulted for `@rpath/...`-prefixed install names, not for a
    // bare filename passed to dlopen. Open it explicitly via @rpath.
    return DynamicLibrary.open('@rpath/lib$_libName.dylib');
  }
  if (Platform.isAndroid || Platform.isLinux) {
    return DynamicLibrary.open('lib$_libName.so');
  }
  if (Platform.isWindows) {
    return DynamicLibrary.open('$_libName.dll');
  }
  throw UnsupportedError('Unknown platform: ${Platform.operatingSystem}');
}();

/// The generated low-level bindings to [tailcatDylib].
///
/// Prefer the high-level [Server]/[Client]/[Conn] wrappers in this package
/// over calling these directly -- every tailcat_* call blocks the calling
/// thread (network I/O, cgo transitions), so the high-level API always
/// dispatches through a background isolate.
final TailcatCgoBindings tailcatBindings = TailcatCgoBindings(tailcatDylib);

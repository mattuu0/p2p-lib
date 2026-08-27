import 'dart:ffi';

import 'package:ffi/ffi.dart';

import 'dylib.dart';
import 'errors.dart';

/// Reads and frees the last error message recorded for `handle` (or the
/// global slot, handle 0, for calls made before a handle exists).
String? lastError(int handle) {
  final ptr = tailcatBindings.tailcat_last_error(handle);
  return _takeString(ptr);
}

/// Builds the exception to throw for a failed call on `handle`: the
/// recorded error message if there was one, else [InvalidHandleError].
TailcatException takeError(int handle) {
  final msg = lastError(handle);
  return msg == null ? const InvalidHandleError() : TailcatError(msg);
}

/// Takes ownership of a `Pointer<Char>` returned by the Go library, copies
/// it into a Dart [String], and frees the original via
/// `tailcat_free_string`. Returns `null` for a NULL pointer.
String? _takeString(Pointer<Char> ptr) {
  if (ptr == nullptr) return null;
  final s = ptr.cast<Utf8>().toDartString();
  tailcatBindings.tailcat_free_string(ptr);
  return s;
}

/// Like [_takeString], but throws the given handle's last recorded error
/// instead of returning `null`, for functions where NULL always means
/// failure.
String takeStringOrThrow(Pointer<Char> ptr, int errHandle) {
  final s = _takeString(ptr);
  if (s != null) return s;
  throw takeError(errHandle);
}

/// Converts a Dart [String] to a NUL-terminated native UTF-8 buffer,
/// running `body` with it and freeing it afterwards -- the FFI analogue of
/// Rust's short-lived `CString`.
R withCString<R>(String s, R Function(Pointer<Char> ptr) body) {
  final ptr = s.toNativeUtf8().cast<Char>();
  try {
    return body(ptr);
  } finally {
    calloc.free(ptr);
  }
}

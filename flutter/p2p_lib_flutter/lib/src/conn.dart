import 'dart:ffi';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'dylib.dart';
import 'errors.dart';
import 'ffi_helpers.dart';

/// A TCP-like connection tunneled over WireGuard via a tailcat [Server]
/// (accepted) or [Client] (dialed).
///
/// Mirrors `rust/p2p-lib/src/conn.rs`'s `Conn`. Every method here does
/// blocking native I/O, so it always runs the actual `tailcat_conn_*` call
/// on a background isolate via [Isolate.run] -- never call these from a
/// frame callback expecting an instant return.
class Conn {
  int _handle;

  Conn.fromHandle(this._handle);

  bool get isClosed => _handle == 0;

  /// Reads up to `maxLength` bytes. Returns an empty list on clean EOF from
  /// the peer.
  Future<Uint8List> read(int maxLength) async {
    final handle = _requireHandle();
    return Isolate.run(() => _readSync(handle, maxLength));
  }

  /// Writes all of `data`, issuing repeated native writes if the native
  /// layer accepts fewer bytes than requested in one call.
  Future<void> writeAll(Uint8List data) async {
    final handle = _requireHandle();
    await Isolate.run(() => _writeAllSync(handle, data));
  }

  /// Half-closes the writing side (sends a TCP FIN) while leaving the read
  /// side open, so the peer sees EOF without losing your ability to read
  /// its reply. See the crate-level note in the Rust implementation on why
  /// this matters: the whole TCP stack runs in-process (gVisor netstack),
  /// so there's no OS buffer to flush a pending FIN after a full close.
  Future<void> closeWrite() async {
    final handle = _requireHandle();
    await Isolate.run(() {
      final rc = tailcatBindings.tailcat_conn_close_write(handle);
      if (rc != 0) throw takeError(handle);
    });
  }

  /// Fully closes the connection. Safe to call more than once.
  Future<void> close() async {
    if (_handle == 0) return;
    final handle = _handle;
    _handle = 0;
    await Isolate.run(() {
      final rc = tailcatBindings.tailcat_conn_close(handle);
      if (rc != 0) throw takeError(handle);
    });
  }

  int _requireHandle() {
    if (_handle == 0) throw const InvalidHandleError();
    return _handle;
  }
}

Uint8List _readSync(int handle, int maxLength) {
  final buf = calloc<Uint8>(maxLength);
  try {
    final n = tailcatBindings.tailcat_conn_read(handle, buf, maxLength);
    if (n == -2) return Uint8List(0); // clean EOF, per tailcat_conn_read's contract
    if (n < 0) throw takeError(handle);
    return Uint8List.fromList(buf.asTypedList(n));
  } finally {
    calloc.free(buf);
  }
}

void _writeAllSync(int handle, Uint8List data) {
  if (data.isEmpty) return;
  final buf = calloc<Uint8>(data.length);
  try {
    buf.asTypedList(data.length).setAll(0, data);
    var offset = 0;
    while (offset < data.length) {
      final n = tailcatBindings.tailcat_conn_write(
        handle,
        (buf + offset).cast(),
        data.length - offset,
      );
      if (n < 0) throw takeError(handle);
      offset += n;
    }
  } finally {
    calloc.free(buf);
  }
}

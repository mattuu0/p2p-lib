import 'dart:isolate';

import 'conn.dart';
import 'dylib.dart';
import 'errors.dart';
import 'ffi_helpers.dart';
import 'keys.dart';

/// Connects to a [Server] over a WireGuard tunnel relayed through DERP,
/// given the connection token the server printed.
///
/// Mirrors `rust/p2p-lib/src/client.rs`'s `Client`. The tunnel is
/// established lazily on first use ([ping] or [dialTcpPort]).
class Client {
  int _handle;

  Client._(this._handle);

  bool get isClosed => _handle == 0;

  /// Creates a client that will connect to the server identified by
  /// `connBlob`, using a fresh ephemeral identity.
  static Future<Client> create(String connBlob) async {
    final handle = await Isolate.run(
      () => withCString(connBlob, (ptr) => tailcatBindings.tailcat_client_new(ptr)),
    );
    if (handle == 0) throw takeError(0);
    return Client._(handle);
  }

  /// Like [create], but uses a previously saved identity key, so a server
  /// can allow-list this client ahead of time (see `Server.allowClient`).
  static Future<Client> createWithKey(String connBlob, PrivateKey key) async {
    final handle = await Isolate.run(
      () => withCString(
        connBlob,
        (blobPtr) => withCString(
          key.json,
          (keyPtr) => tailcatBindings.tailcat_client_new_with_key(blobPtr, keyPtr),
        ),
      ),
    );
    if (handle == 0) throw takeError(0);
    return Client._(handle);
  }

  /// This client's node public key (text form), generating it on first
  /// call if needed. Useful to show your identity to a user, or to give to
  /// a server operator for allow-listing.
  Future<String> publicKey() async {
    final handle = _requireHandle();
    return Isolate.run(() {
      final ptr = tailcatBindings.tailcat_client_public_key(handle);
      return takeStringOrThrow(ptr, handle);
    });
  }

  /// Starts the client if needed, performs the handshake with the server,
  /// and returns the round-trip latency. Calling it explicitly is
  /// optional -- [dialTcpPort] does it implicitly -- but useful to test
  /// connectivity or measure latency first.
  Future<Duration> ping(Duration timeout) async {
    final handle = _requireHandle();
    final latencyMs = await Isolate.run(
      () => tailcatBindings.tailcat_client_ping(handle, timeout.inMilliseconds),
    );
    if (latencyMs < 0) throw takeError(handle);
    return Duration(milliseconds: latencyMs);
  }

  /// Opens a TCP connection to the given port on the server, starting the
  /// client first if needed.
  Future<Conn> dialTcpPort(int port, Duration timeout) async {
    final handle = _requireHandle();
    final connHandle = await Isolate.run(
      () => tailcatBindings.tailcat_client_dial_tcp_port(handle, port, timeout.inMilliseconds),
    );
    if (connHandle == 0) throw takeError(handle);
    return Conn.fromHandle(connHandle);
  }

  /// Shuts the client down. Safe to call more than once.
  Future<void> close() async {
    if (_handle == 0) return;
    final handle = _handle;
    _handle = 0;
    await Isolate.run(() {
      final rc = tailcatBindings.tailcat_client_close(handle);
      if (rc != 0) throw takeError(handle);
    });
  }

  int _requireHandle() {
    if (_handle == 0) throw const InvalidHandleError();
    return _handle;
  }
}

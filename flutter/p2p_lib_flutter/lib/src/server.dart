import 'dart:isolate';

import 'conn.dart';
import 'dylib.dart';
import 'errors.dart';
import 'ffi_helpers.dart';
import 'keys.dart';

/// Listens for tailcat clients over a WireGuard tunnel relayed through
/// DERP, with no control plane or Tailscale account required.
///
/// Mirrors `rust/p2p-lib/src/server.rs`'s `Server`. Construct with
/// [Server.create] (fresh ephemeral identity) or [Server.createWithKey] (a
/// previously persisted [PrivateKey]), then call [start], share the
/// [connBlob] token with clients out of band, and call [accept] in a loop.
class Server {
  int _handle;

  Server._(this._handle);

  bool get isClosed => _handle == 0;

  /// Creates a server with a fresh ephemeral key; the token it prints
  /// after [start] is unique to this run and unusable again once the
  /// process exits.
  static Future<Server> create() async {
    final handle = await Isolate.run(
      () => tailcatBindings.tailcat_server_new(),
    );
    if (handle == 0) throw takeError(0);
    return Server._(handle);
  }

  /// Creates a server using a previously saved identity, so its address
  /// token stays stable across restarts.
  static Future<Server> createWithKey(PrivateKey key) async {
    final handle = await Isolate.run(
      () => withCString(
        key.json,
        (ptr) => tailcatBindings.tailcat_server_new_with_key(ptr),
      ),
    );
    if (handle == 0) throw takeError(0);
    return Server._(handle);
  }

  /// Restricts the server to only accept the given client's public key (as
  /// returned by [PrivateKey.publicKey] or [Client.publicKey]). Must be
  /// called before [start]; call repeatedly to allow more than one client.
  /// If never called, all clients are allowed.
  Future<void> allowClient(String publicKey) async {
    final handle = _requireHandle();
    await Isolate.run(() {
      final rc = withCString(
        publicKey,
        (ptr) => tailcatBindings.tailcat_server_set_allowed_client(handle, ptr),
      );
      if (rc != 0) throw takeError(handle);
    });
  }

  /// Connects to the DERP relay and begins accepting clients.
  Future<void> start() async {
    final handle = _requireHandle();
    await Isolate.run(() {
      final rc = tailcatBindings.tailcat_server_start(handle);
      if (rc != 0) throw takeError(handle);
    });
  }

  /// The token clients pass to [Client.create] to connect to this server.
  /// Must be called after [start].
  Future<String> connBlob() async {
    final handle = _requireHandle();
    return Isolate.run(() {
      final ptr = tailcatBindings.tailcat_server_connblob(handle);
      return takeStringOrThrow(ptr, handle);
    });
  }

  /// Blocks (on a background isolate) until a client connects or
  /// [timeout] elapses, returning `null` on a plain timeout.
  Future<Conn?> accept(Duration timeout) async {
    final handle = _requireHandle();
    final connHandle = await Isolate.run(
      () => tailcatBindings.tailcat_server_accept(handle, timeout.inMilliseconds),
    );
    if (connHandle == 0) {
      // Distinguish a timeout (no error recorded) from a real error.
      final msg = lastError(handle);
      if (msg != null) throw TailcatError(msg);
      return null;
    }
    return Conn.fromHandle(connHandle);
  }

  /// Returns this server's identity and allow-list bundled as one JSON
  /// string, so a caller can persist everything needed to resume this
  /// server later. Treat the JSON shape as opaque unless you need to
  /// inspect it.
  Future<String> stateJson() async {
    final handle = _requireHandle();
    return Isolate.run(() {
      final ptr = tailcatBindings.tailcat_server_state(handle);
      return takeStringOrThrow(ptr, handle);
    });
  }

  /// Shuts the server down, draining in-flight TCP data first. Safe to
  /// call more than once.
  Future<void> close() async {
    if (_handle == 0) return;
    final handle = _handle;
    _handle = 0;
    await Isolate.run(() {
      final rc = tailcatBindings.tailcat_server_close(handle);
      if (rc != 0) throw takeError(handle);
    });
  }

  int _requireHandle() {
    if (_handle == 0) throw const InvalidHandleError();
    return _handle;
  }
}

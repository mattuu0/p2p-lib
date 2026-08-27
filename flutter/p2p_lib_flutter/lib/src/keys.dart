import 'dart:isolate';

import 'dylib.dart';
import 'ffi_helpers.dart';

/// A tailcat identity key pair, serialized as opaque JSON.
///
/// Mirrors `rust/p2p-lib/src/keys.rs`'s `PrivateKey`. Persist [json]
/// yourself (Keychain, Android Keystore, a file, IndexedDB, ...) to give a
/// [Server] or [Client] a stable identity across restarts -- this package
/// never touches disk on its own.
class PrivateKey {
  /// The opaque JSON serialization of this key pair. Persist and reload
  /// this verbatim; do not attempt to parse or construct it by hand.
  final String json;

  const PrivateKey(this.json);

  /// Generates a fresh key pair.
  static Future<PrivateKey> generate() async {
    final json = await Isolate.run(() {
      final ptr = tailcatBindings.tailcat_privatekey_generate();
      return takeStringOrThrow(ptr, 0);
    });
    return PrivateKey(json);
  }

  /// This key pair's public key (text form), safe to share with a peer for
  /// allow-listing (see `Server.allowClient`).
  Future<String> publicKey() async {
    return Isolate.run(() {
      final ptr = withCString(
        json,
        (ptr) => tailcatBindings.tailcat_privatekey_public_key(ptr),
      );
      return takeStringOrThrow(ptr, 0);
    });
  }
}

/// Decodes a connection blob (the token returned by `Server.connBlob`)
/// into a human-readable JSON description, useful for debugging/display.
Future<String> resolveConnBlob(String connBlob) async {
  return Isolate.run(() {
    final ptr = withCString(
      connBlob,
      (ptr) => tailcatBindings.tailcat_connblob_resolve(ptr),
    );
    return takeStringOrThrow(ptr, 0);
  });
}

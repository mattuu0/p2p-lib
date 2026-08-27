/// Flutter FFI bindings for tailcat-cgo: WireGuard + DERP relayed P2P
/// networking with no control plane or account required.
///
/// See [Server], [Client], [Conn], and [PrivateKey] for the high-level API
/// (mirrors `rust/p2p-lib`'s Rust crate of the same shape).
library;

export 'src/client.dart' show Client;
export 'src/conn.dart' show Conn;
export 'src/errors.dart' show InvalidHandleError, TailcatError, TailcatException;
export 'src/keys.dart' show PrivateKey, resolveConnBlob;
export 'src/server.dart' show Server;

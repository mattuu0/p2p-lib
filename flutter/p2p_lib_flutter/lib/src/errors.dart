/// Errors returned by this package's [Server]/[Client]/[Conn] operations.
///
/// Mirrors `rust/p2p-lib/src/error.rs`'s `Error` enum.
sealed class TailcatException implements Exception {
  const TailcatException();
}

/// An error message surfaced by the underlying tailcat/Go layer.
class TailcatError extends TailcatException {
  final String message;
  const TailcatError(this.message);

  @override
  String toString() => 'TailcatError: $message';
}

/// A handle was used after it was already closed, or a native call failed
/// without recording an error message.
class InvalidHandleError extends TailcatException {
  const InvalidHandleError();

  @override
  String toString() => 'InvalidHandleError: invalid handle (already closed?)';
}

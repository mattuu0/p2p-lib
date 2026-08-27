// Clean, hand-written declarations of the tailcat-cgo C ABI, used as the
// ffigen entry point for Dart bindings. The actual symbols are provided at
// link/load time by the Go cgo shared library built from
// ../../../tailcat-cgo (see CMakeLists.txt / the macOS and iOS podspecs),
// not by any C file in this plugin.
#ifndef TAILCAT_CGO_H
#define TAILCAT_CGO_H

#include <stdint.h>

#if _WIN32
#define FFI_PLUGIN_EXPORT __declspec(dllimport)
#else
#define FFI_PLUGIN_EXPORT
#endif

#ifdef __cplusplus
extern "C" {
#endif

// Frees a string previously returned by any tailcat_* function.
FFI_PLUGIN_EXPORT void tailcat_free_string(char* s);

// Returns the last error message recorded for `handle`, or an empty string
// if there was none. The returned string must be freed with
// tailcat_free_string.
FFI_PLUGIN_EXPORT char* tailcat_last_error(int64_t handle);

// Generates a new private key and returns it serialized as JSON. The
// returned string must be freed with tailcat_free_string.
FFI_PLUGIN_EXPORT char* tailcat_privatekey_generate(void);

// Returns the public key portion of a JSON-serialized private key. The
// returned string must be freed with tailcat_free_string.
FFI_PLUGIN_EXPORT char* tailcat_privatekey_public_key(char* privateKeyJSON);

// Decodes a connection blob into a human-readable JSON description. The
// returned string must be freed with tailcat_free_string.
FFI_PLUGIN_EXPORT char* tailcat_connblob_resolve(char* connBlob);

// Creates a new server with a freshly generated key pair. Returns an opaque
// handle, or 0 on failure.
FFI_PLUGIN_EXPORT int64_t tailcat_server_new(void);

// Creates a new server using the given JSON-serialized private key. Returns
// an opaque handle, or 0 on failure.
FFI_PLUGIN_EXPORT int64_t tailcat_server_new_with_key(char* privateKeyJSON);

// Adds a client public key to the server's allow-list. Returns 0 on
// success, non-zero on failure (see tailcat_last_error).
FFI_PLUGIN_EXPORT int tailcat_server_set_allowed_client(int64_t handle, char* pubKeyText);

// Starts the server's networking. Returns 0 on success, non-zero on
// failure (see tailcat_last_error).
FFI_PLUGIN_EXPORT int tailcat_server_start(int64_t handle);

// Blocks up to timeoutMs waiting for an incoming connection. Returns an
// opaque connection handle, 0 on timeout, or a negative value on error (see
// tailcat_last_error).
FFI_PLUGIN_EXPORT int64_t tailcat_server_accept(int64_t handle, int64_t timeoutMs);

// Returns the connection blob (token) that clients use to reach this
// server. The returned string must be freed with tailcat_free_string.
FFI_PLUGIN_EXPORT char* tailcat_server_connblob(int64_t handle);

// Returns the server's current state (e.g. allowed clients) serialized as
// JSON. The returned string must be freed with tailcat_free_string.
FFI_PLUGIN_EXPORT char* tailcat_server_state(int64_t handle);

// Closes the server and releases its handle. Returns 0 on success.
FFI_PLUGIN_EXPORT int tailcat_server_close(int64_t handle);

// Creates a new client with a freshly generated key pair, targeting the
// given connection blob. Returns an opaque handle, or 0 on failure.
FFI_PLUGIN_EXPORT int64_t tailcat_client_new(char* connBlob);

// Creates a new client using the given JSON-serialized private key,
// targeting the given connection blob. Returns an opaque handle, or 0 on
// failure.
FFI_PLUGIN_EXPORT int64_t tailcat_client_new_with_key(char* connBlob, char* privateKeyJSON);

// Returns the client's own public key serialized as JSON. The returned
// string must be freed with tailcat_free_string.
FFI_PLUGIN_EXPORT char* tailcat_client_public_key(int64_t handle);

// Pings the server and returns the round-trip latency in milliseconds, or
// a negative value on error/timeout (see tailcat_last_error).
FFI_PLUGIN_EXPORT int64_t tailcat_client_ping(int64_t handle, int64_t timeoutMs);

// Dials a TCP port on the server. Returns an opaque connection handle, or 0
// on failure (see tailcat_last_error).
FFI_PLUGIN_EXPORT int64_t tailcat_client_dial_tcp_port(int64_t handle, int port, int64_t timeoutMs);

// Closes the client and releases its handle. Returns 0 on success.
FFI_PLUGIN_EXPORT int tailcat_client_close(int64_t handle);

// Reads up to `length` bytes from the connection into `buf`. Returns the
// number of bytes read, -2 on clean EOF, or another negative value on error
// (see tailcat_last_error).
FFI_PLUGIN_EXPORT int tailcat_conn_read(int64_t handle, uint8_t* buf, int length);

// Writes `length` bytes from `buf` to the connection. Returns the number of
// bytes written, or a negative value on error (see tailcat_last_error).
FFI_PLUGIN_EXPORT int tailcat_conn_write(int64_t handle, uint8_t* buf, int length);

// Half-closes the connection's write side (sends FIN / TCP CloseWrite),
// leaving the read side open. Returns 0 on success.
FFI_PLUGIN_EXPORT int tailcat_conn_close_write(int64_t handle);

// Fully closes the connection and releases its handle. Returns 0 on
// success.
FFI_PLUGIN_EXPORT int tailcat_conn_close(int64_t handle);

#ifdef __cplusplus
}
#endif

#endif  // TAILCAT_CGO_H

// Package main builds a C ABI shared library (DLL/so/dylib) wrapping
// github.com/tailscale/tailcat's Server/Client API, so it can be consumed
// from Rust and, in principle, any language with a C FFI.
//
// Design notes (see the project plan for the full rationale):
//   - Every exported function blocks and returns a result synchronously; a
//     cgo call already runs on its own OS thread, so blocking here doesn't
//     stall the Go runtime, and callers use their own language's async
//     primitives (e.g. Rust's spawn_blocking) around these calls.
//   - Objects (Server, Client, net.Conn) are referenced by opaque int64
//     handles (see handle.go), never raw pointers, so Go's GC keeps them
//     alive correctly across the cgo boundary.
//   - Errors are reported via the function's return value (0/negative
//     handle, or a negative error code) plus a companion
//     tailcat_last_error(handle) call for the human-readable message.
//   - Key/connection persistence crosses the boundary as JSON strings only
//     (see keys.go); this library never touches the filesystem itself.
package main

/*
#include <stdlib.h>
*/
import "C"

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"time"
	"unsafe"

	"github.com/tailscale/tailcat"
	"tailscale.com/types/key"
)

func main() {} // required by -buildmode=c-shared, unused

// tailcat_free_string releases a *C.char previously returned by any
// function in this library. Callers must call this exactly once for every
// non-NULL string they receive.
//
//export tailcat_free_string
func tailcat_free_string(s *C.char) {
	C.free(unsafe.Pointer(s))
}

// tailcat_last_error returns the most recent error message recorded for
// handle (or the global slot if handle is 0, e.g. for constructors that
// failed before a handle existed), or NULL if there is none. The returned
// string must be freed with tailcat_free_string.
//
//export tailcat_last_error
func tailcat_last_error(handle C.longlong) *C.char {
	msg := getLastError(Handle(handle))
	if msg == "" {
		return nil
	}
	return C.CString(msg)
}

// ---- PrivateKey ----

// tailcat_privatekey_generate returns a new PrivateKey as a JSON string
// (see keys.go for the shape). The caller owns the returned string.
//
//export tailcat_privatekey_generate
func tailcat_privatekey_generate() *C.char {
	pk := tailcat.NewPrivateKey()
	s, err := marshalPrivateKey(pk)
	if err != nil {
		setLastError(errGlobal, err)
		return nil
	}
	return C.CString(s)
}

// tailcat_privatekey_public_key returns the public key (text form, e.g.
// "nodekey:...") encoded in privateKeyJSON, for building allow-lists.
//
//export tailcat_privatekey_public_key
func tailcat_privatekey_public_key(privateKeyJSON *C.char) *C.char {
	pk, err := unmarshalPrivateKey(C.GoString(privateKeyJSON))
	if err != nil {
		setLastError(errGlobal, err)
		return nil
	}
	return C.CString(pk.Public.ServerPublic.String())
}

// ---- ConnBlob ----

// tailcat_connblob_resolve returns a self-contained ConnBlob with DERP
// relay details embedded (see tailcat.ConnBlob.Resolve), so it can be
// persisted and reused without a network fetch. Returns NULL on error.
//
//export tailcat_connblob_resolve
func tailcat_connblob_resolve(connBlob *C.char) *C.char {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	resolved, err := tailcat.ConnBlob(C.GoString(connBlob)).Resolve(ctx)
	if err != nil {
		setLastError(errGlobal, err)
		return nil
	}
	return C.CString(string(resolved))
}

// ---- Server ----

// tailcat_server_new creates a Server with a fresh ephemeral key. Returns 0
// on error (see tailcat_last_error(0)).
//
//export tailcat_server_new
func tailcat_server_new() C.longlong {
	return newServer(nil)
}

// tailcat_server_new_with_key creates a Server using a previously saved
// key (see tailcat_privatekey_generate / tailcat_server_state). Returns 0
// on error.
//
//export tailcat_server_new_with_key
func tailcat_server_new_with_key(privateKeyJSON *C.char) C.longlong {
	pk, err := unmarshalPrivateKey(C.GoString(privateKeyJSON))
	if err != nil {
		setLastError(errGlobal, err)
		return 0
	}
	return newServer(pk)
}

func newServer(pk *tailcat.PrivateKey) C.longlong {
	e := &serverEntry{
		srv:    &tailcat.Server{},
		accept: make(chan net.Conn, 16),
	}
	if pk != nil {
		e.srv.Key = pk.Private
	}
	h := reg.put(e)
	return C.longlong(h)
}

// tailcat_server_set_allowed_client restricts the server to only accept
// the given client public key (text form). Must be called before
// tailcat_server_start; repeat calls add more allowed clients. Returns 0
// on success, -1 on error.
//
//export tailcat_server_set_allowed_client
func tailcat_server_set_allowed_client(handle C.longlong, pubKeyText *C.char) C.int {
	h := Handle(handle)
	e, ok := getServer(h)
	if !ok {
		setLastError(h, errors.New("invalid server handle"))
		return -1
	}
	var pub key.NodePublic
	if err := pub.UnmarshalText([]byte(C.GoString(pubKeyText))); err != nil {
		setLastError(h, fmt.Errorf("parse public key: %w", err))
		return -1
	}
	e.srv.AllowedClients = append(e.srv.AllowedClients, pub)
	return 0
}

// tailcat_server_start brings the server's network stack up and begins
// accepting clients. Accepted TCP connections (to any port) are queued
// internally; retrieve them with tailcat_server_accept. Returns 0 on
// success, -1 on error.
//
//export tailcat_server_start
func tailcat_server_start(handle C.longlong) C.int {
	h := Handle(handle)
	e, ok := getServer(h)
	if !ok {
		setLastError(h, errors.New("invalid server handle"))
		return -1
	}
	e.srv.OnTCP = func(port uint16) func(net.Conn) {
		return func(c net.Conn) { e.accept <- c }
	}
	if err := e.srv.Start(); err != nil {
		setLastError(h, err)
		return -1
	}
	return 0
}

// tailcat_server_accept blocks until a client connection arrives or
// timeoutMs elapses, returning a new Conn handle, or 0 on timeout/error
// (distinguish via tailcat_last_error, which is empty on a plain timeout).
//
//export tailcat_server_accept
func tailcat_server_accept(handle C.longlong, timeoutMs C.longlong) C.longlong {
	h := Handle(handle)
	e, ok := getServer(h)
	if !ok {
		setLastError(h, errors.New("invalid server handle"))
		return 0
	}
	setLastError(h, nil)
	select {
	case c := <-e.accept:
		return C.longlong(reg.put(c))
	case <-time.After(time.Duration(timeoutMs) * time.Millisecond):
		return 0 // timeout, not an error
	}
}

// tailcat_server_connblob returns the token clients use to connect to this
// server. Must be called after tailcat_server_start.
//
//export tailcat_server_connblob
func tailcat_server_connblob(handle C.longlong) *C.char {
	h := Handle(handle)
	e, ok := getServer(h)
	if !ok {
		setLastError(h, errors.New("invalid server handle"))
		return nil
	}
	return C.CString(string(e.srv.ConnBlob()))
}

// tailcat_server_state returns this server's PrivateKey and allowed-client
// list bundled as one JSON string (see keys.go serverStateJSON), so a
// caller can persist everything needed to resume this identity in a
// single call. The caller is responsible for where/how it's stored.
//
//export tailcat_server_state
func tailcat_server_state(handle C.longlong) *C.char {
	h := Handle(handle)
	e, ok := getServer(h)
	if !ok {
		setLastError(h, errors.New("invalid server handle"))
		return nil
	}
	privText, err := e.srv.Key.MarshalText()
	if err != nil {
		setLastError(h, err)
		return nil
	}
	pubText, err := e.srv.Key.Public().MarshalText()
	if err != nil {
		setLastError(h, err)
		return nil
	}
	state := serverStateJSON{
		PrivateKey: privateKeyJSON{
			Private: string(privText),
			Public:  connInfoJSON{ServerPublic: string(pubText)},
		},
	}
	for _, k := range e.srv.AllowedClients {
		t, err := k.MarshalText()
		if err != nil {
			setLastError(h, err)
			return nil
		}
		state.AllowedClients = append(state.AllowedClients, string(t))
	}
	b, err := jsonMarshal(state)
	if err != nil {
		setLastError(h, err)
		return nil
	}
	return C.CString(b)
}

// tailcat_server_close shuts the server down, closing its WireGuard engine
// and DERP connection, draining in-flight TCP data first (see the plan's
// notes on tailcat.Server.DrainTCP) so a caller that exits its process
// right after this call doesn't lose data still in flight. Returns 0 on
// success (idempotent), -1 on error.
//
//export tailcat_server_close
func tailcat_server_close(handle C.longlong) C.int {
	h := Handle(handle)
	e, ok := getServer(h)
	if !ok {
		setLastError(h, errors.New("invalid server handle"))
		return -1
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	e.srv.DrainTCP(ctx) // best-effort; ignore timeout/errors, we're closing anyway
	err := e.srv.Close()
	reg.delete(h)
	if err != nil {
		setLastError(h, err)
		return -1
	}
	return 0
}

// ---- Client ----

// tailcat_client_new creates a Client for the given server token, using a
// fresh ephemeral identity key. Returns 0 on error.
//
//export tailcat_client_new
func tailcat_client_new(connBlob *C.char) C.longlong {
	return newClient(connBlob, nil)
}

// tailcat_client_new_with_key is like tailcat_client_new but uses a
// previously saved identity key, e.g. so a server can allow-list it ahead
// of time.
//
//export tailcat_client_new_with_key
func tailcat_client_new_with_key(connBlob *C.char, privateKeyJSON *C.char) C.longlong {
	pk, err := unmarshalPrivateKey(C.GoString(privateKeyJSON))
	if err != nil {
		setLastError(errGlobal, err)
		return 0
	}
	return newClient(connBlob, pk)
}

func newClient(connBlob *C.char, pk *tailcat.PrivateKey) C.longlong {
	cl := tailcat.NewClient(tailcat.ConnBlob(C.GoString(connBlob)))
	if pk != nil {
		cl.Key = pk.Private
	}
	h := reg.put(cl)
	return C.longlong(h)
}

// tailcat_client_public_key returns this client's node public key (text
// form), generating it if this is the first call. Useful for a caller that
// wants to show its own identity to a user before connecting.
//
//export tailcat_client_public_key
func tailcat_client_public_key(handle C.longlong) *C.char {
	h := Handle(handle)
	cl, ok := getClient(h)
	if !ok {
		setLastError(h, errors.New("invalid client handle"))
		return nil
	}
	return C.CString(cl.PublicKey().String())
}

// tailcat_client_ping starts the client if needed, performs the meow
// handshake, and returns the round-trip latency in milliseconds, or -1 on
// error/timeout.
//
//export tailcat_client_ping
func tailcat_client_ping(handle C.longlong, timeoutMs C.longlong) C.longlong {
	h := Handle(handle)
	cl, ok := getClient(h)
	if !ok {
		setLastError(h, errors.New("invalid client handle"))
		return -1
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Duration(timeoutMs)*time.Millisecond)
	defer cancel()
	res, err := cl.Ping(ctx)
	if err != nil {
		setLastError(h, err)
		return -1
	}
	return C.longlong(res.Latency.Milliseconds())
}

// tailcat_client_dial_tcp_port starts the client if needed and opens a TCP
// connection to the given port on the server, returning a new Conn handle,
// or 0 on error.
//
//export tailcat_client_dial_tcp_port
func tailcat_client_dial_tcp_port(handle C.longlong, port C.int, timeoutMs C.longlong) C.longlong {
	h := Handle(handle)
	cl, ok := getClient(h)
	if !ok {
		setLastError(h, errors.New("invalid client handle"))
		return 0
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Duration(timeoutMs)*time.Millisecond)
	defer cancel()
	conn, err := cl.DialTCPPort(ctx, uint16(port))
	if err != nil {
		setLastError(h, err)
		return 0
	}
	return C.longlong(reg.put(conn))
}

// tailcat_client_close shuts the client down. Returns 0 on success
// (idempotent), -1 on error.
//
//export tailcat_client_close
func tailcat_client_close(handle C.longlong) C.int {
	h := Handle(handle)
	cl, ok := getClient(h)
	if !ok {
		setLastError(h, errors.New("invalid client handle"))
		return -1
	}
	err := cl.Close()
	reg.delete(h)
	if err != nil {
		setLastError(h, err)
		return -1
	}
	return 0
}

// ---- Conn (shared by server-accepted and client-dialed connections) ----

// tailcat_conn_read reads up to len(buf) bytes into buf, returning the
// number of bytes read (which may be 0 with no error, per io.Reader),
// -2 if the peer cleanly closed the connection (io.EOF; not an error
// condition), or -1 on any other error (see tailcat_last_error).
//
//export tailcat_conn_read
func tailcat_conn_read(handle C.longlong, buf *C.uchar, length C.int) C.int {
	h := Handle(handle)
	c, ok := getConn(h)
	if !ok {
		setLastError(h, errors.New("invalid conn handle"))
		return -1
	}
	if length <= 0 {
		return 0
	}
	goBuf := unsafe.Slice((*byte)(buf), int(length))
	n, err := c.Read(goBuf)
	if n > 0 {
		// io.Reader permits returning n > 0 alongside a non-nil err (e.g.
		// data followed immediately by EOF); hand the bytes back now and
		// let the next read report the EOF/error with n == 0.
		return C.int(n)
	}
	if err != nil {
		if errors.Is(err, io.EOF) {
			return -2
		}
		setLastError(h, err)
		return -1
	}
	return 0
}

// tailcat_conn_write writes len(buf) bytes from buf, returning the number
// of bytes written, or -1 on error.
//
//export tailcat_conn_write
func tailcat_conn_write(handle C.longlong, buf *C.uchar, length C.int) C.int {
	h := Handle(handle)
	c, ok := getConn(h)
	if !ok {
		setLastError(h, errors.New("invalid conn handle"))
		return -1
	}
	if length <= 0 {
		return 0
	}
	goBuf := unsafe.Slice((*byte)(buf), int(length))
	n, err := c.Write(goBuf)
	if err != nil {
		setLastError(h, err)
		return -1
	}
	return C.int(n)
}

// closeWriter is implemented by TCP-like connections (net.TCPConn,
// gVisor's gonet.TCPConn) that can shut down just their writing side,
// mirroring tailcat.go's own closeWriter interface used by the CLI.
type closeWriter interface {
	CloseWrite() error
}

// tailcat_conn_close_write shuts down the writing half of the connection,
// sending a TCP FIN so the peer sees EOF on its next read, while this side
// can still read any reply. Protocols where one side signals
// end-of-request and then reads a response (as the tailcat CLI's stdin/
// stdout piping does) need this instead of a full tailcat_conn_close,
// since the underlying TCP stack runs entirely inside this process: a full
// close on a connection with unflushed writes can lose the FIN before it
// reaches the peer. Returns 0 on success, -1 on error (including if the
// connection type doesn't support half-close).
//
//export tailcat_conn_close_write
func tailcat_conn_close_write(handle C.longlong) C.int {
	h := Handle(handle)
	c, ok := getConn(h)
	if !ok {
		setLastError(h, errors.New("invalid conn handle"))
		return -1
	}
	cw, ok := c.(closeWriter)
	if !ok {
		setLastError(h, errors.New("connection does not support half-close"))
		return -1
	}
	if err := cw.CloseWrite(); err != nil {
		setLastError(h, err)
		return -1
	}
	return 0
}

// tailcat_conn_close closes the connection. Returns 0 on success
// (idempotent), -1 on error.
//
//export tailcat_conn_close
func tailcat_conn_close(handle C.longlong) C.int {
	h := Handle(handle)
	c, ok := getConn(h)
	if !ok {
		setLastError(h, errors.New("invalid conn handle"))
		return -1
	}
	err := c.Close()
	reg.delete(h)
	if err != nil {
		setLastError(h, err)
		return -1
	}
	return 0
}

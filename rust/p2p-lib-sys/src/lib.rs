//! Raw, unsafe FFI bindings to the `tailcat_cgo` C ABI shared library
//! (built from the Go module in `tailcat-cgo/` by this crate's `build.rs`).
//!
//! These declarations mirror `tailcat-cgo/tailcat_cgo.h` exactly. Prefer
//! the safe wrappers in the `p2p-lib` crate over calling these directly.

use std::os::raw::{c_char, c_int, c_longlong, c_uchar};

extern "C" {
    pub fn tailcat_free_string(s: *mut c_char);
    pub fn tailcat_last_error(handle: c_longlong) -> *mut c_char;

    pub fn tailcat_privatekey_generate() -> *mut c_char;
    pub fn tailcat_privatekey_public_key(private_key_json: *const c_char) -> *mut c_char;

    pub fn tailcat_connblob_resolve(conn_blob: *const c_char) -> *mut c_char;

    pub fn tailcat_server_new() -> c_longlong;
    pub fn tailcat_server_new_with_key(private_key_json: *const c_char) -> c_longlong;
    pub fn tailcat_server_set_allowed_client(handle: c_longlong, pub_key_text: *const c_char) -> c_int;
    pub fn tailcat_server_start(handle: c_longlong) -> c_int;
    pub fn tailcat_server_accept(handle: c_longlong, timeout_ms: c_longlong) -> c_longlong;
    pub fn tailcat_server_connblob(handle: c_longlong) -> *mut c_char;
    pub fn tailcat_server_state(handle: c_longlong) -> *mut c_char;
    pub fn tailcat_server_close(handle: c_longlong) -> c_int;

    pub fn tailcat_client_new(conn_blob: *const c_char) -> c_longlong;
    pub fn tailcat_client_new_with_key(conn_blob: *const c_char, private_key_json: *const c_char) -> c_longlong;
    pub fn tailcat_client_public_key(handle: c_longlong) -> *mut c_char;
    pub fn tailcat_client_ping(handle: c_longlong, timeout_ms: c_longlong) -> c_longlong;
    pub fn tailcat_client_dial_tcp_port(handle: c_longlong, port: c_int, timeout_ms: c_longlong) -> c_longlong;
    pub fn tailcat_client_close(handle: c_longlong) -> c_int;

    pub fn tailcat_conn_read(handle: c_longlong, buf: *mut c_uchar, length: c_int) -> c_int;
    pub fn tailcat_conn_write(handle: c_longlong, buf: *const c_uchar, length: c_int) -> c_int;
    pub fn tailcat_conn_close_write(handle: c_longlong) -> c_int;
    pub fn tailcat_conn_close(handle: c_longlong) -> c_int;
}

use std::os::raw::c_longlong;
use std::time::Duration;

use crate::conn::Conn;
use crate::error::{take_error, Result};
use crate::ffi::{take_string_or_err, to_cstring};
use crate::keys::PrivateKey;

/// Connects to a [`crate::Server`] over a WireGuard tunnel relayed through
/// DERP, given the connection token the server printed.
///
/// The tunnel is established lazily on first use ([`Client::ping`] or
/// [`Client::dial_tcp_port`]).
pub struct Client {
    handle: c_longlong,
}

impl Client {
    /// Creates a client that will connect to the server identified by
    /// `conn_blob`, using a fresh ephemeral identity.
    pub fn new(conn_blob: &str) -> Result<Self> {
        let c = to_cstring(conn_blob)?;
        let handle = unsafe { p2p_lib_sys::tailcat_client_new(c.as_ptr()) };
        if handle == 0 {
            return Err(unsafe { take_error(0) });
        }
        Ok(Self { handle })
    }

    /// Like [`Client::new`], but uses a previously saved identity key, so a
    /// server can allow-list this client ahead of time (see
    /// [`crate::Server::allow_client`]).
    pub fn with_key(conn_blob: &str, key: &PrivateKey) -> Result<Self> {
        let blob_c = to_cstring(conn_blob)?;
        let key_c = to_cstring(key.as_str())?;
        let handle =
            unsafe { p2p_lib_sys::tailcat_client_new_with_key(blob_c.as_ptr(), key_c.as_ptr()) };
        if handle == 0 {
            return Err(unsafe { take_error(0) });
        }
        Ok(Self { handle })
    }

    /// This client's node public key (text form), generating it on first
    /// call if needed. Useful to show your identity to a user, or to give
    /// to a server operator for allow-listing.
    pub fn public_key(&self) -> Result<String> {
        let ptr = unsafe { p2p_lib_sys::tailcat_client_public_key(self.handle) };
        unsafe { take_string_or_err(ptr, self.handle) }
    }

    /// Starts the client if needed, performs the handshake with the
    /// server, and returns the round-trip latency. Calling it explicitly
    /// is optional -- [`Client::dial_tcp_port`] does it implicitly -- but
    /// useful to test connectivity or measure latency first.
    pub fn ping(&self, timeout: Duration) -> Result<Duration> {
        let ms = timeout.as_millis().min(i64::MAX as u128) as i64;
        let latency_ms = unsafe { p2p_lib_sys::tailcat_client_ping(self.handle, ms) };
        if latency_ms < 0 {
            return Err(unsafe { take_error(self.handle) });
        }
        Ok(Duration::from_millis(latency_ms as u64))
    }

    /// Opens a TCP connection to the given port on the server, starting
    /// the client first if needed.
    pub fn dial_tcp_port(&self, port: u16, timeout: Duration) -> Result<Conn> {
        let ms = timeout.as_millis().min(i64::MAX as u128) as i64;
        let conn_handle = unsafe {
            p2p_lib_sys::tailcat_client_dial_tcp_port(self.handle, port as i32, ms)
        };
        if conn_handle == 0 {
            return Err(unsafe { take_error(self.handle) });
        }
        Ok(Conn::from_handle(conn_handle))
    }

    /// Shuts the client down.
    pub fn close(mut self) -> Result<()> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> Result<()> {
        if self.handle == 0 {
            return Ok(());
        }
        let handle = self.handle;
        self.handle = 0;
        let rc = unsafe { p2p_lib_sys::tailcat_client_close(handle) };
        if rc != 0 {
            return Err(unsafe { take_error(handle) });
        }
        Ok(())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

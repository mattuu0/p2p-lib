use std::os::raw::c_longlong;
use std::time::Duration;

use crate::conn::Conn;
use crate::error::{take_error, Result};
use crate::ffi::{take_string_or_err, to_cstring};
use crate::keys::PrivateKey;

/// Listens for tailcat clients over a WireGuard tunnel relayed through
/// DERP, with no control plane or Tailscale account required.
///
/// Construct with [`Server::new`] (fresh ephemeral identity) or
/// [`Server::with_key`] (a previously persisted [`PrivateKey`]), then call
/// [`Server::start`], share the [`Server::conn_blob`] token with clients
/// out of band, and call [`Server::accept`] in a loop.
pub struct Server {
    handle: c_longlong,
}

impl Server {
    /// Creates a server with a fresh ephemeral key; the token it prints
    /// after [`Server::start`] is unique to this run and unusable again
    /// once the process exits.
    pub fn new() -> Result<Self> {
        let handle = unsafe { p2p_lib_sys::tailcat_server_new() };
        if handle == 0 {
            return Err(unsafe { take_error(0) });
        }
        Ok(Self { handle })
    }

    /// Creates a server using a previously saved identity, so its address
    /// token stays stable across restarts.
    pub fn with_key(key: &PrivateKey) -> Result<Self> {
        let json = to_cstring(key.as_str())?;
        let handle = unsafe { p2p_lib_sys::tailcat_server_new_with_key(json.as_ptr()) };
        if handle == 0 {
            return Err(unsafe { take_error(0) });
        }
        Ok(Self { handle })
    }

    /// Restricts the server to only accept the given client's public key
    /// (as returned by [`PrivateKey::public_key`] or
    /// [`crate::Client::public_key`]). Must be called before [`Server::start`];
    /// call repeatedly to allow more than one client. If never called, all
    /// clients are allowed.
    pub fn allow_client(&mut self, public_key: &str) -> Result<()> {
        let c = to_cstring(public_key)?;
        let rc = unsafe { p2p_lib_sys::tailcat_server_set_allowed_client(self.handle, c.as_ptr()) };
        if rc != 0 {
            return Err(unsafe { take_error(self.handle) });
        }
        Ok(())
    }

    /// Connects to the DERP relay and begins accepting clients.
    pub fn start(&mut self) -> Result<()> {
        let rc = unsafe { p2p_lib_sys::tailcat_server_start(self.handle) };
        if rc != 0 {
            return Err(unsafe { take_error(self.handle) });
        }
        Ok(())
    }

    /// The token clients pass to [`crate::Client::new`] to connect to this
    /// server. Must be called after [`Server::start`].
    pub fn conn_blob(&self) -> Result<String> {
        let ptr = unsafe { p2p_lib_sys::tailcat_server_connblob(self.handle) };
        unsafe { take_string_or_err(ptr, self.handle) }
    }

    /// Blocks until a client connects (to any port) or `timeout` elapses,
    /// returning `Ok(None)` on a plain timeout.
    pub fn accept(&self, timeout: Duration) -> Result<Option<Conn>> {
        let ms = timeout.as_millis().min(i64::MAX as u128) as i64;
        let conn_handle = unsafe { p2p_lib_sys::tailcat_server_accept(self.handle, ms) };
        if conn_handle == 0 {
            // Distinguish a timeout (no error recorded) from a real error.
            match unsafe { crate::error::last_error(self.handle) } {
                Some(msg) => return Err(crate::error::Error::Tailcat(msg)),
                None => return Ok(None),
            }
        }
        Ok(Some(Conn::from_handle(conn_handle)))
    }

    /// Returns this server's identity and allow-list bundled as one JSON
    /// string, so a caller can persist everything needed to resume this
    /// server later (see [`PrivateKey`] for the storage-agnostic model
    /// this crate follows). The JSON shape is an implementation detail
    /// shared with [`PrivateKey::to_json`]'s `privateKey` field plus an
    /// `allowedClients` array; treat it as opaque unless you need to
    /// inspect it.
    pub fn state_json(&self) -> Result<String> {
        let ptr = unsafe { p2p_lib_sys::tailcat_server_state(self.handle) };
        unsafe { take_string_or_err(ptr, self.handle) }
    }

    /// Shuts the server down, draining in-flight TCP data first so a
    /// caller that exits its process right after this call doesn't lose
    /// data still in flight to a peer (see the crate-level docs on
    /// graceful shutdown). Prefer this over letting [`Server`] drop when
    /// you're about to exit the process.
    pub fn close(mut self) -> Result<()> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> Result<()> {
        if self.handle == 0 {
            return Ok(());
        }
        let handle = self.handle;
        self.handle = 0;
        let rc = unsafe { p2p_lib_sys::tailcat_server_close(handle) };
        if rc != 0 {
            return Err(unsafe { take_error(handle) });
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

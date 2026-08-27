use std::io;
use std::os::raw::c_longlong;

/// A TCP-like connection tunneled over WireGuard via a tailcat [`Server`]
/// (accepted) or [`Client`] (dialed).
///
/// [`Server`]: crate::Server
/// [`Client`]: crate::Client
pub struct Conn {
    pub(crate) handle: c_longlong,
}

impl Conn {
    pub(crate) fn from_handle(handle: c_longlong) -> Self {
        Self { handle }
    }

    /// Shuts down the writing half of the connection (sends a TCP FIN),
    /// while still allowing reads for any reply from the peer. Use this
    /// instead of [`Conn::close`] when you've finished writing but still
    /// want to read a response -- e.g. a request/response protocol, or
    /// piping stdin then waiting for the peer's output. Without this, a
    /// full close risks losing unflushed writes: the whole TCP stack runs
    /// inside this process, so there's no OS-level buffer to flush a
    /// pending FIN after the connection object is gone.
    pub fn close_write(&self) -> crate::Result<()> {
        let rc = unsafe { p2p_lib_sys::tailcat_conn_close_write(self.handle) };
        if rc != 0 {
            return Err(unsafe { crate::error::take_error(self.handle) });
        }
        Ok(())
    }

    /// Closes the connection now, returning any error from the underlying
    /// close. Also happens automatically on drop (errors from an implicit
    /// close on drop are silently ignored, matching `std::net::TcpStream`).
    pub fn close(mut self) -> crate::Result<()> {
        self.close_inner()
    }

    /// Splits the connection into an independent reader and writer, e.g.
    /// so one thread can block reading incoming data while another writes
    /// outgoing data (as a simple chat example does). The underlying
    /// connection is shared -- Go's `net.Conn` supports concurrent
    /// Read/Write from different goroutines -- and is closed once when
    /// either half drops or is explicitly closed; the other half then
    /// starts reporting `InvalidHandle` errors, matching a normal closed
    /// socket.
    pub fn split(self) -> (ConnReader, ConnWriter) {
        let handle = self.handle;
        std::mem::forget(self); // ownership of `handle` moves to the two halves below
        let shared = std::sync::Arc::new(SharedHandle { handle });
        (
            ConnReader {
                shared: shared.clone(),
            },
            ConnWriter { shared },
        )
    }

    fn close_inner(&mut self) -> crate::Result<()> {
        if self.handle == 0 {
            return Ok(());
        }
        let handle = self.handle;
        self.handle = 0;
        let rc = unsafe { p2p_lib_sys::tailcat_conn_close(handle) };
        if rc != 0 {
            return Err(unsafe { crate::error::take_error(handle) });
        }
        Ok(())
    }
}

impl io::Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = unsafe {
            p2p_lib_sys::tailcat_conn_read(
                self.handle,
                buf.as_mut_ptr(),
                buf.len().min(i32::MAX as usize) as i32,
            )
        };
        if n == -2 {
            return Ok(0); // clean EOF from the peer, per the tailcat_conn_read contract
        }
        if n < 0 {
            let msg = unsafe { crate::error::last_error(self.handle) }
                .unwrap_or_else(|| "read failed".to_string());
            return Err(io::Error::other(msg));
        }
        Ok(n as usize)
    }
}

impl io::Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = unsafe {
            p2p_lib_sys::tailcat_conn_write(
                self.handle,
                buf.as_ptr(),
                buf.len().min(i32::MAX as usize) as i32,
            )
        };
        if n < 0 {
            let msg = unsafe { crate::error::last_error(self.handle) }
                .unwrap_or_else(|| "write failed".to_string());
            return Err(io::Error::other(msg));
        }
        Ok(n as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(()) // the underlying gVisor TCP conn has no separate flush step
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

/// The handle shared by a [`ConnReader`]/[`ConnWriter`] pair produced by
/// [`Conn::split`]. Closes the underlying connection once, when the last
/// half is dropped.
struct SharedHandle {
    handle: c_longlong,
}

impl Drop for SharedHandle {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                p2p_lib_sys::tailcat_conn_close(self.handle);
            }
        }
    }
}

/// The read half of a [`Conn`] split via [`Conn::split`].
pub struct ConnReader {
    shared: std::sync::Arc<SharedHandle>,
}

impl io::Read for ConnReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let handle = self.shared.handle;
        let n = unsafe {
            p2p_lib_sys::tailcat_conn_read(handle, buf.as_mut_ptr(), buf.len().min(i32::MAX as usize) as i32)
        };
        if n == -2 {
            return Ok(0);
        }
        if n < 0 {
            let msg =
                unsafe { crate::error::last_error(handle) }.unwrap_or_else(|| "read failed".to_string());
            return Err(io::Error::other(msg));
        }
        Ok(n as usize)
    }
}

/// The write half of a [`Conn`] split via [`Conn::split`].
pub struct ConnWriter {
    shared: std::sync::Arc<SharedHandle>,
}

impl ConnWriter {
    /// Shuts down the writing half only (see [`Conn::close_write`]).
    pub fn close_write(&self) -> crate::Result<()> {
        let handle = self.shared.handle;
        let rc = unsafe { p2p_lib_sys::tailcat_conn_close_write(handle) };
        if rc != 0 {
            return Err(unsafe { crate::error::take_error(handle) });
        }
        Ok(())
    }
}

impl io::Write for ConnWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let handle = self.shared.handle;
        let n = unsafe {
            p2p_lib_sys::tailcat_conn_write(handle, buf.as_ptr(), buf.len().min(i32::MAX as usize) as i32)
        };
        if n < 0 {
            let msg =
                unsafe { crate::error::last_error(handle) }.unwrap_or_else(|| "write failed".to_string());
            return Err(io::Error::other(msg));
        }
        Ok(n as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

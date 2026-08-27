use std::ffi::CStr;
use std::os::raw::c_longlong;

/// Errors returned by this crate's Server/Client/Conn operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Tailcat(String),
    #[error("operation timed out")]
    Timeout,
    #[error("invalid handle (already closed?)")]
    InvalidHandle,
    #[error("invalid UTF-8 or NUL byte in string: {0}")]
    InvalidString(#[from] std::ffi::NulError),
    #[error("failed to (de)serialize JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Reads and frees the last error message recorded for `handle` (or the
/// global slot, handle 0, for calls made before a handle exists).
///
/// # Safety
/// Must only be called right after a p2p-lib-sys call that may have set an
/// error for this handle; the underlying store isn't a snapshot, so a
/// concurrent call on the same handle from another thread could race.
pub(crate) unsafe fn last_error(handle: c_longlong) -> Option<String> {
    let ptr = p2p_lib_sys::tailcat_last_error(handle);
    if ptr.is_null() {
        return None;
    }
    let msg = CStr::from_ptr(ptr).to_string_lossy().into_owned();
    p2p_lib_sys::tailcat_free_string(ptr);
    Some(msg)
}

pub(crate) unsafe fn take_error(handle: c_longlong) -> Error {
    match last_error(handle) {
        Some(msg) => Error::Tailcat(msg),
        None => Error::InvalidHandle,
    }
}

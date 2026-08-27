use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use crate::error::Result;

/// Converts a Rust `&str` to a NUL-terminated C string for a single FFI
/// call. The caller passes `.as_ptr()` into the extern call and drops this
/// value only after that call returns (CString's Drop frees the buffer).
pub(crate) fn to_cstring(s: &str) -> Result<CString> {
    Ok(CString::new(s)?)
}

/// Takes ownership of a `*mut c_char` returned by the Go library, copies it
/// into a Rust `String`, and frees the original via
/// `tailcat_free_string`. Returns `None` for a NULL pointer.
///
/// # Safety
/// `ptr` must be either NULL or a pointer previously returned by one of
/// the `tailcat_*` functions that documents returning an owned string.
pub(crate) unsafe fn take_string(ptr: *mut c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
    p2p_lib_sys::tailcat_free_string(ptr);
    Some(s)
}

/// Like `take_string`, but maps a NULL pointer to the given handle's last
/// recorded error instead of `None`, for functions where NULL always means
/// failure.
pub(crate) unsafe fn take_string_or_err(
    ptr: *mut c_char,
    err_handle: std::os::raw::c_longlong,
) -> Result<String> {
    match take_string(ptr) {
        Some(s) => Ok(s),
        None => Err(crate::error::take_error(err_handle)),
    }
}

use crate::error::Result;
use crate::ffi::{take_string_or_err, to_cstring};

/// A tailcat node identity (private key + associated public connection
/// info), carried as an opaque JSON string.
///
/// This crate never touches the filesystem or any platform secure storage
/// on your behalf: call [`PrivateKey::to_json`] to get a string you can
/// persist however fits your platform (a file, OS keychain, encrypted
/// prefs, ...), and [`PrivateKey::from_json`] to restore it later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateKey(String);

impl PrivateKey {
    /// Generates a new, random node identity.
    pub fn generate() -> Result<Self> {
        unsafe {
            let ptr = p2p_lib_sys::tailcat_privatekey_generate();
            let json = take_string_or_err(ptr, 0)?;
            Ok(Self(json))
        }
    }

    /// Wraps a previously saved JSON string (from [`PrivateKey::to_json`])
    /// without validating it; validation happens on first use.
    pub fn from_json(json: impl Into<String>) -> Self {
        Self(json.into())
    }

    /// Returns the JSON representation to persist.
    pub fn to_json(&self) -> &str {
        &self.0
    }

    /// Returns this identity's public key in its text form (e.g.
    /// `"nodekey:..."`), for building an allow-list on a server.
    pub fn public_key(&self) -> Result<String> {
        unsafe {
            let json_c = to_cstring(&self.0)?;
            let ptr = p2p_lib_sys::tailcat_privatekey_public_key(json_c.as_ptr());
            take_string_or_err(ptr, 0)
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Resolves a [`ConnBlob`]-style token into a self-contained form with the
/// DERP relay's details embedded, so it can be persisted and reconnected
/// to later without an extra network fetch to resolve the region.
pub fn resolve_conn_blob(conn_blob: &str) -> Result<String> {
    unsafe {
        let c = to_cstring(conn_blob)?;
        let ptr = p2p_lib_sys::tailcat_connblob_resolve(c.as_ptr());
        take_string_or_err(ptr, 0)
    }
}

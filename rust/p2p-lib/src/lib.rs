//! Safe Rust API for peer-to-peer networking over
//! [tailcat](https://github.com/tailscale/tailcat): WireGuard encryption
//! and DERP-relayed NAT traversal with no control plane, no Tailscale
//! account, and no root/admin privileges required.
//!
//! One side runs a [`Server`], which prints a short connection token. The
//! other side gives that token to [`Client`]. Traffic between them is
//! end-to-end WireGuard-encrypted, bootstrapped through a DERP relay and
//! upgraded to a direct UDP path when NAT traversal succeeds.
//!
//! ```no_run
//! use std::time::Duration;
//! use std::io::{Read, Write};
//!
//! // Server side:
//! let mut server = p2p_lib::Server::new()?;
//! server.start()?;
//! println!("share this token: {}", server.conn_blob()?);
//! let mut conn = server.accept(Duration::from_secs(60))?.expect("no client connected in time");
//! conn.write_all(b"hello from server\n")?;
//!
//! # Ok::<(), p2p_lib::Error>(())
//! ```
//!
//! ```no_run
//! use std::time::Duration;
//! use std::io::Read;
//!
//! // Client side, given the token printed above:
//! let client = p2p_lib::Client::new("tc...")?;
//! let mut conn = client.dial_tcp_port(0, Duration::from_secs(10))?;
//! let mut buf = String::new();
//! conn.read_to_string(&mut buf)?;
//! println!("{buf}");
//! # Ok::<(), p2p_lib::Error>(())
//! ```
//!
//! # Persistence
//!
//! This crate never performs file I/O. [`PrivateKey::to_json`] and
//! [`Server::state_json`] hand back plain JSON strings; storing them (in a
//! file, an OS keychain, encrypted app storage, ...) is left to the
//! caller, since the right mechanism varies by platform.

mod client;
mod conn;
mod error;
mod ffi;
mod keys;
mod peer_path;
mod server;

pub use client::Client;
pub use conn::{Conn, ConnReader, ConnWriter};
pub use error::{Error, Result};
pub use keys::{resolve_conn_blob, PrivateKey};
pub use peer_path::PeerPath;
pub use server::Server;

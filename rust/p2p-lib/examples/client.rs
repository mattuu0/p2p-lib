//! Minimal client: connects to the token printed by `cargo run --example
//! server`, writes a message, and half-closes so the server sees EOF.
//!
//! Run with `cargo run --example client -- <token>`.

use std::env;
use std::io::Write;
use std::time::Duration;

fn main() -> Result<(), p2p_lib::Error> {
    let token = env::args()
        .nth(1)
        .expect("usage: client <connection token>");

    let client = p2p_lib::Client::new(&token)?;
    let latency = client.ping(Duration::from_secs(10))?;
    println!("connected, ping latency: {latency:?}");

    let mut conn = client.dial_tcp_port(0, Duration::from_secs(10))?;
    conn.write_all(b"hello from the rust client\n")
        .map_err(|e| p2p_lib::Error::Tailcat(e.to_string()))?;
    // Half-close: signal EOF to the server so its read unblocks, without
    // tearing down the connection before the write is actually flushed.
    conn.close_write()?;
    conn.close()?;

    client.close()?;
    Ok(())
}

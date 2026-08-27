//! Minimal echo-style server: prints its connection token, accepts one
//! client, and copies whatever it receives back to stdout.
//!
//! Run with `cargo run --example server`, then feed the printed token to
//! `cargo run --example client -- <token>` in another terminal.

use std::io::Read;
use std::time::Duration;

fn main() -> Result<(), p2p_lib::Error> {
    let mut server = p2p_lib::Server::new()?;
    server.start()?;
    println!("listening; token: {}", server.conn_blob()?);

    let mut conn = loop {
        match server.accept(Duration::from_secs(120))? {
            Some(conn) => break conn,
            None => println!("still waiting for a client..."),
        }
    };

    println!("client connected, reading until EOF:");
    let mut buf = String::new();
    conn.read_to_string(&mut buf)
        .map_err(|e| p2p_lib::Error::Tailcat(e.to_string()))?;
    println!("{buf}");

    conn.close()?;
    server.close()?;
    Ok(())
}

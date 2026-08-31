//! Spike (server half): host once, accept TWO independent dials against the
//! SAME printed conn_blob while this process stays alive, to see whether a
//! second `Client::new(blob)` in another process can reuse it -- this
//! directly informs the remote-assist agent's "reconnect after a drop using
//! the same URL" design.
//!
//! Run: `cargo run --example replay_spike_server`, then run
//! `replay_spike_client` twice (sequentially) against the printed token.

use std::io::Read;
use std::time::Duration;

fn main() -> Result<(), p2p_lib::Error> {
    let mut server = p2p_lib::Server::new()?;
    server.start()?;
    println!("conn_blob: {}", server.conn_blob()?);

    for i in 1..=2 {
        println!("\n=== waiting for dial #{i} ===");
        let mut conn = loop {
            match server.accept(Duration::from_secs(60))? {
                Some(conn) => break conn,
                None => println!("still waiting..."),
            }
        };
        let mut buf = String::new();
        conn.read_to_string(&mut buf)
            .map_err(|e| p2p_lib::Error::Tailcat(e.to_string()))?;
        println!("dial #{i} received: {buf:?}");
        conn.close()?;
    }

    server.close()?;
    println!("\nboth dials against the same conn_blob succeeded.");
    Ok(())
}

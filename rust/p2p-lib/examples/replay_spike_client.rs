//! Spike (client half): dial a conn_blob with a brand new `Client::new()`
//! identity and write a message. Run this twice against the same printed
//! token (from `replay_spike_server`) to test reuse.
//!
//! Run: `cargo run --example replay_spike_client -- <token> <label>`.

use std::env;
use std::io::Write;
use std::time::Duration;

fn main() -> Result<(), p2p_lib::Error> {
    let mut args = env::args().skip(1);
    let token = args.next().expect("usage: replay_spike_client <token> <label>");
    let label = args.next().unwrap_or_else(|| "unlabeled".to_string());

    let client = p2p_lib::Client::new(&token)?;
    let latency = client.ping(Duration::from_secs(10))?;
    println!("[{label}] connected, ping latency: {latency:?}");

    let mut conn = client.dial_tcp_port(0, Duration::from_secs(10))?;
    conn.write_all(format!("hello from {label}\n").as_bytes())
        .map_err(|e| p2p_lib::Error::Tailcat(e.to_string()))?;
    conn.close_write()?;
    conn.close()?;
    client.close()?;

    println!("[{label}] dial succeeded");
    Ok(())
}

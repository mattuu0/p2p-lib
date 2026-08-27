//! A minimal CLI chat over tailcat's WireGuard/DERP tunnel.
//!
//! Run without arguments to host (prints a connection token):
//!
//!     cargo run --example chat
//!
//! Run with that token as an argument, in another terminal (or on another
//! machine), to join:
//!
//!     cargo run --example chat -- <token>
//!
//! Type a line and press Enter to send it; the peer's messages print as
//! they arrive on a separate thread. Ctrl-D (EOF on stdin) ends your
//! session and half-closes the connection so the peer sees you've left.

use std::io::{self, BufRead, Write};
use std::thread;
use std::time::Duration;

use p2p_lib::{Client, Conn, Server};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Which role this process is playing, kept around so it can be closed
/// (gracefully, draining in-flight data -- see the crate docs) once the
/// chat loop ends, instead of just dropping.
enum Endpoint {
    Host(Server),
    Peer(Client),
}

impl Endpoint {
    fn close(self) -> Result<(), p2p_lib::Error> {
        match self {
            Endpoint::Host(server) => server.close(),
            Endpoint::Peer(client) => client.close(),
        }
    }
}

fn run() -> Result<(), p2p_lib::Error> {
    let token_arg = std::env::args().nth(1);
    let (endpoint, conn) = match token_arg {
        Some(token) => join(&token)?,
        None => host()?,
    };
    let result = chat_loop(conn);
    endpoint.close()?;
    result
}

fn host() -> Result<(Endpoint, Conn), p2p_lib::Error> {
    let mut server = Server::new()?;
    server.start()?;
    println!("=== hosting a chat ===");
    println!("share this token with your peer:\n{}\n", server.conn_blob()?);
    println!("waiting for someone to join...");

    let conn = loop {
        match server.accept(Duration::from_secs(300))? {
            Some(conn) => break conn,
            None => println!("still waiting..."),
        }
    };
    println!("peer connected! start typing (Ctrl-D to quit).\n");
    Ok((Endpoint::Host(server), conn))
}

fn join(token: &str) -> Result<(Endpoint, Conn), p2p_lib::Error> {
    let client = Client::new(token)?;
    println!("=== joining chat ===");
    let latency = client.ping(Duration::from_secs(10))?;
    println!("connected (ping {latency:?}). start typing (Ctrl-D to quit).\n");

    let conn = client.dial_tcp_port(0, Duration::from_secs(10))?;
    Ok((Endpoint::Peer(client), conn))
}

fn chat_loop(conn: Conn) -> Result<(), p2p_lib::Error> {
    let (reader, mut writer) = conn.split();

    // Print whatever the peer sends, on its own thread, so it can arrive
    // at any time without blocking on our own stdin read.
    let recv_thread = thread::spawn(move || {
        let mut reader = io::BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    println!("\n[peer disconnected]");
                    break;
                }
                Ok(_) => {
                    print!("peer> {line}");
                    let _ = io::stdout().flush();
                }
                Err(e) => {
                    println!("\n[read error: {e}]");
                    break;
                }
            }
        }
    });

    let stdin = io::stdin();
    let mut input = String::new();
    loop {
        input.clear();
        print!("you> ");
        io::stdout().flush().ok();
        let n = stdin.lock().read_line(&mut input).unwrap_or(0);
        if n == 0 {
            break; // Ctrl-D
        }
        if writer.write_all(input.as_bytes()).is_err() {
            println!("[connection lost]");
            break;
        }
    }

    // Tell the peer we're done sending, but let the receive thread keep
    // reading until it sees EOF/disconnect too.
    let _ = writer.close_write();
    let _ = recv_thread.join();
    Ok(())
}

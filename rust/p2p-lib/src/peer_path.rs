/// Whether the (single, 1:1) connected peer is reachable over a direct UDP
/// path (best latency/throughput), or is still relayed through a DERP
/// server (works everywhere, but adds latency and is capped by the relay's
/// bandwidth). See [`Server::peer_path`] and [`Client::peer_path`].
///
/// [`Server::peer_path`]: crate::Server::peer_path
/// [`Client::peer_path`]: crate::Client::peer_path
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerPath {
    /// A direct UDP path to the peer has been established (NAT traversal
    /// succeeded).
    Direct,
    /// Traffic is currently relayed through a DERP server -- NAT traversal
    /// hasn't (yet, or won't) succeed for this pair.
    Relay,
    /// No path information is available yet (e.g. called immediately after
    /// `start()`/`dial_tcp_port()`, before the peer has been seen, or on a
    /// client that hasn't been started).
    Unknown,
}

impl PeerPath {
    pub(crate) fn from_c_str(s: &str) -> Self {
        match s {
            "direct" => PeerPath::Direct,
            "relay" => PeerPath::Relay,
            _ => PeerPath::Unknown,
        }
    }
}

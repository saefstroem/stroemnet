#![cfg_attr(target_arch = "wasm32", allow(clippy::arc_with_non_send_sync))]

pub mod addr;
pub mod error;
pub mod gossip;
pub mod identity;
pub mod network;
pub mod peer;
pub mod transport;
pub mod wire;

pub use addr::{listen_addrs_equal, normalize_listen_addr};
pub use error::StroemnetP2pError;
pub use identity::proposal_digest;
pub use network::{P2p, P2pConfig};
pub use transport::WsTransport;
pub use wire::P2pMsg;

pub type Result<T> = std::result::Result<T, StroemnetP2pError>;

pub const SEED_NODES: &[&str] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_nodes_default_is_empty() {
        assert!(SEED_NODES.is_empty());
    }
}

#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::mpsc::UnboundedSender;

use crate::identity::Identity;

#[derive(Debug, Clone)]
/// Configuration for the p2p instance
pub struct P2pConfig {
    /// Node identity on the p2p net
    pub identity: Identity,
    /// Which peers you will connect to
    pub bootstrap_peers: Vec<String>,
    /// Maximum outbound peers
    pub target_outbound: usize,
    /// maximum inbound
    pub max_inbound: usize,
    /// How other peers can reach you
    pub advertised_listen_addr: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    /// Where to send discovered peers from other peers
    pub discovered_peer_dial_tx: Option<UnboundedSender<String>>,
}

impl P2pConfig {
    pub fn node_id(&self) -> [u8; 32] {
        self.identity.id
    }
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            identity: Identity::generate(),
            bootstrap_peers: Vec::new(),
            target_outbound: 8,
            max_inbound: 125,
            advertised_listen_addr: None,
            #[cfg(not(target_arch = "wasm32"))]
            discovered_peer_dial_tx: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_sane_limits_and_node_id() {
        let cfg = P2pConfig::default();
        assert_eq!(cfg.target_outbound, 8);
        assert_eq!(cfg.max_inbound, 125);
        assert!(cfg.bootstrap_peers.is_empty());
        assert_eq!(cfg.node_id(), cfg.identity.id);
    }
}

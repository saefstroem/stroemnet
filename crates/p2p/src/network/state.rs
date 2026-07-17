use std::collections::HashSet;
use std::sync::Arc;

use futures::channel::mpsc;
use futures::lock::Mutex;

use super::config::P2pConfig;
use crate::gossip::SeenSet;
use crate::peer::ConnectedPeer;
use crate::wire::message::{NodeState, P2pMsg, PeerAddr};

pub struct P2p {
    pub config: P2pConfig,
    pub connected_peers: Arc<Mutex<Vec<ConnectedPeer>>>,
    pub seen: Arc<Mutex<SeenSet>>,
    pub events_tx: mpsc::Sender<NetEvent>,
    pub blacklist: Arc<Mutex<HashSet<[u8; 32]>>>,
}

#[derive(Debug)]
pub struct NetEvent {
    pub from: String,
    pub msg: P2pMsg,
}

impl P2p {
    pub fn new(config: P2pConfig) -> (Self, mpsc::Receiver<NetEvent>) {
        let (tx, rx) = mpsc::channel(256);
        let net = Self {
            config,
            connected_peers: Arc::new(Mutex::new(Vec::new())),
            seen: Arc::new(Mutex::new(SeenSet::new(50_000))),
            events_tx: tx,
            blacklist: Arc::new(Mutex::new(HashSet::new())),
        };
        (net, rx)
    }

    pub async fn blacklist_peer(&self, node_id: [u8; 32]) {
        self.blacklist.lock().await.insert(node_id);
    }

    pub async fn is_blacklisted(&self, node_id: &[u8; 32]) -> bool {
        self.blacklist.lock().await.contains(node_id)
    }

    pub async fn add_connected_peer(&self, peer: ConnectedPeer) -> usize {
        let mut peers = self.connected_peers.lock().await;
        peers.push(peer);
        peers.len() - 1
    }

    pub async fn remove_connected_peer(&self, url: &str) {
        let mut peers = self.connected_peers.lock().await;
        peers.retain(|p| p.url != url);
    }

    pub async fn connected_peer_addrs(&self) -> Vec<PeerAddr> {
        let our_listen = self.config.advertised_listen_addr.as_deref();
        self.connected_peers
            .lock()
            .await
            .iter()
            .filter_map(|p| {
                let url = p.advertised_listen.clone()?;
                if let Some(ours) = our_listen
                    && crate::listen_addrs_equal(&url, ours)
                {
                    return None;
                }
                Some(PeerAddr {
                    url,
                    last_seen: p.connected_at,
                })
            })
            .collect()
    }

    pub async fn is_connected_peer(&self, listen_url_norm: &str) -> bool {
        self.connected_peers.lock().await.iter().any(|p| {
            p.advertised_listen
                .as_deref()
                .map(|u| crate::listen_addrs_equal(u, listen_url_norm))
                .unwrap_or(false)
        })
    }

    pub async fn current_state(&self) -> NodeState {
        NodeState {
            node_id: self.config.node_id(),
            listen_addr: self.config.advertised_listen_addr.clone(),
            peers: self.connected_peer_addrs().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_starts_empty_and_blacklist_tracks_ids() {
        let (net, _rx) = P2p::new(P2pConfig::default());
        assert!(net.connected_peer_addrs().await.is_empty());
        assert!(!net.is_blacklisted(&[1u8; 32]).await);
        net.blacklist_peer([1u8; 32]).await;
        assert!(net.is_blacklisted(&[1u8; 32]).await);
    }
}

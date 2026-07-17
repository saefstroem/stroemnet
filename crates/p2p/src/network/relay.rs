use std::sync::Arc;

use super::state::P2p;
use crate::Result;
use crate::error::StroemnetP2pError;
use crate::gossip::SeenSet;
use crate::peer::ConnectedPeer;
use crate::wire::encode;
use crate::wire::message::P2pMsg;

const SEND_TIMEOUT_SECS: u64 = 5;

/// Races a byte transmission to a connected peer with a timeout
async fn send_timed(peer: &ConnectedPeer, bytes: Vec<u8>) -> Result<()> {
    use futures::future::{Either, select};
    let send = std::pin::pin!(peer.send_bytes(bytes));
    let timer = std::pin::pin!(stroemnet_protocol::sleep_secs(SEND_TIMEOUT_SECS));
    match select(send, timer).await {
        Either::Left((r, _)) => r,
        Either::Right(_) => Err(StroemnetP2pError::Io("ws send timed out".into())),
    }
}

impl P2p {
    /// Spawns a periodic task that will emit the nodes current state to all its peers
    /// A bit noisy but its good enough for an initial implementation
    pub fn spawn_periodic_state_broadcast(self: Arc<Self>, interval_secs: u64) {
        let fut = async move {
            loop {
                stroemnet_protocol::sleep_secs(interval_secs).await;
                let state = self.current_state().await;
                if state.peers.is_empty() {
                    continue;
                }
                if let Err(e) = self.broadcast(&P2pMsg::State(state)).await {
                    tracing::debug!("periodic state broadcast failed: {e}");
                }
            }
        };
        stroemnet_protocol::spawn(fut);
    }

    pub async fn broadcast(&self, msg: &P2pMsg) -> Result<()> {
        let bytes = encode(msg)?;
        self.observe(&bytes).await;
        self.send_bytes_to_all(bytes, None).await;
        Ok(())
    }

    pub async fn send_to(&self, url: &str, msg: &P2pMsg) -> Result<()> {
        let bytes = encode(msg)?;
        let peer = {
            let peers = self.connected_peers.lock().await;
            peers.iter().find(|p| p.url == url).cloned()
        };
        if let Some(peer) = peer {
            send_timed(&peer, bytes).await?;
        }
        Ok(())
    }

    pub async fn forward(&self, from: &str, msg: &P2pMsg) -> Result<bool> {
        let bytes = encode(msg)?;
        if !self.observe(&bytes).await {
            return Ok(false);
        }
        self.send_bytes_to_all(bytes, Some(from)).await;
        Ok(true)
    }

    pub async fn observe(&self, payload: &[u8]) -> bool {
        self.seen.lock().await.insert(SeenSet::hash(payload))
    }

    async fn send_bytes_to_all(&self, bytes: Vec<u8>, exclude: Option<&str>) {
        let targets: Vec<ConnectedPeer> = {
            let peers = self.connected_peers.lock().await;
            peers
                .iter()
                .filter(|p| Some(p.url.as_str()) != exclude)
                .cloned()
                .collect()
        };
        for peer in targets {
            if let Err(e) = send_timed(&peer, bytes.clone()).await {
                tracing::warn!("send to {}: {e}", peer.url);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::super::config::P2pConfig;
    use super::*;
    use stroemnet_protocol::v1::RevealV1;

    #[tokio::test]
    async fn forward_returns_false_on_duplicate() {
        let (net, _evts) = P2p::new(P2pConfig::default());
        let msg = P2pMsg::Reveal(RevealV1::new([7; 32], [0; 32]));
        assert!(net.forward("peer-a", &msg).await.unwrap());
        assert!(!net.forward("peer-b", &msg).await.unwrap());
    }

    #[tokio::test]
    async fn broadcast_marks_seen_to_block_loopback() {
        let (net, _evts) = P2p::new(P2pConfig::default());
        let msg = P2pMsg::Reveal(RevealV1::new([8; 32], [0; 32]));
        net.broadcast(&msg).await.unwrap();
        assert!(!net.forward("relayer", &msg).await.unwrap());
    }
}

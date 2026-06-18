use std::collections::HashSet;
use std::sync::Arc;

use futures::channel::mpsc;
use futures::lock::Mutex;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::mpsc::UnboundedSender;

use crate::Result;
use crate::gossip::SeenSet;
use crate::identity::Identity;
use crate::peer::ConnectedPeer;
use crate::wire::encode;
use crate::wire::message::P2pMsg;

#[derive(Debug, Clone)]
/// Configuration for the p2p protocol
pub struct P2pConfig {
    /// A unique identity for each node.
    pub identity: Identity,
    /// Bootstrap peers for those that are first-joiners to the network.
    pub bootstrap_peers: Vec<String>,
    /// Target number of outbound connections.  
    pub target_outbound: usize,
    /// Limit the number of inbound connections up to the max
    pub max_inbound: usize,
    /// If we are an LP node, we have also provided our listen
    /// address
    pub advertised_listen_addr: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    /// Only for non WASM can we accept peers from
    /// other peers' states.
    pub discovered_peer_dial_tx: Option<UnboundedSender<String>>,
}

impl P2pConfig {
    /// Get the node id for this p2p configuration
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

/// The main struct for managing the p2p network
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

    pub async fn connected_peer_addrs(&self) -> Vec<crate::wire::message::PeerAddr> {
        let our_listen = self
            .config
            .advertised_listen_addr
            .as_deref()
            .map(|s| s.trim_end_matches('/'));
        self.connected_peers
            .lock()
            .await
            .iter()
            .filter_map(|p| {
                let url = p.advertised_listen.clone()?;
                if let Some(ours) = our_listen
                    && url.trim_end_matches('/').eq_ignore_ascii_case(ours) {
                        return None;
                    }
                Some(crate::wire::message::PeerAddr {
                    url,
                    last_seen: p.connected_at,
                })
            })
            .collect()
    }

    pub async fn process_peer_addrs(&self, addrs: Vec<crate::wire::message::PeerAddr>) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = addrs;
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.process_peer_addrs_native(addrs).await;
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn process_peer_addrs_native(&self, addrs: Vec<crate::wire::message::PeerAddr>) {
        let Some(dial_tx) = self.config.discovered_peer_dial_tx.as_ref() else {
            return;
        };
        let our_listen = self
            .config
            .advertised_listen_addr
            .as_deref()
            .map(|s| s.trim_end_matches('/').to_ascii_lowercase());
        let connected: std::collections::HashSet<String> = self
            .connected_peers
            .lock()
            .await
            .iter()
            .filter_map(|p| p.advertised_listen.clone())
            .map(|u| u.trim_end_matches('/').to_ascii_lowercase())
            .collect();
        let target = self.config.target_outbound;
        let mut requested = 0;
        for entry in addrs {
            if connected.len() + requested >= target {
                break;
            }
            let url_norm = entry.url.trim_end_matches('/').to_ascii_lowercase();
            if our_listen.as_deref() == Some(url_norm.as_str()) {
                continue;
            }
            if connected.contains(&url_norm) {
                continue;
            }
            if !url_norm.starts_with("ws://") && !url_norm.starts_with("wss://") {
                tracing::debug!("addr: skipping non-ws URL {}", entry.url);
                continue;
            }
            if let Err(e) = dial_tx.send(entry.url.clone()) {
                tracing::warn!("discovery: dial channel closed dropping {}: {e}", entry.url);
                return;
            }
            requested += 1;
        }
    }

    pub async fn current_state(&self) -> crate::wire::message::NodeState {
        crate::wire::message::NodeState {
            node_id: self.config.node_id(),
            listen_addr: self.config.advertised_listen_addr.clone(),
            peers: self.connected_peer_addrs().await,
        }
    }

    pub async fn is_connected_peer(&self, listen_url_norm: &str) -> bool {
        self.connected_peers.lock().await.iter().any(|p| {
            p.advertised_listen
                .as_deref()
                .map(|u| {
                    u.trim_end_matches('/')
                        .eq_ignore_ascii_case(listen_url_norm)
                })
                .unwrap_or(false)
        })
    }

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
        let peers = self.connected_peers.lock().await;
        if let Some(peer) = peers.iter().find(|p| p.url == url) {
            peer.send_bytes(bytes).await?;
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
        let peers = self.connected_peers.lock().await;
        for peer in peers.iter() {
            if Some(peer.url.as_str()) == exclude {
                continue;
            }
            if let Err(e) = peer.send_bytes(bytes.clone()).await {
                tracing::warn!("send to {}: {e}", peer.url);
            }
        }
    }

    pub async fn dial(&self, url: &str) -> Result<ConnectedPeer> {
        use crate::transport::WsTransport;

        let our_state = self.current_state().await;
        let transport = WsTransport::dial(url).await?;
        ConnectedPeer::handshake(url.to_string(), transport, our_state, false).await
    }

    pub async fn dial_with_backoff(&self, url: &str) -> Option<ConnectedPeer> {
        let mut delay_ms: u64 = 1_000;
        let max_ms: u64 = 60_000;
        let mut last_error = None;
        for _ in 1..=5 {
            match self.dial(url).await {
                Ok(p) => return Some(p),
                Err(e) => {
                    last_error = Some(e);
                    stroemnet_protocol::sleep_ms(delay_ms).await;
                    delay_ms = (delay_ms * 2).min(max_ms);
                }
            }
        }
        tracing::warn!("dial {url} giving up after 5 attempts: {last_error:?}");
        None
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn listen(
        self: Arc<Self>,
        bind_addr: std::net::SocketAddr,
    ) -> Result<tokio::sync::mpsc::Receiver<ConnectedPeer>> {
        use crate::error::StroemnetP2pError;
        use crate::transport::WsTransport;
        use tokio::net::TcpListener;
        use tokio_tungstenite::accept_async;

        let listener = TcpListener::bind(bind_addr)
            .await
            .map_err(|e| StroemnetP2pError::Io(format!("bind {bind_addr}: {e}")))?;
        let (tx, rx) = tokio::sync::mpsc::channel(32);

        let net = self;
        tokio::spawn(async move {
            loop {
                let (stream, addr) = match listener.accept().await {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("accept failed: {e}");
                        continue;
                    }
                };
                let tx = tx.clone();
                let net = net.clone();
                tokio::spawn(async move {
                    let ws = match accept_async(stream).await {
                        Ok(ws) => ws,
                        Err(e) => {
                            tracing::warn!("ws accept from {addr}: {e}");
                            return;
                        }
                    };
                    let transport = WsTransport::from_inbound(ws);
                    let our_s = net.current_state().await;
                    match ConnectedPeer::handshake(format!("ws://{addr}"), transport, our_s, true)
                        .await
                    {
                        Ok(peer) => {
                            match peer.advertised_listen.as_deref() {
                                Some(listen) => tracing::info!(
                                    "inbound peer node_id={} accepted (listen={listen}, src={addr})",
                                    hex::encode(peer.node_id)
                                ),
                                None => tracing::info!(
                                    "inbound peer node_id={} accepted (non-listening, src={addr})",
                                    hex::encode(peer.node_id)
                                ),
                            }
                            let _ = tx.send(peer).await;
                        }
                        Err(e) => tracing::warn!("inbound handshake from {addr} failed: {e}"),
                    }
                });
            }
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
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

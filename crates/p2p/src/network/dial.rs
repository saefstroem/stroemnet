#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use super::state::P2p;
use crate::Result;
use crate::error::StroemnetP2pError;
use crate::peer::ConnectedPeer;
use crate::transport::WsTransport;
use tokio::net::TcpListener;

impl P2p {
    /// Dial a peer and handshake which leads to an open connection
    pub async fn dial(&self, url: &str) -> Result<ConnectedPeer> {
        let our_state = self.current_state().await;
        let transport = WsTransport::dial(url).await?;
        ConnectedPeer::handshake(url.to_string(), transport, our_state, false).await
    }

    /// Dial the peer with a backoff and retry
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
    /// Create a listener for incoming connections and return a receiver
    /// which will emit connections
    pub async fn listen(
        self: Arc<Self>,
        bind_addr: std::net::SocketAddr,
    ) -> Result<tokio::sync::mpsc::Receiver<ConnectedPeer>> {
        // BInd the listener
        let listener = TcpListener::bind(bind_addr)
            .await
            .map_err(|e| StroemnetP2pError::Io(format!("bind {bind_addr}: {e}")))?;
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let net = self;
        let limiter = Arc::new(tokio::sync::Semaphore::new(net.config.max_inbound));
        tokio::spawn(async move {
            loop {
                let (stream, addr) = match listener.accept().await {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("accept failed: {e}");
                        continue;
                    }
                };

                // Limit the number of inbound connections via semaphore
                let Ok(permit) = limiter.clone().try_acquire_owned() else {
                    tracing::warn!("inbound from {addr} rejected: handshake limit reached");
                    continue;
                };
                let tx = tx.clone();
                let net = net.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    accept_inbound(net, stream, addr, tx).await;
                });
            }
        });
        Ok(rx)
    }
}

#[cfg(not(target_arch = "wasm32"))]
const HANDSHAKE_TIMEOUT_SECS: u64 = 10;

#[cfg(not(target_arch = "wasm32"))]
async fn accept_inbound(
    net: Arc<P2p>,
    stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
    tx: tokio::sync::mpsc::Sender<ConnectedPeer>,
) {
    // Accept the websocket connection future closure
    let handshake = async {
        let ws = tokio_tungstenite::accept_async_with_config(
            stream,
            Some(crate::transport::ws_config()),
        )
        .await
        .map_err(|e| crate::error::StroemnetP2pError::Io(format!("ws accept {addr}: {e}")))?;
        let transport = WsTransport::from_inbound(ws);
        let our_s = net.current_state().await;
        ConnectedPeer::handshake(format!("ws://{addr}"), transport, our_s, true).await
    };
    let timeout = std::time::Duration::from_secs(HANDSHAKE_TIMEOUT_SECS);
    
    // Race handhsake against timeout
    match tokio::time::timeout(timeout, handshake).await {
        Ok(Ok(peer)) => {
            tracing::info!(
                "inbound peer node_id={} accepted (src={addr})",
                hex::encode(peer.node_id)
            );
            let _ = tx.send(peer).await;
        }
        Ok(Err(e)) => tracing::warn!("inbound handshake from {addr} failed: {e}"),
        Err(_) => tracing::warn!("inbound handshake from {addr} timed out"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::P2pConfig;
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn dial_requires_live_peer() {
        let (net, _rx) = P2p::new(P2pConfig::default());
        assert!(net.dial("ws://127.0.0.1:1").await.is_err());
    }
}

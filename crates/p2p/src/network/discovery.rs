use super::state::P2p;
use crate::wire::message::PeerAddr;

impl P2p {
    /// Processer a list of peers
    pub async fn process_peer_addrs(&self, addrs: Vec<PeerAddr>) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = addrs;
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.process_peer_addrs_native(addrs).await;
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Process a list of peers
    async fn process_peer_addrs_native(&self, addrs: Vec<PeerAddr>) {
        let Some(dial_tx) = self.config.discovered_peer_dial_tx.as_ref() else {
            return;
        };
        let our_listen = self
            .config
            .advertised_listen_addr
            .as_deref()
            .map(crate::normalize_listen_addr); // compute our listening address
        
        // compute all connected peers listening address
        let connected: std::collections::HashSet<String> = self
            .connected_peers
            .lock()
            .await
            .iter()
            .filter_map(|p| p.advertised_listen.clone())
            .map(|u| crate::normalize_listen_addr(&u))
            .collect();
        let target = self.config.target_outbound;
        let mut requested = 0;

        // Go over all addresses
        for entry in addrs {
            if connected.len() + requested >= target {
                break;
            }
            let url_norm = crate::normalize_listen_addr(&entry.url);
            if our_listen.as_deref() == Some(url_norm.as_str()) || connected.contains(&url_norm) {
                continue;
            }
            if !url_norm.starts_with("ws://") && !url_norm.starts_with("wss://") {
                tracing::debug!("addr: skipping non-ws URL {}", entry.url);
                continue;
            }

            // Transmit to the dialler that we request to call this peer
            if let Err(e) = dial_tx.send(entry.url.clone()) {
                tracing::warn!("discovery: dial channel closed dropping {}: {e}", entry.url);
                return;
            }
            requested += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::P2pConfig;
    use super::*;

    #[tokio::test]
    async fn no_dial_tx_is_a_safe_noop() {
        let (net, _rx) = P2p::new(P2pConfig::default());
        net.process_peer_addrs(vec![PeerAddr {
            url: "ws://x/".into(),
            last_seen: 0,
        }])
        .await;
        assert!(net.connected_peer_addrs().await.is_empty());
    }
}

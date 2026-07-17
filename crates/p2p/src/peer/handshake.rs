use crate::Result;
use crate::error::StroemnetP2pError;
use crate::peer::ConnectedPeer;
use crate::transport::WsTransport;
use crate::wire::message::{NodeState, P2pMsg};
use crate::wire::{decode, encode};

impl ConnectedPeer {
    /// Performs the inner handshake which creates a confirmed
    /// connected and healthy peer connection
    pub async fn handshake(
        url: String,
        transport: WsTransport,
        our_state: NodeState,
        is_inbound: bool,
    ) -> Result<Self> {
        let our_id = our_state.node_id;
        let our_listen = our_state.listen_addr.clone();

        // Transmit the p2p state to the peer
        let send_fut = async {
            let bytes = encode(&P2pMsg::State(our_state))?;
            transport.send(bytes).await
        };
        // Create closure to read from this peer
        let recv_fut = async {
            let bytes = transport.recv().await?;
            decode(&bytes)
        };
        let (send_res, recv_res) = futures::join!(send_fut, recv_fut);
        send_res?;

        // We expect to receive a state from the peer too
        let peer_state = match recv_res? {
            P2pMsg::State(s) => s,
            other => {
                return Err(StroemnetP2pError::HandshakeFailed(format!(
                    "expected state, got {other:?}"
                )));
            }
        };
        // Ensure that we are not trying to handshake ourselves
        Self::check_self_loop(&peer_state, our_id, our_listen.as_deref())?;

        // Since we got here means we have valid state from peer
        let mut peer = ConnectedPeer::new(url.clone(), transport);
        peer.node_id = peer_state.node_id;
        peer.advertised_listen = if is_inbound {
            peer_state.listen_addr.clone()
        } else {
            Some(peer_state.listen_addr.unwrap_or(url))
        };
        peer.connected_at = stroemnet_protocol::now_unix_secs();
        peer.is_inbound = is_inbound;
        peer.known_peers = peer_state.peers;

        // Return the connected peer
        Ok(peer)
    }

    fn check_self_loop(
        peer: &NodeState,
        our_node_id: [u8; 32],
        our_listen: Option<&str>,
    ) -> Result<()> {
        if peer.node_id == our_node_id {
            return Err(StroemnetP2pError::HandshakeFailed(
                "rejecting self-loop: peer node_id matches ours".to_string(),
            ));
        }
        if let (Some(ours), Some(theirs)) = (our_listen, peer.listen_addr.as_deref())
            && crate::listen_addrs_equal(ours, theirs)
        {
            tracing::warn!(
                "rejecting peer (node_id={}) advertising our own listen_addr {theirs} \
                     — likely a misconfigured EXTERNAL_HOSTNAME shared across multiple nodes",
                hex::encode(peer.node_id)
            );
            return Err(StroemnetP2pError::HandshakeFailed(format!(
                "peer advertises our listen_addr ({theirs})"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::transport::loopback_pair;

    fn test_state(id: u8) -> NodeState {
        NodeState {
            node_id: [id; 32],
            listen_addr: None,
            peers: Vec::new(),
        }
    }

    #[tokio::test]
    async fn handshake_succeeds() {
        let (a, b) = loopback_pair().await;
        let init_s = test_state(1);
        let resp_s = test_state(2);

        let init_handle = tokio::spawn(async move {
            ConnectedPeer::handshake("ws://a".into(), a, init_s, false).await
        });
        let peer_for_resp = ConnectedPeer::handshake("ws://b".into(), b, resp_s, true)
            .await
            .unwrap();
        let peer_for_init = init_handle.await.unwrap().unwrap();

        assert_eq!(peer_for_init.node_id, [2; 32]);
        assert_eq!(peer_for_resp.node_id, [1; 32]);
    }

    #[tokio::test]
    async fn handshake_rejects_self_node_id() {
        let (a, b) = loopback_pair().await;
        let our = test_state(7);
        let peer = test_state(7);

        let init_handle =
            tokio::spawn(
                async move { ConnectedPeer::handshake("ws://a".into(), a, our, false).await },
            );
        let err = ConnectedPeer::handshake("ws://b".into(), b, peer, true)
            .await
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("self-loop") || msg.contains("node_id"),
            "expected self-loop error, got {msg}"
        );
        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), init_handle).await;
    }

    #[tokio::test]
    async fn handshake_rejects_peer_advertising_our_listen_addr() {
        let (a, b) = loopback_pair().await;
        let mut our = test_state(1);
        our.listen_addr = Some("ws://lp.example.com:3000".into());
        let mut peer = test_state(2);
        peer.listen_addr = Some("ws://lp.example.com:3000/".into());

        let init_handle =
            tokio::spawn(
                async move { ConnectedPeer::handshake("ws://a".into(), a, our, false).await },
            );
        let err = ConnectedPeer::handshake("ws://b".into(), b, peer, true)
            .await
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("listen_addr"),
            "expected listen_addr conflict, got {msg}"
        );
        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), init_handle).await;
    }

    #[tokio::test]
    async fn handshake_allows_distinct_listen_addrs() {
        let (a, b) = loopback_pair().await;
        let mut init_s = test_state(1);
        init_s.listen_addr = Some("ws://node-a:3000".into());
        let mut resp_s = test_state(2);
        resp_s.listen_addr = Some("ws://node-b:3001".into());

        let init_handle = tokio::spawn(async move {
            ConnectedPeer::handshake("ws://a".into(), a, init_s, false).await
        });
        let peer_for_responder = ConnectedPeer::handshake("ws://b".into(), b, resp_s, true)
            .await
            .unwrap();
        assert_eq!(
            peer_for_responder.advertised_listen.as_deref(),
            Some("ws://node-a:3000")
        );
        let peer_for_initiator = init_handle.await.unwrap().unwrap();
        assert_eq!(
            peer_for_initiator.advertised_listen.as_deref(),
            Some("ws://node-b:3001")
        );
    }

    #[test]
    fn listen_addrs_equal_ignores_trailing_slash_and_case() {
        assert!(crate::listen_addrs_equal("ws://x:3000", "ws://x:3000/"));
        assert!(crate::listen_addrs_equal("WS://X:3000/", "ws://x:3000"));
        assert!(!crate::listen_addrs_equal("ws://x:3000", "ws://x:3001"));
    }
}

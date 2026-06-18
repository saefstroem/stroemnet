use stroemnet_p2p::peer::ConnectedPeer;
use stroemnet_p2p::transport::loopback_pair;

pub async fn paired_peers() -> (ConnectedPeer, ConnectedPeer) {
    let (a, b) = loopback_pair().await;
    let a_peer = ConnectedPeer::new("ws://test-a".into(), a);
    let b_peer = ConnectedPeer::new("ws://test-b".into(), b);
    (a_peer, b_peer)
}

pub struct LoopbackNetwork {
    pub peers: Vec<ConnectedPeer>,
}

impl LoopbackNetwork {
    pub async fn ring(n: usize) -> Self {
        assert!(n >= 2, "ring needs at least 2 peers");
        let mut peers = Vec::with_capacity(n);
        for i in 0..n {
            let (a, _b) = loopback_pair().await;
            peers.push(ConnectedPeer::new(format!("ws://node{i}"), a));
        }
        Self { peers }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn paired_peers_work() {
        let (a, b) = paired_peers().await;
        a.send_bytes(b"hello".to_vec()).await.unwrap();
        let got = b.transport.recv().await.unwrap();
        assert_eq!(got, b"hello");
    }

    #[tokio::test]
    async fn ring_constructs() {
        let net = LoopbackNetwork::ring(3).await;
        assert_eq!(net.peers.len(), 3);
    }
}

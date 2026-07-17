#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::time::Duration;

use stroemnet_p2p::transport::{WsTransport, loopback_pair};
use stroemnet_p2p::wire::message::P2pMsg;
use stroemnet_p2p::wire::{decode, encode};
use stroemnet_protocol::v1::RevealV1;

struct Mesh {
    edges: Vec<((usize, usize), WsTransport)>,
}

impl Mesh {
    async fn build(n: usize) -> Self {
        let mut edges = Vec::new();
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let (a, _b) = loopback_pair().await;
                edges.push(((i, j), a));
            }
        }
        Self { edges }
    }

    fn out(&self, from: usize, to: usize) -> &WsTransport {
        self.edges
            .iter()
            .find(|(k, _)| *k == (from, to))
            .map(|(_, t)| t)
            .expect("edge exists")
    }
}

#[tokio::test]
async fn loopback_delivers_arbitrary_p2p_message() {
    let swap_id = [9u8; 32];

    let (sender, receiver) = loopback_pair().await;
    let reveal = RevealV1::new(swap_id, [0xAB; 32]);
    let msg_bytes = encode(&P2pMsg::Reveal(reveal)).unwrap();
    sender.send(msg_bytes).await.unwrap();

    let received = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("peer should receive message within 1s")
        .unwrap();
    match decode(&received).unwrap() {
        P2pMsg::Reveal(r) => {
            assert_eq!(r.swap_id, swap_id);
            assert_eq!(r.secret, [0xAB; 32]);
        }
        other => panic!("expected reveal, got {other:?}"),
    }

    let mesh = Mesh::build(3).await;
    let _ = mesh.out(0, 1);
}

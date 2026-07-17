pub mod handshake;

use crate::Result;
use crate::transport::WsTransport;
use crate::wire::decode;
use crate::wire::message::{P2pMsg, PeerAddr};

#[derive(Clone)]
pub struct ConnectedPeer {
    pub url: String,
    pub transport: WsTransport,
    pub node_id: [u8; 32],
    /// A wasm node does not have a listening address
    /// hence this is optional
    pub advertised_listen: Option<String>,
    pub connected_at: u64,
    pub is_inbound: bool,
    pub known_peers: Vec<PeerAddr>,
}

impl ConnectedPeer {
    pub fn new(url: String, transport: WsTransport) -> Self {
        Self {
            url,
            transport,
            node_id: [0; 32],
            advertised_listen: None,
            connected_at: 0,
            is_inbound: false,
            known_peers: Vec::new(),
        }
    }

    pub async fn recv_msg(&self) -> Result<P2pMsg> {
        let bytes = self.transport.recv().await?;
        decode(&bytes)
    }

    pub async fn send_bytes(&self, bytes: Vec<u8>) -> Result<()> {
        self.transport.send(bytes).await
    }

    pub async fn disconnect(&self) -> Result<()> {
        self.transport.close().await
    }
}

impl std::fmt::Debug for ConnectedPeer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectedPeer")
            .field("url", &self.url)
            .field("node_id", &hex::encode(self.node_id))
            .finish()
    }
}

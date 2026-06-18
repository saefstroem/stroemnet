pub mod error;
pub mod gossip;
pub mod identity;
pub mod network;
pub mod peer;
pub mod transport;
pub mod wire;

pub use error::StroemnetP2pError;
pub use identity::{Identity, proposal_digest};
pub use network::{P2p, P2pConfig};
pub use transport::WsTransport;
pub use wire::P2pMsg;

pub type Result<T> = std::result::Result<T, StroemnetP2pError>;

pub const SEED_NODES: &[&str] = &[];

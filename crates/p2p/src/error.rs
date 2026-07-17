use thiserror::Error;

#[derive(Error, Debug)]
pub enum StroemnetP2pError {
    #[error("Borsh codec error: {0}")]
    Codec(#[from] borsh::io::Error),

    #[error("Inbound P2P message too large: {size} bytes (max {max})")]
    MessageTooLarge { size: usize, max: usize },

    #[error("IO error: {0}")]
    Io(String),

    #[error("Transport closed")]
    TransportClosed,

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),
}

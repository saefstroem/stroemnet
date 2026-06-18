use thiserror::Error;

pub type Result<T> = std::result::Result<T, DataError>;

#[derive(Debug, Error)]
pub enum DataError {
    #[error("unknown channel: {0}")]
    UnknownChannel(stroemnet_protocol::ChannelId),

    #[error("config: {0}")]
    Config(String),

    #[error("connect: {0}")]
    Connect(String),

    #[error("rpc: {0}")]
    Rpc(String),

    #[error("sign: {0}")]
    Sign(String),

    #[error("broadcast: {0}")]
    Broadcast(String),

    #[error("missing signing key for {0}")]
    MissingKey(stroemnet_protocol::ChannelId),

    #[error("{0}")]
    Other(String),
}

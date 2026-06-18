use thiserror::Error;

#[cfg(not(target_arch = "wasm32"))]
use crate::oracle::OracleError;

#[derive(Error, Debug)]
pub enum StroemnetError {
    #[cfg(not(target_arch = "wasm32"))]
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[cfg(not(target_arch = "wasm32"))]
    #[error("Oracle error: {0}")]
    Oracle(#[from] OracleError),

    #[cfg(not(target_arch = "wasm32"))]
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Float parse error: {0}")]
    ParseFloat(#[from] std::num::ParseFloatError),

    #[error("Env error: {0}")]
    Env(String),

    #[cfg(not(target_arch = "wasm32"))]
    #[error("Peer db error: {0}")]
    PeerDb(#[from] stroemnet_storage::DbError),

    #[error("Invalid channel ID: {0}")]
    InvalidChannelId(String),

    #[error("LP mode forbids trade initiation")]
    LpModeForbidsInitiation,

    #[error("secret/commitment mismatch: sha256(secret) != commitment.secret_hash")]
    SecretHashMismatch,

    #[error("{0}")]
    Other(String),
}

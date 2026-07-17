#[derive(Debug, thiserror::Error)]
pub(crate) enum RecordError {
    #[error("record decode: {0}")]
    Decode(#[from] std::io::Error),
    #[error("unknown record version {0}")]
    UnknownVersion(u8),
    #[error("record too short")]
    Truncated,
}

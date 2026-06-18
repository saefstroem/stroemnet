use thiserror::Error;

#[derive(Debug, Error)]
pub enum SwapTrackerError {
    #[error("Swap with id {0:?} already exists")]
    DuplicateSwap([u8; 32]),

    #[error("Swap with id {0:?} not found")]
    SwapNotFound([u8; 32]),

    #[error("Swap with id {0:?} already has a counter commitment")]
    AlreadyCounterLocked([u8; 32]),

    #[error("Swap with id {0:?} does not have a counter commitment yet")]
    NotCounterLocked([u8; 32]),

    #[error("Swap with id {0:?} is already resolved")]
    AlreadyResolved([u8; 32]),

    #[error("Cross-chain validation failed for swap {swap_id:?}: {reason}")]
    ValidationFailed { swap_id: [u8; 32], reason: String },

    #[error(
        "Reveal secret hash mismatch for swap {swap_id:?}: H(secret) does not equal commitment secret_hash"
    )]
    SecretHashMismatch { swap_id: [u8; 32] },
}

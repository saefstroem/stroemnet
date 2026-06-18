use alloy::{
    primitives::{U256, ruint::ParseError},
    signers::local::LocalSignerError,
};
use thiserror::Error;

use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::ChainEvent;

#[derive(Error, Debug)]
pub enum HandlerError {
    #[error("Alloy ruint parse error: {0}")]
    AlloyRuintParse(#[from] ParseError),

    #[error("Missing price data for channel: {0:?}")]
    MissingPriceData(ChannelId),

    #[error("Trade amount {amount_in} USD value {amount_in_usd} is below minimum of {min_usd} USD")]
    TradeTooSmall {
        amount_in: String,
        amount_in_usd: f64,
        min_usd: f64,
    },

    #[error("Trade amount {amount_in} USD value {amount_in_usd} is above maximum of {max_usd} USD")]
    TradeTooLarge {
        amount_in: String,
        amount_in_usd: f64,
        max_usd: f64,
    },

    #[error("Missing address for channel: {0:?}")]
    MissingAddress(ChannelId),

    #[error("Alloy local signer error: {0}")]
    LocalSigner(#[from] LocalSignerError),

    #[error("Parse float error: {0}")]
    ParseFloat(#[from] std::num::ParseFloatError),

    #[error("Swap tracker error: {0}")]
    SwapTracker(#[from] stroemnet_protocol::swap_tracker::SwapTrackerError),

    #[error("Swap not found for swap ID: {0:?}")]
    SwapNotFound([u8; 32]),

    #[error("Invalid state for swap ID: {0:?}")]
    InvalidState([u8; 32]),

    #[error("Invalid channel id: {0:?}")]
    InvalidChannelId(ChannelId),

    #[error("Commitment for swap {0:?} is not addressed to this LP — ignoring")]
    NotAddressedToUs([u8; 32]),

    #[error("System time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("Invalid price data: {0}")]
    InvalidPriceData(f64),

    #[error("Invalid amount: {0}")]
    InvalidAmount(U256),

    #[error("Invalid lock time duration: {0}")]
    InvalidLockTimeDuration(u64),

    #[error("Chain time unavailable for channel: {0:?}")]
    ChainTimeUnavailable(ChannelId),

    #[error("Price error: {0}")]
    Price(#[from] stroemnet_amounts::AmountError),

    #[error("Unknown channel: {0:?}")]
    UnknownChannel(ChannelId),

    #[error("Send error event to channel: {0:?}")]
    SendEventToChannel(#[from] tokio::sync::mpsc::error::SendError<ChainEvent>),

    #[error("Recv error from channel: {0:?}")]
    RecvFromChannel(#[from] tokio::sync::oneshot::error::RecvError),

    #[error("Other error: {0}")]
    Other(String),
}

impl From<String> for HandlerError {
    fn from(s: String) -> Self {
        HandlerError::Other(s)
    }
}

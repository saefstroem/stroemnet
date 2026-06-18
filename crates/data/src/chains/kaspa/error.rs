use std::sync::Arc;

use kaspa_txscript_errors::TxScriptError;
use kaspa_wrpc_client::prelude::RpcBlock;
use thiserror::Error;

use super::contracts::contract_v1::DataType;
use crate::DataError;

pub(super) type Result<T> = std::result::Result<T, KaspaError>;

#[derive(Error, Debug)]
pub enum KaspaError {
    #[error("Kaspa Tx Script Error: {0}")]
    TxScript(#[from] TxScriptError),

    #[error("HTLC Validation: Too many opcodes in script")]
    TooManyOpcodes,

    #[error("HTLC Validation: Opcode mismatch at position {0}")]
    OpcodeMismatch(usize),

    #[error("HTLC Validation: Missing data for datatype {0:?}")]
    MissingData(DataType),

    #[error("HTLC Validation: Invalid secret hash length")]
    InvalidSecretHashLength,

    #[error("HTLC Validation: Invalid swap ID length")]
    InvalidSwapIdLength,

    #[error("Invalid signature script length: expected {expected}, got {got}")]
    InvalidSigScriptLength { expected: usize, got: usize },

    #[error("Wrong branch selector: expected 0x{expected:02x}, got 0x{got:02x}")]
    WrongBranchSelector { expected: u8, got: u8 },

    #[error("Missing secret in signature script")]
    MissingSecret,

    #[error("Missing signature in signature script")]
    MissingSignature,

    #[error("Missing redeem script in signature script")]
    MissingRedeemScript,

    #[error("Invalid secret length: expected 32 bytes")]
    InvalidSecretLength,

    #[error("Other error: {0}")]
    Other(String),

    #[error("Script builder error: {0}")]
    ScriptBuilder(String),

    #[error("Failed to parse amount: {0}")]
    AmountParse(String),

    #[error("Parse int error: {0}")]
    ParseInt(#[from] std::num::ParseIntError),

    #[error("Kaspa address parsing error: {0}")]
    AddressParse(#[from] kaspa_addresses::AddressError),

    #[error("From utf8 error: {0}")]
    FromUtf8(#[from] std::string::FromUtf8Error),

    #[error("Invalid sigmsg detail length: expected {expected}, got {got}")]
    InvalidSigMsgDetailLength { expected: usize, got: usize },

    #[error("Kaspa Rpc Tx Error: {0}")]
    RpcTx(#[from] kaspa_wrpc_client::prelude::RpcError),

    #[error("Kaspa WRpc Client error: {0}")]
    RpcClient(#[from] kaspa_wrpc_client::error::Error),

    #[error("Safe block send error: {0}")]
    SafeBlockSend(#[from] tokio::sync::mpsc::error::SendError<Arc<RpcBlock>>),

    #[error("Missing channel id for destination: {0:?}")]
    MissingChannelId(stroemnet_protocol::ChannelId),

    #[error("Swap not found: {}", hex::encode(_0))]
    SwapNotFound([u8; 32]),

    #[error("HTLC UTXO not found on-chain for swap: {}", hex::encode(_0))]
    HtlcUtxoNotFound([u8; 32]),

    #[error(
        "ScriptAnnounce validation: announced address {announced} does not match P2SH derived from script ({derived})"
    )]
    ScriptAnnounceAddressMismatch { announced: String, derived: String },

    #[error(
        "ScriptAnnounce validation: swap_id {} does not match announced {}",
        hex::encode(_0),
        hex::encode(_1)
    )]
    ScriptAnnounceSwapIdMismatch([u8; 32], [u8; 32]),

    #[error(
        "ScriptAnnounce validation: timelock {script_secs}s does not match announced {announced_secs}s"
    )]
    ScriptAnnounceTimelockMismatch {
        script_secs: u64,
        announced_secs: u64,
    },

    #[error("Insufficient funds: need {needed} sompi, have {available} sompi")]
    InsufficientFunds { needed: u64, available: u64 },

    #[error("No UTXOs available for address")]
    NoUtxos,
}

impl From<String> for KaspaError {
    fn from(s: String) -> Self {
        KaspaError::Other(s)
    }
}

impl From<KaspaError> for DataError {
    fn from(e: KaspaError) -> Self {
        DataError::Other(e.to_string())
    }
}

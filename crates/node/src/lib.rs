#[cfg(target_arch = "wasm32")]
mod claim;
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
mod connection;
pub mod coordinator;
pub mod error;
mod node;
pub mod oracle;
pub mod result;

#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;

use ahash::AHashMap;
use serde_json::Value;
use stroemnet_protocol::ChannelId;

pub use stroemnet_handler::HandlerConfig;

#[cfg(target_arch = "wasm32")]
pub use crate::claim::{PendingClaim, pending_claim_matches};
pub use crate::coordinator::Role as NodeRole;
pub use crate::node::Node;

#[derive(Clone)]
/// Specification for a chain channel,
/// including its configuration and optional wallet information for signing transactions.
pub struct ChannelSpec {
    pub config: Value,
    pub lp_private_key: Option<String>,
}

impl ChannelSpec {
    /// Get the minimum block confirmations required for this channel, defaulting to 0 if not specified.
    pub fn minimum_block_confirmations(&self) -> u64 {
        self.config
            .get("minimum_block_confirmations")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }
}

/// Configuration for the node
pub struct NodeConfig {
    /// Configuration for the handler in terms of
    /// min/max swap amounts, spread percents
    /// and commit buffer in seconds
    pub handler: HandlerConfig,
    /// Each channel's specific specification containing
    /// config and wallet configuration
    pub channels: AHashMap<ChannelId, ChannelSpec>,
    #[cfg(not(target_arch = "wasm32"))]
    /// The address the node should bind to for incoming peer connections, if any.
    pub bind_addr: Option<SocketAddr>,
    #[cfg(not(target_arch = "wasm32"))]
    /// The interval in seconds at which the node should update price information from oracles.
    pub price_oracle_update_interval_secs: u64,
    /// A list of bootstrap peer addresses that the node should connect to for discovering other peers in the network.
    pub bootstrap_peers: Vec<String>,
    /// The role of the node in the network, which can affect its behavior and responsibilities.
    pub role: NodeRole,
    /// This is the address that the node will advertise to other peers
    /// that it is listening on.
    pub advertised_listen_addr: Option<String>,
    #[cfg(target_arch = "wasm32")]
    /// A sender for reporting swap status updates, so that the UI can be updated with the latest information.
    pub swap_status_tx: tokio::sync::mpsc::UnboundedSender<SwapStatusUpdate>,
}

#[cfg_attr(target_arch = "wasm32", derive(tsify_next::Tsify))]
#[cfg_attr(target_arch = "wasm32", tsify(into_wasm_abi, from_wasm_abi))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
/// Represents the current stage of a swap, including relevant information for each stage.
pub enum SwapStage {
    /// The swap has been initiated and a quote has been obtained, but the deposit has not yet been made.
    AwaitingDeposit {
        #[cfg_attr(
            target_arch = "wasm32",
            tsify(type = "\"KaspaTn10\" | \"EthereumSepolia\" | \"IgraGalleon\"")
        )]
        chain: ChannelId,
        address: String,
        deposit_target: Option<String>,
    },
    /// The user has submitted the initial commitment
    CommitSubmitted {
        unlock_ts: u64,
    },
    Locked,
    /// The swap has been fully completed, with the output amount received and the swap finalized.
    Completed,
    /// The swap has failed, with a reason provided for the failure.
    Failed {
        reason: String,
    },
}

#[cfg_attr(target_arch = "wasm32", derive(tsify_next::Tsify))]
#[cfg_attr(target_arch = "wasm32", tsify(into_wasm_abi, from_wasm_abi))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
/// Represents an update to the status of a swap, including the swap ID, current stage, and timestamp of the update.
pub struct SwapStatusUpdate {
    /// The unique identifier for the swap
    pub swap_id: [u8; 32],
    /// The current stage of the swap,
    pub stage: SwapStage,
    /// The timestamp of when the status update occurred
    pub at: u64,
}

#[cfg_attr(target_arch = "wasm32", derive(tsify_next::Tsify))]
#[cfg_attr(target_arch = "wasm32", tsify(into_wasm_abi, from_wasm_abi))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CheckedQuote {
    /// The unique identifier for the swap
    pub swap_id: [u8; 32],
    /// The channel from which the user is sending
    pub origin: u8,
    /// The channel to which the user is sending
    pub destination: u8,
    /// The amount the user is sending
    pub amount_in: String,
    /// The amount the user will receive,
    pub amount_out: String,
    /// The address the user needs to send to in order to complete the swap
    /// (its from the LP's perspective)
    pub sender_destination_address: String,
    /// How many extra seconds the user needs to offset their unlock ts in addition to
    /// finality
    pub commit_unlock_offset_secs: u64,
    /// The lp's sender address from the destination channel (from LP perspective)
    pub lp_sender_address: String,
    /// The signature provided by the LP for the swap
    pub lp_signature: Vec<u8>,
    /// The number of block confirmations required for the LP's transaction
    pub lp_block_confirmations: u64,
    /// Indicates whether the signature is valid
    pub signature_valid: bool,
    /// Indicates whether the balance is sufficient for the swap
    pub balance_sufficient: bool,
}

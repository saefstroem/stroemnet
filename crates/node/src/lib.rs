#![cfg_attr(target_arch = "wasm32", allow(clippy::arc_with_non_send_sync))]

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
pub struct ChannelSpec {
    pub config: Value,
    pub lp_private_key: Option<String>,
}

impl ChannelSpec {
    pub fn minimum_block_confirmations(&self) -> u64 {
        self.config
            .get("minimum_block_confirmations")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }
}

pub struct NodeConfig {
    pub handler: HandlerConfig,
    pub channels: AHashMap<ChannelId, ChannelSpec>,
    #[cfg(not(target_arch = "wasm32"))]
    pub bind_addr: Option<SocketAddr>,
    #[cfg(not(target_arch = "wasm32"))]
    pub price_oracle_update_interval_secs: u64,
    pub bootstrap_peers: Vec<String>,
    pub role: NodeRole,
    pub advertised_listen_addr: Option<String>,
    #[cfg(target_arch = "wasm32")]
    pub swap_status_tx: tokio::sync::mpsc::UnboundedSender<SwapStatusUpdate>,
}

#[cfg_attr(target_arch = "wasm32", derive(tsify_next::Tsify))]
#[cfg_attr(target_arch = "wasm32", tsify(into_wasm_abi, from_wasm_abi))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SwapStage {
    AwaitingDeposit {
        #[cfg_attr(
            target_arch = "wasm32",
            tsify(type = "\"KaspaTn10\" | \"EthereumSepolia\" | \"IgraGalleon\"")
        )]
        chain: ChannelId,
        address: String,
        deposit_target: Option<String>,
    },
    CommitSubmitted {
        unlock_ts: u64,
    },
    Locked,
    Completed,
    Failed {
        reason: String,
    },
}

#[cfg_attr(target_arch = "wasm32", derive(tsify_next::Tsify))]
#[cfg_attr(target_arch = "wasm32", tsify(into_wasm_abi, from_wasm_abi))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SwapStatusUpdate {
    pub swap_id: [u8; 32],
    pub stage: SwapStage,
    pub at: u64,
}

#[cfg_attr(target_arch = "wasm32", derive(tsify_next::Tsify))]
#[cfg_attr(target_arch = "wasm32", tsify(into_wasm_abi, from_wasm_abi))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CheckedQuote {
    pub swap_id: [u8; 32],
    pub origin: u8,
    pub destination: u8,
    pub amount_in: String,
    pub amount_out: String,
    pub sender_destination_address: String,
    pub commit_unlock_offset_secs: u64,
    pub lp_sender_address: String,
    pub lp_signature: Vec<u8>,
    pub lp_block_confirmations: u64,
    pub signature_valid: bool,
    pub balance_sufficient: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn channel_spec_reads_min_confirmations_with_default() {
        let with = ChannelSpec {
            config: json!({ "minimum_block_confirmations": 30 }),
            lp_private_key: None,
        };
        assert_eq!(with.minimum_block_confirmations(), 30);
        let without = ChannelSpec {
            config: json!({}),
            lp_private_key: None,
        };
        assert_eq!(without.minimum_block_confirmations(), 0);
    }
}

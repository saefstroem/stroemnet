use serde::Deserialize;

use super::GasPayment;
use super::finality::{DEFAULT_MAX_BLOCKS_PER_POLL, DEFAULT_POLL_INTERVAL_MS};

#[derive(Deserialize)]
/// EVM channel configuration
pub(super) struct EvmConfig {
    /// the RPC url to connec to the evm network
    pub rpc_url: String,
    /// The htlc address, i.e. contract address on this chain
    pub htlc_address: String,
    #[serde(default)]
    /// Minimum number of block confirmations to consider a chain event to be confirmed
    pub minimum_block_confirmations: u64,
    #[serde(default = "default_poll_interval_ms")]
    /// How frequently to poll the RPC for new data
    pub poll_interval_ms: u64,
    #[serde(default = "default_max_blocks_per_poll")]
    /// Maximum amount of blocks to poll per each rpc request
    pub max_blocks_per_poll: u64,
    #[serde(default)]
    /// Whether to participate in CCR, requires gas balance
    pub participate_ccr: bool,
    #[serde(default)]
    /// Whether the network is a legacy or eip1559 network
    pub gas_payment: GasPayment,
}

/// Default poll interval
fn default_poll_interval_ms() -> u64 {
    DEFAULT_POLL_INTERVAL_MS
}

/// Default maximum blocks per poll
fn default_max_blocks_per_poll() -> u64 {
    DEFAULT_MAX_BLOCKS_PER_POLL
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn applies_defaults_when_absent() {
        let cfg: EvmConfig = serde_json::from_value(serde_json::json!({
            "rpc_url": "http://localhost:8545",
            "htlc_address": "0x0000000000000000000000000000000000000000"
        }))
        .unwrap();
        assert_eq!(cfg.poll_interval_ms, DEFAULT_POLL_INTERVAL_MS);
        assert_eq!(cfg.max_blocks_per_poll, DEFAULT_MAX_BLOCKS_PER_POLL);
        assert_eq!(cfg.minimum_block_confirmations, 0);
        assert!(!cfg.participate_ccr);
        assert!(matches!(cfg.gas_payment, GasPayment::Eip1559));
    }

    #[test]
    fn parses_gas_payment_lowercase() {
        let cfg: EvmConfig = serde_json::from_value(serde_json::json!({
            "rpc_url": "u",
            "htlc_address": "a",
            "gas_payment": "legacy"
        }))
        .unwrap();
        assert!(matches!(cfg.gas_payment, GasPayment::Legacy));
    }
}

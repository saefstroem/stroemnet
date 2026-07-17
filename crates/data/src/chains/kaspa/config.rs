use serde::Deserialize;

/// Minimum coinbase maturity
const DEFAULT_COINBASE_MATURITY: u64 = 1000;
/// Minimum block confirmations
const DEFAULT_MINIMUM_BLOCK_CONFIRMATIONS: u64 = 10 * (60 * 10);
/// Amount of time a script is valid for
const DEFAULT_SCRIPT_TTL_SECS: u64 = 4 * 60 * 60;

#[derive(Deserialize)]
/// The kaspa channel config
pub(super) struct KaspaConfig {
    #[serde(default)]
    /// Rpc url to connect to
    pub wrpc_url: Option<String>,
    /// The kaspa specific network id
    pub network_id: String,
    #[serde(default = "default_min_confirmations")]
    /// minimum amount of block confirmations to consider a block finalized
    pub minimum_block_confirmations: u64,
    #[serde(default = "default_coinbase_maturity")]
    /// amount of daa score to wait for miner utxo to be valid
    pub coinbase_maturity: u64,
    #[serde(default = "default_script_ttl_secs")]
    /// how long to keep announced utxo scripts for until they are invalid
    pub script_ttl_secs: u64,
    #[serde(default)]
    /// whether to participate in ccr and earn ccr rewards
    pub participate_ccr: bool,
}

fn default_min_confirmations() -> u64 {
    DEFAULT_MINIMUM_BLOCK_CONFIRMATIONS
}

fn default_coinbase_maturity() -> u64 {
    DEFAULT_COINBASE_MATURITY
}

fn default_script_ttl_secs() -> u64 {
    DEFAULT_SCRIPT_TTL_SECS
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn applies_defaults_when_absent() {
        let cfg: KaspaConfig = serde_json::from_value(serde_json::json!({
            "network_id": "testnet-10"
        }))
        .unwrap();
        assert_eq!(cfg.coinbase_maturity, DEFAULT_COINBASE_MATURITY);
        assert_eq!(
            cfg.minimum_block_confirmations,
            DEFAULT_MINIMUM_BLOCK_CONFIRMATIONS
        );
        assert_eq!(cfg.script_ttl_secs, DEFAULT_SCRIPT_TTL_SECS);
        assert!(!cfg.participate_ccr);
        assert!(cfg.wrpc_url.is_none());
    }

    #[test]
    fn honors_explicit_values() {
        let cfg: KaspaConfig = serde_json::from_value(serde_json::json!({
            "network_id": "mainnet",
            "coinbase_maturity": 7,
            "participate_ccr": true
        }))
        .unwrap();
        assert_eq!(cfg.coinbase_maturity, 7);
        assert!(cfg.participate_ccr);
    }
}

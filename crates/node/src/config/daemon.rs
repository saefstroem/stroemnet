use std::collections::HashMap;
use std::net::SocketAddrV4;

use serde::Deserialize;

#[derive(Deserialize)]
/// Stroemnet node daemon configuration
pub struct DaemonConfig {
    /// Which address to bind to
    pub bind_addr: SocketAddrV4,
    /// How other nodes can contact us
    pub external_hostname: String,
    /// Minimum trade usd optional
    pub min_trade_usd: Option<f64>,
    /// Maximum trade usd optional (only for Lps)
    pub max_trade_usd: Option<f64>,
    /// Spread percent that you as an LP will charge
    pub spread_percent: Option<f64>,
    /// How frequently to update the price from the oracle
    pub price_oracle_update_interval_secs: u64,
    #[serde(default = "default_commit_buffer_secs")]
    /// Perceived network delay to propagate orders
    pub commit_buffer_secs: u64,
    #[serde(default)]
    /// Bootstrap peers to connect to the p2p network
    pub bootstrap_peers: Vec<String>,
    #[serde(default)]
    /// Whether you will act as an LP
    pub lp: bool,
    #[serde(default = "default_peer_db")]
    /// Path to the peer database
    pub peer_db: String,
    #[serde(default)]
    /// Channel specific configurations
    pub channels: HashMap<String, ChannelConfig>,
}

#[derive(Deserialize)]
pub struct ChannelConfig {
    /// Private key for the LP and CCR bot
    pub private_key: Option<String>,
    #[serde(default)]
    /// Whether you will participate in CCR and earn fees from fulfilling swaps
    pub participate_ccr: bool,
    /// Minimum amount of confirmations
    pub min_confirmations: Option<u64>,
    /// The rpc url in order to connect to the node
    pub rpc_url: Option<String>,
    /// The contract address to interact with
    pub htlc_address: Option<String>,
    /// How you plan to pay for gas, relevant for evm networks whether they support eip1559
    pub gas_payment: Option<String>,
    /// Network id used on some channels
    pub network_id: Option<String>,
    /// Whether this channel requires a particular wait time for coinbase transactions to be spend
    pub coinbase_maturity: Option<u64>,
    /// Whether this channel makes use of ttl timeout for storing scripts.
    /// Used for UTXO based channels that use the p2sh pattern
    pub script_ttl_secs: Option<u64>,
}

/// Default propagation time
fn default_commit_buffer_secs() -> u64 {
    960
}

/// Default peer database
fn default_peer_db() -> String {
    "./stroemnet-peers.db".to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn defaults_apply_when_omitted() {
        let cfg: DaemonConfig = toml::from_str(
            "bind_addr = \"0.0.0.0:9000\"\nexternal_hostname = \"wss://x/\"\nprice_oracle_update_interval_secs = 60\n",
        )
        .unwrap();
        assert_eq!(cfg.commit_buffer_secs, 960);
        assert_eq!(cfg.peer_db, "./stroemnet-peers.db");
        assert!(!cfg.lp);
        assert!(cfg.channels.is_empty());
    }
}

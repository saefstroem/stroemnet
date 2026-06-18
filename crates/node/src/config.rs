use std::collections::HashMap;
use std::net::{SocketAddr, SocketAddrV4};
use std::path::Path;

use ahash::AHashMap;
use serde::Deserialize;
use serde_json::json;
use stroemnet_handler::HandlerConfig;
use stroemnet_protocol::ChannelId;

use crate::coordinator::Role;
use crate::error::StroemnetError;
use crate::result::Result;
use crate::{ChannelSpec, NodeConfig};

#[derive(Deserialize)]
/// Stroemnet daemon configuration loaded from a toml file
pub struct DaemonConfig {
    /// Which address we are going to bind to in order to listen
    pub bind_addr: SocketAddrV4,
    /// How other nodes can reach us
    pub external_hostname: String,
    /// Minimum trade value in usd, used to prevent spam and uneconomic trades
    pub min_trade_usd: Option<f64>,
    /// Maximum trade value in usd, used to prevent large trades that may be too risky for the LP
    pub max_trade_usd: Option<f64>,
    /// Percentage to be used for spread, which is how the LP makes money on each swap
    pub spread_percent: Option<f64>,
    /// How often we update price in seconds
    pub price_oracle_update_interval_secs: u64,
    #[serde(default = "default_commit_buffer_secs")]
    /// The number of seconds we require for propagation buffer
    /// which is used to compute the unlock timestamp for a users trades
    pub commit_buffer_secs: u64,
    #[serde(default)]
    /// Bootstrap nodes to connect to the p2p network and discover new nodes,
    pub bootstrap_peers: Vec<String>,
    #[serde(default)]
    /// Whether this node should act as an LP and respond to proposal requests,
    /// or just be an observer that tracks swaps and broadcasts reveals.
    pub lp: bool,
    #[serde(default = "default_peer_db")]
    /// Path to the peer db file, which is used to persist known peers across restarts
    pub peer_db: String,
    #[serde(default)]
    /// Channel-specific configurations, where the key is the channel name (e.g. "kaspa-tn10")
    pub channels: HashMap<String, ChannelConfig>,
}

#[derive(Deserialize)]
pub struct ChannelConfig {
    /// The private key used by the LP for this channel
    pub private_key: Option<String>,
    #[serde(default)]
    /// Whether to participate in competitive claim rescue (CCR) for this channel
    pub participate_ccr: bool,
    /// Minimum amount of block confirmations required
    /// in order to consider this chain events as final
    pub min_confirmations: Option<u64>,
    /// The RPC URL for EVM chains, used to interact with the blockchain
    pub rpc_url: Option<String>,
    /// The HTLC contract address for EVM chains, used to monitor and interact with the contract
    pub htlc_address: Option<String>,
    /// Gas pricing mode for EVM chains: "eip1559" (default) or "legacy" (for chains whose
    /// enforced minimum gas price is decoupled from the base fee, e.g. Igra Galleon)
    pub gas_payment: Option<String>,
    /// The network ID for Kaspa channels, used to connect to the correct network
    pub network_id: Option<String>,
    /// The WRPC URL for Kaspa channels, used to interact with the Kaspa node
    pub wrpc_url: Option<String>,
    /// The coinbase maturity for Kaspa channels, used to determine when mined blocks can be considered final
    pub coinbase_maturity: Option<u64>,
    /// The TTL for the redeem scripts that we monitor on Kaspa, used to determine when a script can be considered expired
    pub script_ttl_secs: Option<u64>,
}

/// Default value for the commit buffer seconds, which is used to compute the unlock timestamp for a users trades
fn default_commit_buffer_secs() -> u64 {
    960
}

/// Default value for the peer db path, which is used to persist known peers across restarts
fn default_peer_db() -> String {
    "./stroemnet-peers.db".to_string()
}

/// Enum representing the kind of blockchain for a channel
enum ChainKind {
    Kaspa,
    Evm,
}

/// The configuration for a single channel, converting a string
/// identifier for channel id
fn channel_id_from_name(name: &str) -> Result<ChannelId> {
    match name {
        "kaspa-tn10" => Ok(ChannelId::KaspaTn10),
        "ethereum-sepolia" => Ok(ChannelId::EthereumSepolia),
        "igra-galleon" => Ok(ChannelId::IgraGalleon),
        other => Err(StroemnetError::Env(format!("unknown channel '{other}'"))),
    }
}

/// Chain kind is to separate between evm and other kind of
/// chain configuration
fn chain_kind(id: ChannelId) -> ChainKind {
    match id {
        ChannelId::KaspaTn10 => ChainKind::Kaspa,
        ChannelId::EthereumSepolia | ChannelId::IgraGalleon => ChainKind::Evm,
    }
}

impl ChannelConfig {
    /// Converts a raw channel configuration into a ChannelSpec
    fn into_spec(self, id: ChannelId, name: &str) -> Result<ChannelSpec> {
        let mut config = match chain_kind(id) {
            ChainKind::Evm => json!({
                "rpc_url": self.rpc_url.ok_or_else(|| StroemnetError::Env(format!("channel '{name}': rpc_url is required")))?,
                "htlc_address": self.htlc_address.ok_or_else(|| StroemnetError::Env(format!("channel '{name}': htlc_address is required")))?,
                "participate_ccr": self.participate_ccr,
            }),
            ChainKind::Kaspa => {
                let mut cfg = json!({
                    "network_id": self.network_id.ok_or_else(|| StroemnetError::Env(format!("channel '{name}': network_id is required")))?,
                    "participate_ccr": self.participate_ccr,
                });
                if let Some(wrpc) = self.wrpc_url {
                    cfg["wrpc_url"] = wrpc.into();
                }
                if let Some(cm) = self.coinbase_maturity {
                    cfg["coinbase_maturity"] = cm.into();
                }
                if let Some(ttl) = self.script_ttl_secs {
                    cfg["script_ttl_secs"] = ttl.into();
                }
                cfg
            }
        };
        if let Some(conf) = self.min_confirmations {
            config["minimum_block_confirmations"] = conf.into();
        }
        if let Some(gp) = self.gas_payment {
            config["gas_payment"] = gp.into();
        }

        Ok(ChannelSpec {
            config,
            lp_private_key: self.private_key,
        })
    }
}

impl DaemonConfig {
    /// Loads the configuration from a toml file at the given path
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| StroemnetError::Env(format!("config read ({}): {e}", path.display())))?;
        toml::from_str(&raw)
            .map_err(|e| StroemnetError::Env(format!("config parse ({}): {e}", path.display())))
    }

    /// Converts the raw daemon configuration into a NodeConfig, which is used to initialize the node
    pub fn into_node_config(self, db_peers: Vec<String>) -> Result<NodeConfig> {
        let mut channels: AHashMap<ChannelId, ChannelSpec> = AHashMap::new();
        // Load the configuration for each channel
        for (name, ch) in self.channels {
            let id = channel_id_from_name(&name)?;
            if self.lp && ch.private_key.is_none() {
                return Err(StroemnetError::Env(format!(
                    "LP mode: channel '{name}' requires private_key"
                )));
            }
            tracing::info!("Loaded config for channel {id}");
            channels.insert(id, ch.into_spec(id, &name)?);
        }
        if channels.is_empty() {
            return Err(StroemnetError::Env("no channels configured".into()));
        }

        let mut bootstrap_peers = self.bootstrap_peers;

        // Merge the bootstrap peers from the config with the peers loaded from the db, avoiding duplicates
        for p in db_peers {
            if !bootstrap_peers.contains(&p) {
                bootstrap_peers.push(p);
            }
        }

        let (min_trade_usd, max_trade_usd, spread_percent) = if self.lp {
            (
                self.min_trade_usd.ok_or_else(|| {
                    StroemnetError::Env("LP mode: min_trade_usd is required".into())
                })?,
                self.max_trade_usd.ok_or_else(|| {
                    StroemnetError::Env("LP mode: max_trade_usd is required".into())
                })?,
                self.spread_percent.ok_or_else(|| {
                    StroemnetError::Env("LP mode: spread_percent is required".into())
                })?,
            )
        } else {
            (
                self.min_trade_usd.unwrap_or(0.0),
                self.max_trade_usd.unwrap_or(0.0),
                self.spread_percent.unwrap_or(0.0),
            )
        };

        Ok(NodeConfig {
            handler: HandlerConfig {
                min_trade_usd,
                max_trade_usd,
                spread_percent,
                commit_buffer_secs: self.commit_buffer_secs,
            },
            channels,
            bind_addr: Some(SocketAddr::V4(self.bind_addr)),
            price_oracle_update_interval_secs: self.price_oracle_update_interval_secs,
            bootstrap_peers,
            role: if self.lp { Role::Lp } else { Role::Observer },
            advertised_listen_addr: Some(self.external_hostname.trim_end_matches('/').to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
bind_addr = "0.0.0.0:9000"
external_hostname = "wss://example.test/"
min_trade_usd = 1.0
max_trade_usd = 100000.0
spread_percent = 0.5
price_oracle_update_interval_secs = 60
lp = true

[channels.kaspa-tn10]
private_key = "deadbeef"
network_id = "testnet-10"

[channels.ethereum-sepolia]
private_key = "0xkey"
rpc_url = "https://rpc.test"
htlc_address = "0xhtlc"
"#;

    #[test]
    fn parses_and_builds_node_config() {
        let cfg: DaemonConfig = toml::from_str(SAMPLE).expect("parse");
        let node = cfg.into_node_config(Vec::new()).expect("into_node_config");

        assert_eq!(node.role, Role::Lp);
        assert_eq!(node.channels.len(), 2);
        assert!(node.channels.contains_key(&ChannelId::KaspaTn10));
        assert!(node.channels.contains_key(&ChannelId::EthereumSepolia));
        assert_eq!(node.handler.commit_buffer_secs, 960);
        assert_eq!(node.price_oracle_update_interval_secs, 60);

        let kas = &node.channels[&ChannelId::KaspaTn10];
        assert_eq!(kas.config["participate_ccr"], json!(false));
        assert_eq!(kas.config["network_id"], json!("testnet-10"));
    }

    #[test]
    fn evm_channel_missing_rpc_url_errors() {
        let raw = r#"
bind_addr = "0.0.0.0:9000"
external_hostname = "wss://x/"
min_trade_usd = 1.0
max_trade_usd = 2.0
spread_percent = 0.5
price_oracle_update_interval_secs = 60

[channels.ethereum-sepolia]
private_key = "k"
htlc_address = "0xhtlc"
"#;
        let cfg: DaemonConfig = toml::from_str(raw).expect("parse");
        let Err(err) = cfg.into_node_config(Vec::new()) else {
            panic!("expected error for missing rpc_url");
        };
        assert!(format!("{err}").contains("rpc_url"));
    }

    #[test]
    fn observer_channel_without_keys_ok() {
        let raw = r#"
bind_addr = "0.0.0.0:9000"
external_hostname = "wss://x/"
min_trade_usd = 1.0
max_trade_usd = 2.0
spread_percent = 0.5
price_oracle_update_interval_secs = 60

[channels.kaspa-tn10]
network_id = "testnet-10"
"#;
        let cfg: DaemonConfig = toml::from_str(raw).expect("parse");
        let node = cfg.into_node_config(Vec::new()).expect("into_node_config");

        assert_eq!(node.role, Role::Observer);
        let kas = &node.channels[&ChannelId::KaspaTn10];
        assert!(kas.lp_private_key.is_none());
    }

    #[test]
    fn lp_channel_without_keys_errors() {
        let raw = r#"
bind_addr = "0.0.0.0:9000"
external_hostname = "wss://x/"
min_trade_usd = 1.0
max_trade_usd = 2.0
spread_percent = 0.5
price_oracle_update_interval_secs = 60
lp = true

[channels.kaspa-tn10]
network_id = "testnet-10"
"#;
        let cfg: DaemonConfig = toml::from_str(raw).expect("parse");
        let Err(err) = cfg.into_node_config(Vec::new()) else {
            panic!("expected error for LP channel missing keys");
        };
        assert!(format!("{err}").contains("requires private_key"));
    }

    #[test]
    fn observer_omits_trade_bounds_ok() {
        let raw = r#"
bind_addr = "0.0.0.0:9000"
external_hostname = "wss://x/"
price_oracle_update_interval_secs = 60

[channels.kaspa-tn10]
network_id = "testnet-10"
"#;
        let cfg: DaemonConfig = toml::from_str(raw).expect("parse");
        let node = cfg.into_node_config(Vec::new()).expect("into_node_config");
        assert_eq!(node.role, Role::Observer);
        assert_eq!(node.handler.min_trade_usd, 0.0);
        assert_eq!(node.handler.max_trade_usd, 0.0);
        assert_eq!(node.handler.spread_percent, 0.0);
    }

    #[test]
    fn lp_missing_trade_bounds_errors() {
        let raw = r#"
bind_addr = "0.0.0.0:9000"
external_hostname = "wss://x/"
price_oracle_update_interval_secs = 60
lp = true

[channels.kaspa-tn10]
private_key = "deadbeef"
network_id = "testnet-10"
"#;
        let cfg: DaemonConfig = toml::from_str(raw).expect("parse");
        let Err(err) = cfg.into_node_config(Vec::new()) else {
            panic!("expected error for LP missing trade bounds");
        };
        assert!(format!("{err}").contains("min_trade_usd"));
    }
}

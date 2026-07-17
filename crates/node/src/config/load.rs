use std::net::SocketAddr;
use std::path::Path;

use ahash::AHashMap;
use stroemnet_handler::HandlerConfig;
use stroemnet_protocol::ChannelId;

use super::daemon::DaemonConfig;
use super::spec::channel_id_from_name;
use crate::coordinator::Role;
use crate::error::StroemnetError;
use crate::result::Result;
use crate::{ChannelSpec, NodeConfig};

impl DaemonConfig {
    /// Loads a daemon configuration from a specified path
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| StroemnetError::Env(format!("config read ({}): {e}", path.display())))?;
        toml::from_str(&raw)
            .map_err(|e| StroemnetError::Env(format!("config parse ({}): {e}", path.display())))
    }

    /// Convert the configuration into node configuration which also converts the general
    /// configuration into channel specific configuration
    pub fn into_node_config(self, db_peers: Vec<String>) -> Result<NodeConfig> {
        // Create ds to store all activated channels
        let mut channels: AHashMap<ChannelId, ChannelSpec> = AHashMap::new();
        for (name, ch) in self.channels {
            // go over all channels
            let id = channel_id_from_name(&name)?;
            if self.lp && ch.private_key.is_none() {
                // lp mode requires private key
                return Err(StroemnetError::Env(format!(
                    "LP mode: channel '{name}' requires private_key"
                )));
            }
            if ch.participate_ccr && ch.private_key.is_none() {
                // ccr mode requires private key with gas
                return Err(StroemnetError::Env(format!(
                    "CCR mode: channel '{name}' requires private_key"
                )));
            }
            if self.lp && !ch.participate_ccr {
                // if you are lp you are by definition ccr as well
                return Err(StroemnetError::Env(format!(
                    "LP mode: channel '{name}' requires participate_ccr = true so the LP claims its own settled legs"
                )));
            }
            // Convert the generic configuration into the channel spec expected by each channel
            channels.insert(id, ch.into_spec(id, &name)?);
        }
        if channels.is_empty() {
            return Err(StroemnetError::Env("no channels configured".into()));
        }
        if self.lp {
            // We need to ensure that the expected propagation time must always be larger than the lock time
            // so that there is enough time to propagate before lock time is considered
            let min_buffer = channels
                .keys()
                .map(|c| c.lock_time_secs())
                .max()
                .unwrap_or(0);
            if self.commit_buffer_secs < min_buffer {
                return Err(StroemnetError::Env(format!(
                    "LP mode: commit_buffer_secs ({}) must be >= {} (max chain lock time among configured channels) to preserve atomic-swap timelock safety",
                    self.commit_buffer_secs, min_buffer
                )));
            }
        }

        // Put all saved peers as bootstrap peers
        let mut bootstrap_peers = self.bootstrap_peers;
        for p in db_peers {
            if !bootstrap_peers.contains(&p) {
                bootstrap_peers.push(p);
            }
        }

        // Ensure the trade specific configurations are configured if LP mode is activated
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
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
    use super::*;
    use serde_json::json;

    fn lp_sample() -> &'static str {
        "bind_addr = \"0.0.0.0:9000\"\nexternal_hostname = \"wss://x/\"\nmin_trade_usd = 1.0\nmax_trade_usd = 2.0\nspread_percent = 0.5\nprice_oracle_update_interval_secs = 60\nlp = true\n\n[channels.kaspa-tn10]\nprivate_key = \"deadbeef\"\nparticipate_ccr = true\nnetwork_id = \"testnet-10\"\n"
    }

    #[test]
    fn parses_and_builds_lp_node_config() {
        let cfg: DaemonConfig = toml::from_str(lp_sample()).unwrap();
        let node = cfg.into_node_config(Vec::new()).unwrap();
        assert_eq!(node.role, Role::Lp);
        assert_eq!(node.channels.len(), 1);
        assert_eq!(node.handler.commit_buffer_secs, 960);
        assert_eq!(
            node.channels[&ChannelId::KaspaTn10].config["network_id"],
            json!("testnet-10")
        );
    }

    #[test]
    fn lp_channel_without_key_errors() {
        let raw = "bind_addr = \"0.0.0.0:9000\"\nexternal_hostname = \"wss://x/\"\nmin_trade_usd = 1.0\nmax_trade_usd = 2.0\nspread_percent = 0.5\nprice_oracle_update_interval_secs = 60\nlp = true\n\n[channels.kaspa-tn10]\nnetwork_id = \"testnet-10\"\n";
        let cfg: DaemonConfig = toml::from_str(raw).unwrap();
        let Err(err) = cfg.into_node_config(Vec::new()) else {
            panic!("expected error");
        };
        assert!(format!("{err}").contains("requires private_key"));
    }

    #[test]
    fn evm_channel_missing_rpc_url_errors() {
        let raw = "bind_addr = \"0.0.0.0:9000\"\nexternal_hostname = \"wss://x/\"\nmin_trade_usd = 1.0\nmax_trade_usd = 2.0\nspread_percent = 0.5\nprice_oracle_update_interval_secs = 60\n\n[channels.ethereum-sepolia]\nprivate_key = \"k\"\nhtlc_address = \"0xhtlc\"\n";
        let cfg: DaemonConfig = toml::from_str(raw).unwrap();
        let Err(err) = cfg.into_node_config(Vec::new()) else {
            panic!("expected error");
        };
        assert!(format!("{err}").contains("rpc_url"));
    }

    #[test]
    fn observer_omits_keys_and_bounds() {
        let raw = "bind_addr = \"0.0.0.0:9000\"\nexternal_hostname = \"wss://x/\"\nprice_oracle_update_interval_secs = 60\n\n[channels.kaspa-tn10]\nnetwork_id = \"testnet-10\"\n";
        let cfg: DaemonConfig = toml::from_str(raw).unwrap();
        let node = cfg.into_node_config(Vec::new()).unwrap();
        assert_eq!(node.role, Role::Observer);
        assert_eq!(node.handler.min_trade_usd, 0.0);
        assert!(
            node.channels[&ChannelId::KaspaTn10]
                .lp_private_key
                .is_none()
        );
    }
}

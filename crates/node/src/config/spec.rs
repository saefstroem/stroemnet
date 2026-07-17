use serde_json::{Value, json};
use stroemnet_protocol::ChannelId;

use super::daemon::ChannelConfig;
use crate::ChannelSpec;
use crate::error::StroemnetError;
use crate::result::Result;

/// Insert a value if the target is an object with a key and a value
fn insert_opt(target: &mut Value, key: &str, value: Option<Value>) {
    // we only insert if this is an object
    if let (Some(map), Some(value)) = (target.as_object_mut(), value) {
        map.insert(key.to_string(), value);
    }
}

/// Concrete defined channel types
/// These are networks which have corresponding data sinks
/// Could theoretically be handled via traits but we are not passing
/// data sinks in this module so having this just makes it simpler
enum ChainKind {
    Kaspa,
    Evm,
}

/// Converts a name into a channel id
pub(super) fn channel_id_from_name(name: &str) -> Result<ChannelId> {
    match name {
        "kaspa-tn10" => Ok(ChannelId::KaspaTn10),
        "ethereum-sepolia" => Ok(ChannelId::EthereumSepolia),
        "igra-galleon" => Ok(ChannelId::IgraGalleon),
        other => Err(StroemnetError::Env(format!("unknown channel '{other}'"))),
    }
}

/// Matches a channel to a chain type
fn chain_kind(id: ChannelId) -> ChainKind {
    match id {
        ChannelId::KaspaTn10 => ChainKind::Kaspa,
        ChannelId::EthereumSepolia | ChannelId::IgraGalleon => ChainKind::Evm,
    }
}

impl ChannelConfig {
    pub(super) fn into_spec(self, id: ChannelId, name: &str) -> Result<ChannelSpec> {
        let mut config = match chain_kind(id) {
            //Create the relevant channel configuration depending on which
            // chain we are working on
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
                insert_opt(&mut cfg, "wrpc_url", self.rpc_url.map(Value::from));
                insert_opt(
                    &mut cfg,
                    "coinbase_maturity",
                    self.coinbase_maturity.map(Value::from),
                );
                insert_opt(
                    &mut cfg,
                    "script_ttl_secs",
                    self.script_ttl_secs.map(Value::from),
                );
                cfg
            }
        };

        // We always enforce minimum confirmations to be greater than 0
        if self.min_confirmations.unwrap_or(0) == 0 {
            return Err(StroemnetError::Env(format!(
                "channel '{name}': min_confirmations must be set to a non-zero value for EVM channels (reorg safety)"
            )));
        }
        insert_opt(
            &mut config,
            "minimum_block_confirmations",
            self.min_confirmations.map(Value::from),
        );
        insert_opt(
            &mut config,
            "gas_payment",
            self.gas_payment.map(Value::from),
        );
        Ok(ChannelSpec {
            config,
            lp_private_key: self.private_key,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn channel_id_from_name_maps_known_and_rejects_unknown() {
        assert_eq!(
            channel_id_from_name("kaspa-tn10").unwrap(),
            ChannelId::KaspaTn10
        );
        assert_eq!(
            channel_id_from_name("igra-galleon").unwrap(),
            ChannelId::IgraGalleon
        );
        assert!(channel_id_from_name("nope").is_err());
    }
}

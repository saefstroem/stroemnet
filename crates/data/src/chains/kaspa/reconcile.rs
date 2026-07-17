use kaspa_addresses::Address;
use kaspa_rpc_core::api::rpc::RpcApi;

use super::Kaspa;
use super::signing;
use crate::chains::net::retry_timed;
use crate::chains::settlement::{ActionKey, Observation};

impl Kaspa {
    /// Take an action key and compute at which state a swap exists
    pub(super) async fn observe_onchain(&self, key: ActionKey) -> Observation {
        // Try to get the commitment
        let Some(commitment) = self.commitment(&key.swap_id) else {
            return Observation::Unknown;
        };

        // Try to get the p2sh address and redeem script
        let Ok((address, _redeem)) = signing::p2sh_components(&self.network_id, &commitment) else {
            return Observation::Unknown;
        };

        // Try to compute the address
        let Ok(addr) = Address::try_from(address.as_str()) else {
            return Observation::Unknown;
        };
        let client = self.client.clone();
        let utxos = match retry_timed("observe utxos", || {
            client.get_utxos_by_addresses(vec![addr.clone()])
        })
        .await
        {
            Some(u) => u,
            None => return Observation::Unknown,
        };
        if utxos.is_empty() {
            Observation::Unknown
        } else {
            // if there are utxos then it means its not settled
            Observation::NotSettled
        }
        // But we can never confirm its settled here as kaspa is a utxo based platform
    }
}

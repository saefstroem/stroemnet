use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::CommitmentV1;

use super::Kaspa;
use crate::{DataError, Result, UtxoScript};

impl Kaspa {
    /// Return the private key
    pub(super) fn key(&self) -> Result<&str> {
        self.private_key
            .as_deref()
            .ok_or(DataError::MissingKey(self.channel_id))
    }

    /// Cache a commitment
    pub(super) fn cache_commitment(&self, commitment: &CommitmentV1) {
        self.commitments
            .lock()
            .insert(commitment.swap_id, commitment.clone());
    }

    /// Retrieve a commitment by swap id
    pub(super) fn commitment(&self, swap_id: &[u8; 32]) -> Option<CommitmentV1> {
        self.commitments.lock().get(swap_id).cloned()
    }

    /// Prune announced scripts in according with the script ttl policy
    pub(super) async fn prune_scripts(&self) {
        let now = now_unix_secs();
        let ttl = self.script_ttl_secs;
        let mut scripts = self.utxo_scripts.write().await;
        scripts.retain(|_, s| now <= s.unlock_ts.saturating_add(ttl));
    }

    /// Register a script announcement internally
    pub(super) async fn register_internal(&self, address: String, script: UtxoScript) {
        self.utxo_scripts.write().await.insert(address, script);
    }
}

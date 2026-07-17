use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{ChainEvent, CommitmentV1};

use super::Kaspa;
use super::signing;
#[cfg(not(target_arch = "wasm32"))]
use crate::TaskFut;
use crate::{
    BufFut, ChainDataBuffer, DataError, ProposalVerification, Result, ScriptAnnouncement,
    UtxoScriptDetector,
};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

impl ChainDataBuffer for Kaspa {
    /// Compute the lp addressa from the private key
    fn lp_address(&self) -> Result<String> {
        Ok(signing::lp_address_from_private_key(
            &self.network_id,
            self.key()?,
        )?)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Retrieve the settler task which settles claims and refunds
    fn settler_task(self: Arc<Self>) -> Option<TaskFut> {
        let metrics = self.metrics.clone();
        Some(crate::chains::settlement::settler_loop(self, metrics))
    }

    /// Compute the deposit address for some commitment
    fn derive_deposit(&self, commitment: &CommitmentV1) -> Result<(String, Vec<u8>)> {
        Ok(signing::p2sh_components(&self.network_id, commitment)?)
    }

    /// Retrieve the next chunk of finalized confirmed events from the chain
    fn finalized_chunk(&self) -> BufFut<'_, Vec<(ChannelId, ChainEvent)>> {
        Box::pin(self.poll_finalized())
    }

    /// Broadcast a chain event across the kaspa network
    fn broadcast_event<'a>(&'a self, event: &'a ChainEvent) -> BufFut<'a, ()> {
        Box::pin(self.emit_event(event))
    }

    /// Sign a message whilst also requiring a minimum amount of balance
    fn sign_message<'a>(
        &'a self,
        digest: [u8; 32],
        required_balance: &'a str,
    ) -> BufFut<'a, (String, Vec<u8>)> {
        Box::pin(async move {
            let required: u64 = required_balance
                .parse()
                .map_err(|e| DataError::Sign(format!("required_balance: {e}")))?;
            signing::sign_message(
                &self.client,
                &self.network_id,
                self.key()?,
                digest,
                required,
            )
            .await
            .map_err(DataError::from)
        })
    }

    /// Verifies a message signature whilst also requiring a minimum amount of balance
    /// in order to fulfill the swap.
    fn verify_message<'a>(
        &'a self,
        digest: [u8; 32],
        claimed_address: &'a str,
        signature: &'a [u8],
        required_balance: &'a str,
    ) -> BufFut<'a, ProposalVerification> {
        Box::pin(async move {
            let required: u64 = required_balance
                .parse()
                .map_err(|e| DataError::Sign(format!("required_balance: {e}")))?;
            signing::verify_message(&self.client, digest, claimed_address, signature, required)
                .await
                .map_err(DataError::from)
        })
    }

    /// Retrieve the utxo script detector
    fn utxo_script_detector(&self) -> Option<&dyn UtxoScriptDetector> {
        Some(self)
    }

    /// Retrieve the utxo script announcements
    fn take_utxo_script_announcements(&self) -> Vec<ScriptAnnouncement> {
        std::mem::take(&mut self.announcements.lock())
    }
}

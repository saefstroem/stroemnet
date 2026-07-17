use alloy::primitives::U256;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::ChainEvent;

use super::Evm;
use super::signing;
#[cfg(not(target_arch = "wasm32"))]
use crate::TaskFut;
use crate::{BufFut, ChainDataBuffer, DataError, ProposalVerification, Result};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

impl ChainDataBuffer for Evm {
    /// Retrieve the chain specific address for this LP
    fn lp_address(&self) -> Result<String> {
        // Derive the public key from the private key
        let pk = self
            .private_key
            .as_deref()
            .ok_or(DataError::MissingKey(self.channel_id))?;

        // Simply return the address from the private key
        signing::address_from_private_key(pk)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Returns the future task which represents the function that helps settle
    /// pending actions, such as refunds or claims
    fn settler_task(self: Arc<Self>) -> Option<TaskFut> {
        let metrics = self.metrics.clone();
        Some(crate::chains::settlement::settler_loop(self, metrics))
    }

    /// Retrieve the next chunk of finalized events for this channel
    /// Stroemnet works in a cursor based fashion, not via subscriptions, for robustness.
    fn finalized_chunk(&self) -> BufFut<'_, Vec<(ChannelId, ChainEvent)>> {
        Box::pin(self.poll_finalized())
    }

    /// Retrieve the current onchain timestamp ensuring that it is below
    /// Some maximum age, which is for now the polling interval *3 but a maximum
    /// of 30 seconds.
    fn chain_now(&self) -> Option<u64> {
        let max_age = self.poll_interval_secs.saturating_mul(3).max(30);
        let (ts, observed) = self.state.lock().last_block_ts?;
        if now_unix_secs().saturating_sub(observed) > max_age {
            return None;
        }
        Some(ts)
    }

    /// Broadcast an event across the channel, these are commitments, refunds, claims
    fn broadcast_event<'a>(&'a self, event: &'a ChainEvent) -> BufFut<'a, ()> {
        Box::pin(self.emit_event(event))
    }

    /// Signs a message with this configured channel
    /// Used for proving the validity of your quotes and ensuring that you indeed
    /// have enough balance to cover the swap
    fn sign_message<'a>(
        &'a self,
        digest: [u8; 32],          // the digest of the swap
        required_balance: &'a str, // the minimum required balance
    ) -> BufFut<'a, (String, Vec<u8>)> {
        Box::pin(async move {
            let pk = self
                .private_key
                .as_deref()
                .ok_or(DataError::MissingKey(self.channel_id))?;
            let required = U256::from_str_radix(required_balance, 10)
                .map_err(|e| DataError::Sign(format!("required_balance: {e}")))?;

            // Sign the message and prove you have enough balance too
            signing::sign_message(&self.read_provider, pk, digest, required).await
        })
    }

    /// Verifies a message for other components, checking their signature
    /// and that they have enough balance to fulfill the swap.
    fn verify_message<'a>(
        &'a self,
        digest: [u8; 32],          // the digest of the message
        claimed_address: &'a str,  // which address they are claiming to be
        signature: &'a [u8],       // signature for the digest
        required_balance: &'a str, // minimum required balance to fulfill this swap
    ) -> BufFut<'a, ProposalVerification> {
        Box::pin(async move {
            let required = U256::from_str_radix(required_balance, 10)
                .map_err(|e| DataError::Sign(format!("required_balance: {e}")))?;

            // Verify the message and ensure the balance is satisfied
            signing::verify_message(
                &self.read_provider,
                digest,
                claimed_address,
                signature,
                required,
            )
            .await
        })
    }
}

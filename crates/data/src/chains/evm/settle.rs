use super::Evm;
use super::broadcast;
use super::provider;
use crate::chains::settlement::{ActionKey, SettleOutcome};

impl Evm {
    /// Attempts to settle a refund onchain on EVM
    pub(super) async fn settle_refund(&self, key: ActionKey) -> SettleOutcome {
        // Retrieve the swap id
        let swap_id = key.swap_id;

        // Ensure that we have a signed provider that can sign transactions
        let Ok(signed) = self.signed() else {
            return SettleOutcome::Fatal("missing key".into());
        };

        // Retrieve the unlock timestamp for the swap id
        let Some(unlock) = self
            .state
            .lock()
            .pending_refunds
            .iter()
            .find(|(r, _)| r.swap_id == swap_id)
            .map(|(_, ts)| *ts)
        else {
            return SettleOutcome::Retry("no_refund_intent");
        };

        // Read the onchain timestamp and ensure that the swap can be refunded
        match provider::current_block_timestamp(&self.read_provider).await {
            Some(block_ts) if block_ts < unlock => return SettleOutcome::Retry("not_yet_unlocked"),
            None => return SettleOutcome::Retry("block_ts_timeout"),
            _ => {}
        }

        // Compute the nonce to use and gas
        // Initially this is zeroed so its safe to use
        // i.e. its not just a replacement but also initialization
        let Some((nonce, gas)) = self.replacement(key).await else {
            return SettleOutcome::Retry("replacement_unavailable");
        };

        // Try to submit the refund across the chain
        match broadcast::submit_refund(
            signed,
            self.htlc_address,
            swap_id,
            nonce,
            gas,
            self.gas_payment,
        )
        .await
        {
            Ok(()) => SettleOutcome::Retry("submitted_awaiting_inclusion"),
            Err(e) => {
                tracing::warn!(target: "settlement", "evm refund broadcast: {e}");
                SettleOutcome::Retry("broadcast_error")
            }
        }
    }

    /// Attempt to settle a swap by claiming it
    pub(super) async fn settle_claim(&self, key: ActionKey) -> SettleOutcome {
        // Retrieve the swap id
        let swap_id = key.swap_id;

        // Ensure we have a signed provider that can sign onchain transactions
        let Ok(signed) = self.signed() else {
            return SettleOutcome::Fatal("missing key".into());
        };

        // Retrieve the reveal v1 which contains the secret needed in order to
        // unlock the swap
        let Some(reveal) = self
            .state
            .lock()
            .pending_claims
            .iter()
            .find(|c| c.swap_id == swap_id)
            .cloned()
        else {
            return SettleOutcome::Retry("no_reveal");
        };

        // Compute the nonce and gas to use for this attempt
        let Some((nonce, gas)) = self.replacement(key).await else {
            return SettleOutcome::Retry("replacement_unavailable");
        };

        // Submit the claim across the chain
        match broadcast::submit_claim(
            signed,
            self.htlc_address,
            &reveal,
            nonce,
            gas,
            self.gas_payment,
        )
        .await
        {
            Ok(()) => SettleOutcome::Retry("submitted_awaiting_inclusion"),
            Err(e) => {
                tracing::warn!(target: "settlement", "evm claim broadcast: {e}");
                SettleOutcome::Retry("broadcast_error")
            }
        }
    }
}

use kaspa_rpc_core::api::rpc::RpcApi;

use super::Kaspa;
use super::broadcast;
use crate::chains::net::retry_timed;
use crate::chains::settlement::SettleOutcome;

impl Kaspa {
    /// Settles a refund based on a swap id and returns a settlement outcome
    pub(super) async fn settle_refund(&self, swap_id: [u8; 32]) -> SettleOutcome {
        // Retrieve the commitment and the private key needed in order to sign for the transaction
        let (Some(commitment), Ok(pk)) = (self.commitment(&swap_id), self.key()) else {
            return SettleOutcome::Fatal("missing commitment or key".into());
        };

        // Retrieve dag info
        let client = self.client.clone();
        let pmt = match retry_timed("settle dag_info", || client.get_block_dag_info()).await {
            Some(info) => info.past_median_time,
            None => return SettleOutcome::Retry("dag_info_timeout"),
        };

        // Check if the past median time has passed the unlock timestamp
        if pmt <= commitment.unlock_ts.saturating_mul(1000) {
            return SettleOutcome::Retry("not_yet_unlocked");
        }

        // If its unlocked we can try and claim it
        match broadcast::submit_refund(&self.client, pk, self.coinbase_maturity, &commitment).await
        {
            Ok(_) => SettleOutcome::Retry("submitted_awaiting_inclusion"),
            Err(e) => {
                tracing::warn!(target: "settlement", "kaspa refund broadcast: {e}");
                SettleOutcome::Retry("broadcast_error")
            }
        }
    }

    /// Settles a claim based on a swap id
    pub(super) async fn settle_claim(&self, swap_id: [u8; 32]) -> SettleOutcome {
        // Retrieve the commitment and the private key that we will use to sign for the tx
        let (Some(commitment), Ok(pk)) = (self.commitment(&swap_id), self.key()) else {
            return SettleOutcome::Fatal("missing commitment or key".into());
        };

        // Check if we have a reveal v1 struct so that we can submit the reveal ourselves
        let Some(reveal) = self
            .pending_claims
            .lock()
            .iter()
            .find(|c| c.swap_id == swap_id)
            .cloned()
        else {
            return SettleOutcome::Retry("no_reveal");
        };
        // Now that we have the details we can submit the reveal onchain
        match broadcast::submit_reveal(
            &self.client,
            pk,
            self.coinbase_maturity,
            &commitment,
            &reveal,
        )
        .await
        {
            Ok(()) => SettleOutcome::Retry("submitted_awaiting_inclusion"),
            Err(e) => {
                tracing::warn!(target: "settlement", "kaspa claim broadcast: {e}");
                SettleOutcome::Retry("broadcast_error")
            }
        }
    }
}

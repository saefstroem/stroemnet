use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::ChainEvent;

use super::Evm;
use super::broadcast;
use crate::Result;
use crate::chains::settlement::ActionKey;

impl Evm {
    /// Emit the chain event across the current evm entwork
    pub(super) async fn emit_event<'a>(&'a self, event: &'a ChainEvent) -> Result<()> {
        match event {
            // forward to the commitment submitted
            ChainEvent::Commitment(c) => {
                broadcast::submit_commitment(self.signed()?, self.htlc_address, c, self.gas_payment)
                    .await
            }
            ChainEvent::Reveal(r) => {
                // We only transmit reveals if we participate in CCR
                if self.participate_ccr {
                    // Add this as a pending claim, we cannot add it earlier since we cannot
                    // predetermine when a claim is pending
                    super::super::push_pending_claim(&mut self.state.lock().pending_claims, r);
                    // Add a note that we have attempted to claim
                    self.queue
                        .ensure(ActionKey::claim(r.swap_id), now_unix_secs());

                    // Sync the claim status for this swap id to disk
                    self.persist_swap(r.swap_id);
                }
                Ok(())
            }
            // Refunds are handled elsewhere in the code and are reactions to
            // commitments that we see onchain. So we do not handle them here.
            ChainEvent::Refund(_) => Ok(()),
        }
    }
}

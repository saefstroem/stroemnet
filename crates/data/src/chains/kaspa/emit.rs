use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::ChainEvent;

use super::Kaspa;
use super::broadcast;
use crate::chains::settlement::ActionKey;
use crate::{Result, ScriptAnnouncement, UtxoScript};

impl Kaspa {
    /// Emits an event based on the request that we should emit a chain event
    pub(super) async fn emit_event<'a>(&'a self, event: &'a ChainEvent) -> Result<()> {
        match event {
            ChainEvent::Commitment(c) => {
                // Cache the commitment, as we need to unlock it later
                self.cache_commitment(c);

                // Submit the commitment across kaspa network
                let announce = broadcast::submit_commitment(
                    &self.client,
                    self.key()?,
                    self.coinbase_maturity,
                    c,
                )
                .await?;

                // Since we were the ones creating this script we can populate it with us internally
                let script = UtxoScript {
                    redeem_script: announce.redeem_script,
                    unlock_ts: c.unlock_ts,
                    deposit_target: c.amount.value.clone(),
                };
                self.scripts.lock().insert(c.swap_id, script.clone());
                self.register_internal(announce.address.clone(), script.clone())
                    .await;

                // Announce the script
                self.announcements.lock().push(ScriptAnnouncement {
                    address: announce.address,
                    swap_id: c.swap_id,
                    script,
                });
                self.persist_swap(c.swap_id);

                // Other peers will discover it via their respective connections
                Ok(())
            }
            ChainEvent::Reveal(r) => {
                // If the node participates in ccr and whether we have a commitment for this swap
                if self.participate_ccr && self.commitment(&r.swap_id).is_some() {
                    // We push a pending claim to the pending DS
                    super::super::push_pending_claim(&mut self.pending_claims.lock(), r);
                    // then we register an attempt to claim now in the queue, the settler should pick this up
                    self.queue
                        .ensure(ActionKey::claim(r.swap_id), now_unix_secs());

                    // persist any swap changes to disk
                    self.persist_swap(r.swap_id);
                }
                Ok(())
            }
            ChainEvent::Refund(_) => Ok(()),
        }
    }
}

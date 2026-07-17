use alloy::providers::DynProvider;
use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::ChainEvent;

use super::Evm;
use crate::chains::record::encode;
use crate::chains::settlement::ActionKey;
use crate::{DataError, PersistedSwap, Result};

impl Evm {
    /// Retrieve the signer provider for this evm channel
    pub(super) fn signed(&self) -> Result<&DynProvider> {
        self.signed_provider
            .as_ref()
            .ok_or(DataError::MissingKey(self.channel_id))
    }

    /// Persist the state of a swap to disk
    pub(super) fn persist_swap(&self, swap_id: [u8; 32]) {
        // Retrieve the swap store or simply exit
        let Some(store) = &self.swap_store else {
            return;
        };

        // Create a persisted swap record
        let record = {
            let st = self.state.lock();
            PersistedSwap {
                script: None,
                pending_refund: st
                    .pending_refunds
                    .iter()
                    .find(|(r, _)| r.swap_id == swap_id)
                    .map(|(r, ts)| (r.clone(), *ts)),
                pending_claim: st
                    .pending_claims
                    .iter()
                    .find(|c| c.swap_id == swap_id)
                    .cloned(),
                claim_attempt: self.queue.get(ActionKey::claim(swap_id)),
                refund_attempt: self.queue.get(ActionKey::refund(swap_id)),
            }
        };
        if record.is_empty() {
            store.delete(self.channel_id, swap_id);
        } else {
            match encode(&record) {
                Ok(bytes) => store.save(self.channel_id, swap_id, &bytes),
                Err(e) => tracing::error!(
                    target: "settlement",
                    "EVM persist swap {} encode failed: {e}",
                    hex::encode(swap_id)
                ),
            }
        }
    }

    pub(super) fn track_actionable_event(&self, event: &ChainEvent) {
        let swap_id = super::super::event_swap_id(event);
        {
            let mut st = self.state.lock();
            super::super::queue_dequeue_refund_event(
                &mut st.pending_refunds,
                event,
                self.participate_ccr,
            );
            if !matches!(event, ChainEvent::Commitment(_)) {
                st.pending_claims.retain(|c| c.swap_id != swap_id);
            }
        }
        if matches!(event, ChainEvent::Commitment(_)) {
            if self.participate_ccr {
                self.queue
                    .ensure(ActionKey::refund(swap_id), now_unix_secs());
            }
        } else {
            self.queue.record_success(ActionKey::claim(swap_id));
            self.queue.record_success(ActionKey::refund(swap_id));
        }
        self.persist_swap(swap_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stroemnet_protocol::v1::{RefundV1, RevealV1};

    #[test]
    fn event_swap_id_reads_each_variant() {
        assert_eq!(
            crate::chains::event_swap_id(&ChainEvent::Reveal(RevealV1::new([3u8; 32], [0u8; 32]))),
            [3u8; 32]
        );
        assert_eq!(
            crate::chains::event_swap_id(&ChainEvent::Refund(RefundV1::new([4u8; 32]))),
            [4u8; 32]
        );
    }
}

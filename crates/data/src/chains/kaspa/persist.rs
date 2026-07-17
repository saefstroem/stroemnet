use stroemnet_protocol::v1::ChainEvent;

use super::Kaspa;
use crate::PersistedSwap;
use crate::chains::record::encode;
use crate::chains::settlement::ActionKey;

/// Whether a chain event is a resolving type of event
fn is_resolving(event: &ChainEvent) -> bool {
    matches!(event, ChainEvent::Reveal(_) | ChainEvent::Refund(_))
}

impl Kaspa {
    pub(super) fn persist_swap(&self, swap_id: [u8; 32]) {
        // We only persist a swap if we have a configured swap store
        let Some(store) = &self.swap_store else {
            return;
        };
        // Find any kind of pending refund
        let pending_refund = self
            .pending_refunds
            .lock()
            .iter()
            .find(|(r, _)| r.swap_id == swap_id)
            .map(|(r, ts)| (r.clone(), *ts));

        // Pending claim?
        let pending_claim = self
            .pending_claims
            .lock()
            .iter()
            .find(|c| c.swap_id == swap_id)
            .cloned();

        // Retrieve the potential registered utxo script
        let script = self.scripts.lock().get(&swap_id).cloned();
        // Create a record for the swap
        let record = PersistedSwap {
            script,
            pending_refund,
            pending_claim,
            claim_attempt: self.queue.get(ActionKey::claim(swap_id)),
            refund_attempt: self.queue.get(ActionKey::refund(swap_id)),
        };

        // If the record is fully empty it can mean that the swap is settled and we should remove it
        if record.is_empty() {
            store.delete(self.channel_id, swap_id);
        } else {
            // Otherwise we serialize it and store it to disk
            match encode(&record) {
                Ok(bytes) => store.save(self.channel_id, swap_id, &bytes),
                Err(e) => tracing::error!(
                    target: "settlement",
                    "kaspa persist swap {} encode failed: {e}",
                    hex::encode(swap_id)
                ),
            }
        }
    }

    /// Take an event and track it in the relevant trackers
    pub(super) fn track_actionable_event(&self, event: &ChainEvent) {
        // Get the swap id from an event
        let swap_id = super::super::event_swap_id(event);
        if let ChainEvent::Commitment(c) = event {
            // If its a commitment we need to cache to react when its time to unlock
            self.cache_commitment(c);
        }
        // Queue the refund
        super::super::queue_dequeue_refund_event(
            &mut self.pending_refunds.lock(),
            event,
            self.participate_ccr,
        );
        // If its a resolving event
        if is_resolving(event) {
            // It means we need to remove and untrack this event from respective trackers
            self.queue.record_success(ActionKey::claim(swap_id));
            self.queue.record_success(ActionKey::refund(swap_id));
            self.pending_claims.lock().retain(|c| c.swap_id != swap_id);
            self.commitments.lock().remove(&swap_id);
            self.scripts.lock().remove(&swap_id);
        }
        // Then as usual sync it to disk
        self.persist_swap(swap_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stroemnet_protocol::v1::{RefundV1, RevealV1};

    #[test]
    fn event_swap_id_and_resolving_classification() {
        let reveal = ChainEvent::Reveal(RevealV1 {
            swap_id: [1u8; 32],
            secret: [0u8; 32],
        });
        let refund = ChainEvent::Refund(RefundV1 { swap_id: [2u8; 32] });
        assert_eq!(crate::chains::event_swap_id(&reveal), [1u8; 32]);
        assert_eq!(crate::chains::event_swap_id(&refund), [2u8; 32]);
        assert!(is_resolving(&reveal));
        assert!(is_resolving(&refund));
    }
}

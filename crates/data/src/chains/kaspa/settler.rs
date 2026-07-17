use super::Kaspa;
use crate::chains::settlement::{
    Action, ActionKey, Observation, SettleFut, SettleOutcome, Settler,
};

/// Compute a random u64 in order to shift the retry a little bit
fn jitter(key: ActionKey) -> u64 {
    key.swap_id.iter().map(|b| u64::from(*b)).sum()
}

impl Settler for Kaspa {
    /// Retrieve the action keys that are due now
    fn due_now(&self, now: u64) -> Vec<ActionKey> {
        self.queue.due_now(now)
    }

    /// Settle an achtion key based on the type of action
    fn settle(&self, key: ActionKey) -> SettleFut<'_, SettleOutcome> {
        Box::pin(async move {
            match key.action {
                Action::Refund => self.settle_refund(key.swap_id).await,
                Action::Claim => self.settle_claim(key.swap_id).await,
            }
        })
    }

    /// Compute observation based on action key
    fn observe(&self, key: ActionKey) -> SettleFut<'_, Observation> {
        Box::pin(async move { self.observe_onchain(key).await })
    }

    /// Marks this key as completed and settle which means we should remove it
    /// from pending claims and refunds
    fn record_success(&self, key: ActionKey) {
        self.queue.record_success(key);
        match key.action {
            Action::Claim => self
                .pending_claims
                .lock()
                .retain(|c| c.swap_id != key.swap_id),
            Action::Refund => self
                .pending_refunds
                .lock()
                .retain(|(r, _)| r.swap_id != key.swap_id),
        }
        // Sync to disk
        self.persist_swap(key.swap_id);
    }

    /// Record failure which will also retry the key in some jitter
    fn record_failure(&self, key: ActionKey, now: u64) {
        self.queue.record_failure(key, now, jitter(key));
        self.persist_swap(key.swap_id);
    }

    /// Check if an action key is stuck
    fn is_stuck(&self, key: ActionKey, now: u64) -> bool {
        self.queue.is_stuck(key, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_is_deterministic_sum_of_swap_id_bytes() {
        assert_eq!(jitter(ActionKey::claim([1u8; 32])), 32);
        assert_eq!(jitter(ActionKey::refund([0u8; 32])), 0);
    }
}

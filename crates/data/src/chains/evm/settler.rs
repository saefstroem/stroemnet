use super::Evm;
use crate::chains::settlement::{
    Action, ActionKey, Observation, SettleFut, SettleOutcome, Settler,
};

fn jitter(key: ActionKey) -> u64 {
    key.swap_id.iter().map(|b| u64::from(*b)).sum()
}

impl Settler for Evm {
    /// Compute which swaps are due for an action right now
    fn due_now(&self, now: u64) -> Vec<ActionKey> {
        self.queue.due_now(now)
    }

    /// Execute settlement of a concrete swap (action key)
    fn settle(&self, key: ActionKey) -> SettleFut<'_, SettleOutcome> {
        Box::pin(async move {
            match key.action {
                Action::Refund => self.settle_refund(key).await,
                Action::Claim => self.settle_claim(key).await,
            }
        })
    }

    /// Observe a swap and see what is its current state
    fn observe(&self, key: ActionKey) -> SettleFut<'_, Observation> {
        Box::pin(async move { self.observe_onchain(key).await })
    }

    /// Record the settlement of a swap
    fn record_success(&self, key: ActionKey) {
        // Remove the swap from the queue
        self.queue.record_success(key);
        {
            let mut st = self.state.lock();
            // Remove the swap from pending claims or refunds depending on what it was representing
            match key.action {
                Action::Claim => st.pending_claims.retain(|c| c.swap_id != key.swap_id),
                Action::Refund => st.pending_refunds.retain(|(r, _)| r.swap_id != key.swap_id),
            }
        }
        // Persist swap state to disk
        self.persist_swap(key.swap_id);
    }

    /// Record that a swap has failed at the current time
    fn record_failure(&self, key: ActionKey, now: u64) {
        self.queue.record_failure(key, now, jitter(key));
        self.persist_swap(key.swap_id);
    }

    /// Check whether a particular swap is stuck in its settlement
    fn is_stuck(&self, key: ActionKey, now: u64) -> bool {
        self.queue.is_stuck(key, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_is_deterministic_sum_of_swap_id_bytes() {
        assert_eq!(jitter(ActionKey::claim([2u8; 32])), 64);
        assert_eq!(jitter(ActionKey::refund([0u8; 32])), 0);
    }
}

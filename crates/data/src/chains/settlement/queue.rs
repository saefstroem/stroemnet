use ahash::AHashMap;
use parking_lot::Mutex;

use super::action::ActionKey;
use crate::AttemptState;
use crate::chains::record::RestoredSwaps;

/// Seeds the retry queue with restored swaps
pub(crate) fn seed_queue(restored: &RestoredSwaps, now: u64) -> RetryQueue {
    let queue = RetryQueue::default();
    for (sid, st) in &restored.claim_attempts {
        queue.seed(ActionKey::claim(*sid), *st);
    }
    for (sid, st) in &restored.refund_attempts {
        queue.seed(ActionKey::refund(*sid), *st);
    }
    for c in &restored.pending_claims {
        queue.ensure(ActionKey::claim(c.swap_id), now);
    }
    for (r, _) in &restored.pending_refunds {
        queue.ensure(ActionKey::refund(r.swap_id), now);
    }
    queue
}

#[derive(Default)]
/// A retry queue that periodically retries each action key
pub(crate) struct RetryQueue {
    attempts: Mutex<AHashMap<ActionKey, AttemptState>>,
}

impl RetryQueue {
    /// Seed the key with attempt state
    pub(crate) fn seed(&self, key: ActionKey, state: AttemptState) {
        self.attempts.lock().insert(key, state);
    }

    /// Ensure that an action key is present in the queue
    pub(crate) fn ensure(&self, key: ActionKey, now: u64) {
        self.attempts
            .lock()
            .entry(key)
            .or_insert_with(|| AttemptState::new(now));
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Get all action keys that are due now
    pub(crate) fn due_now(&self, now: u64) -> Vec<ActionKey> {
        self.attempts
            .lock()
            .iter()
            .filter(|(_, s)| s.due(now))
            .map(|(k, _)| *k)
            .collect()
    }

    /// Mark an action key as settled and remove it from the queue/
    pub(crate) fn record_success(&self, key: ActionKey) {
        self.attempts.lock().remove(&key);
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Record failure and reschedule the key at a more random retry time
    pub(crate) fn record_failure(&self, key: ActionKey, now: u64, jitter: u64) {
        if let Some(s) = self.attempts.lock().get_mut(&key) {
            s.on_failure(now, jitter);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Set the nonce, we will retry with this nonce but a higher gas price in the next RBF attempt
    pub(crate) fn set_nonce(&self, key: ActionKey, nonce: u64) {
        if let Some(s) = self.attempts.lock().get_mut(&key) {
            s.nonce = Some(nonce);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Set last used gas price for the attempt
    pub(crate) fn set_last_gas(&self, key: ActionKey, gas: u128) {
        if let Some(s) = self.attempts.lock().get_mut(&key) {
            s.last_gas = Some(gas);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Whether the key is expired, because it has not been evicted yet
    pub(crate) fn is_stuck(&self, key: ActionKey, now: u64) -> bool {
        self.attempts
            .lock()
            .get(&key)
            .is_some_and(|s| s.expired(now))
    }

    /// Retrieve the attemptstate from action key
    pub(crate) fn get(&self, key: ActionKey) -> Option<AttemptState> {
        self.attempts.lock().get(&key).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ActionKey {
        ActionKey::claim([7u8; 32])
    }

    #[test]
    fn ensure_inserts_due_now_then_record_success_removes() {
        let q = RetryQueue::default();
        q.ensure(key(), 1000);
        assert_eq!(q.due_now(1000), vec![key()]);
        q.record_success(key());
        assert!(q.due_now(1000).is_empty());
        assert!(q.get(key()).is_none());
    }

    #[test]
    fn record_failure_backs_off_so_not_due() {
        let q = RetryQueue::default();
        q.ensure(key(), 1000);
        q.record_failure(key(), 1000, 0);
        assert!(q.due_now(1000).is_empty());
        let later = 1000 + 10_000;
        assert_eq!(q.due_now(later), vec![key()]);
    }

    #[test]
    fn seed_preserves_backoff_and_is_stuck_reflects_deadline() {
        let q = RetryQueue::default();
        let mut st = AttemptState::new(0);
        st.on_failure(0, 0);
        q.seed(key(), st);
        assert_eq!(q.get(key()), Some(st));
        assert!(!q.is_stuck(key(), 0));
        assert!(q.is_stuck(key(), st.deadline));
    }

    #[test]
    fn seed_queue_makes_pending_due_and_preserves_seeded_backoff() {
        use stroemnet_protocol::v1::RevealV1;
        let mut restored = RestoredSwaps::default();
        restored
            .pending_claims
            .push(RevealV1::new([1u8; 32], [0u8; 32]));
        let mut backed = AttemptState::new(0);
        backed.on_failure(0, 0);
        restored.refund_attempts.insert([2u8; 32], backed);
        let q = seed_queue(&restored, 1000);
        assert!(q.due_now(1000).contains(&ActionKey::claim([1u8; 32])));
        assert_eq!(q.get(ActionKey::refund([2u8; 32])), Some(backed));
    }
}

use borsh::{BorshDeserialize, BorshSerialize};

#[cfg(not(target_arch = "wasm32"))]
const BASE_DELAY_SECS: u64 = 5;
#[cfg(not(target_arch = "wasm32"))]
const MAX_DELAY_SECS: u64 = 24 * 3600;
#[cfg(not(target_arch = "wasm32"))]
const JITTER_MAX_SECS: u64 = 60;
#[cfg(not(target_arch = "wasm32"))]
const MAX_BACKOFF_SHIFT: u32 = 13;
const DEADLINE_SECS: u64 = 7 * 24 * 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
/// Tracks an attempt and its state in order to compute when an attempt should be retried
pub struct AttemptState {
    /// First time we saw the attempt
    pub first_seen: u64,
    /// When is the next attempt
    pub next_attempt_at: u64,
    /// How many attempts for this
    pub attempt_count: u32,
    /// What is the deadline for this attempt record
    pub deadline: u64,
    /// Last nonce used for this attempt
    pub nonce: Option<u64>,
    /// Last gas used for this attempt
    pub last_gas: Option<u128>,
}

impl AttemptState {
    /// Create a new attempt record
    pub(crate) fn new(now: u64) -> Self {
        Self {
            first_seen: now,
            next_attempt_at: now,
            attempt_count: 0,
            deadline: now.saturating_add(DEADLINE_SECS),
            nonce: None,
            last_gas: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Check whether an attempt record is due by comparing now to when it should be retried
    pub(crate) fn due(&self, now: u64) -> bool {
        now >= self.next_attempt_at
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Check if a record has expired
    pub(crate) fn expired(&self, now: u64) -> bool {
        now >= self.deadline
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// In the case that a record failed we need to delay it by a random jitter value
    pub(crate) fn on_failure(&mut self, now: u64, jitter: u64) {
        // Compute the shift whatever is smallest
        let shift = self.attempt_count.min(MAX_BACKOFF_SHIFT);
        let delay = BASE_DELAY_SECS
            .saturating_mul(1u64 << shift) // multiply by the shift value
            .min(MAX_DELAY_SECS) // the minimum of whatever
            .saturating_add(jitter % JITTER_MAX_SECS); // random jitter value
        self.attempt_count = self.attempt_count.saturating_add(1); // increment attempt count
        self.next_attempt_at = now.saturating_add(delay); // update the next attempt at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_due_now_and_expires_at_deadline() {
        let s = AttemptState::new(1000);
        assert!(s.due(1000));
        assert!(!s.expired(1000));
        assert!(s.expired(1000 + DEADLINE_SECS));
        assert!(!s.due(999));
    }

    #[test]
    fn backoff_grows_then_caps_at_shift_bound() {
        let mut s = AttemptState::new(0);
        s.on_failure(0, 0);
        assert_eq!(s.next_attempt_at, BASE_DELAY_SECS);
        s.on_failure(0, 0);
        assert_eq!(s.next_attempt_at, BASE_DELAY_SECS * 2);
        for _ in 0..40 {
            s.on_failure(0, 0);
        }
        assert_eq!(s.next_attempt_at, BASE_DELAY_SECS << MAX_BACKOFF_SHIFT);
    }

    #[test]
    fn jitter_stays_bounded() {
        let mut s = AttemptState::new(0);
        s.on_failure(0, 999);
        assert!(s.next_attempt_at - BASE_DELAY_SECS < JITTER_MAX_SECS);
    }
}

use super::SwapTracker;
use crate::clock::now_unix_secs;

impl SwapTracker {
    pub fn is_expired(&self, swap_id: &[u8; 32]) -> bool {
        if let Some(record) = self.swaps.get(swap_id) {
            if record.resolution.is_some() {
                return false;
            }
            let now = now_unix_secs();
            if let Some(counter) = &record.counter_commitment {
                now >= counter.unlock_ts
            } else {
                now >= record.init_commitment.unlock_ts
            }
        } else {
            false
        }
    }

    pub fn time_until_init_refund(&self, swap_id: &[u8; 32]) -> Option<u64> {
        let record = self.swaps.get(swap_id)?;
        if record.resolution.is_some() {
            return None;
        }
        let now = now_unix_secs();
        Some(record.init_commitment.unlock_ts.saturating_sub(now))
    }

    pub fn time_until_ctpy_refund(&self, swap_id: &[u8; 32]) -> Option<u64> {
        let record = self.swaps.get(swap_id)?;
        if record.resolution.is_some() {
            return None;
        }
        let counter = record.counter_commitment.as_ref()?;
        let now = now_unix_secs();
        Some(counter.unlock_ts.saturating_sub(now))
    }

    pub fn cleanup_old_swaps(&mut self, max_age_secs: u64) {
        let now = now_unix_secs();
        self.swaps.retain(|_key, record| {
            if record.resolution.is_some() {
                let age = now.saturating_sub(record.created_at);
                age < max_age_secs
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v1::{AddressesV1, AmountV1, CommitmentV1};

    fn tracker_with_init(swap_id: [u8; 32], unlock_ts: u64) -> SwapTracker {
        let init = CommitmentV1::new(
            swap_id,
            AddressesV1::new("a".into(), "b".into(), "c".into()),
            AmountV1::new("1".into(), 8),
            [0u8; 32],
            unlock_ts,
            1,
            0,
        );
        let mut t = SwapTracker::new();
        t.set_init_commitment(swap_id, init).unwrap();
        t
    }

    #[test]
    fn expired_when_past_init_unlock() {
        let t = tracker_with_init([1u8; 32], 0);
        assert!(t.is_expired(&[1u8; 32]));
        assert_eq!(t.time_until_init_refund(&[1u8; 32]), Some(0));
        assert_eq!(t.time_until_ctpy_refund(&[1u8; 32]), None);
        assert!(!t.is_expired(&[9u8; 32]));
    }

    #[test]
    fn cleanup_retains_unresolved() {
        let mut t = tracker_with_init([2u8; 32], 0);
        t.cleanup_old_swaps(0);
        assert!(t.get_swap(&[2u8; 32]).is_some());
    }
}

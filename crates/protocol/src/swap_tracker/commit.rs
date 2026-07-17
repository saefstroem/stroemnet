use super::{Result, SwapRecord, SwapTracker, SwapTrackerError};
use crate::clock::now_unix_secs;
use crate::v1::CommitmentV1;

impl SwapTracker {
    pub fn set_init_commitment(
        &mut self,
        swap_id: [u8; 32],
        commitment: CommitmentV1,
    ) -> Result<()> {
        if self.swaps.contains_key(&swap_id) {
            return Err(SwapTrackerError::DuplicateSwap(swap_id));
        }
        let record = SwapRecord {
            init_commitment: commitment,
            counter_commitment: None,
            resolution: None,
            created_at: now_unix_secs(),
        };
        self.swaps.insert(swap_id, record);
        Ok(())
    }

    pub fn set_counter_commitment(
        &mut self,
        swap_id: [u8; 32],
        commitment: CommitmentV1,
    ) -> Result<()> {
        let record = self
            .swaps
            .get_mut(&swap_id)
            .ok_or(SwapTrackerError::SwapNotFound(swap_id))?;
        if record.counter_commitment.is_some() {
            return Err(SwapTrackerError::AlreadyCounterLocked(swap_id));
        }
        if record.resolution.is_some() {
            return Err(SwapTrackerError::AlreadyResolved(swap_id));
        }
        let init = &record.init_commitment;
        let fail = |reason: String| Err(SwapTrackerError::ValidationFailed { swap_id, reason });
        if init.swap_id != commitment.swap_id {
            return fail("Commitment swap_id does not match InitLock".to_string());
        }
        if init.addresses.sender_destination != commitment.addresses.receiver {
            return fail(
                "Commitment receiver does not match InitLock sender_destination".to_string(),
            );
        }
        if init.addresses.receiver != commitment.addresses.sender_destination {
            return fail(
                "Commitment sender_destination does not match InitLock receiver".to_string(),
            );
        }
        if init.secret_hash != commitment.secret_hash {
            return fail("Commitment secret_hash does not match InitLock secret_hash".to_string());
        }
        if init.source != commitment.destination || init.destination != commitment.source {
            return fail(format!(
                "source/destination mismatch: init(src={},dst={}) vs counter(src={},dst={})",
                init.source, init.destination, commitment.source, commitment.destination,
            ));
        }
        record.counter_commitment = Some(commitment);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v1::{AddressesV1, AmountV1};

    fn init(swap_id: [u8; 32]) -> CommitmentV1 {
        CommitmentV1::new(
            swap_id,
            AddressesV1::new("a".into(), "b".into(), "c".into()),
            AmountV1::new("1".into(), 8),
            [9u8; 32],
            0,
            1,
            0,
        )
    }

    #[test]
    fn duplicate_init_is_rejected() {
        let mut t = SwapTracker::new();
        t.set_init_commitment([1u8; 32], init([1u8; 32])).unwrap();
        assert!(matches!(
            t.set_init_commitment([1u8; 32], init([1u8; 32])),
            Err(SwapTrackerError::DuplicateSwap(_))
        ));
    }

    #[test]
    fn counter_on_missing_swap_errors() {
        let mut t = SwapTracker::new();
        assert!(matches!(
            t.set_counter_commitment([2u8; 32], init([2u8; 32])),
            Err(SwapTrackerError::SwapNotFound(_))
        ));
    }
}

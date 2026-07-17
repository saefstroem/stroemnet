use super::{Result, SwapRecord, SwapTracker, SwapTrackerError};

impl SwapTracker {
    fn locked_unresolved_mut(&mut self, swap_id: [u8; 32]) -> Result<&mut SwapRecord> {
        let record = self
            .swaps
            .get_mut(&swap_id)
            .ok_or(SwapTrackerError::SwapNotFound(swap_id))?;
        if record.counter_commitment.is_none() {
            return Err(SwapTrackerError::NotCounterLocked(swap_id));
        }
        if record.resolution.is_some() {
            return Err(SwapTrackerError::AlreadyResolved(swap_id));
        }
        Ok(record)
    }

    pub fn set_revealed(&mut self, swap_id: [u8; 32], secret: [u8; 32]) -> Result<()> {
        use sha2::{Digest, Sha256};

        let record = self.locked_unresolved_mut(swap_id)?;
        let counter = record
            .counter_commitment
            .as_ref()
            .ok_or(SwapTrackerError::NotCounterLocked(swap_id))?;

        let mut hasher = Sha256::new();
        hasher.update(secret);
        let digest = hasher.finalize();

        if digest.as_slice() != counter.secret_hash.as_slice() {
            return Err(SwapTrackerError::SecretHashMismatch { swap_id });
        }

        record.resolution = Some(hex::encode(secret));
        Ok(())
    }

    pub fn set_refunded(&mut self, swap_id: [u8; 32]) -> Result<()> {
        let record = self.locked_unresolved_mut(swap_id)?;
        record.resolution = Some("refunded".to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v1::{AddressesV1, AmountV1, CommitmentV1};

    fn locked_tracker(swap_id: [u8; 32], secret_hash: [u8; 32]) -> SwapTracker {
        let init = CommitmentV1::new(
            swap_id,
            AddressesV1::new("snd".into(), "rcv".into(), "sdst".into()),
            AmountV1::new("1".into(), 8),
            secret_hash,
            0,
            1,
            0,
        );
        let counter = CommitmentV1::new(
            swap_id,
            AddressesV1::new("ksnd".into(), "sdst".into(), "rcv".into()),
            AmountV1::new("1".into(), 8),
            secret_hash,
            0,
            0,
            1,
        );
        let mut t = SwapTracker::new();
        t.set_init_commitment(swap_id, init).unwrap();
        t.set_counter_commitment(swap_id, counter).unwrap();
        t
    }

    #[test]
    fn reveal_with_wrong_secret_rejected() {
        use sha2::{Digest, Sha256};
        let secret = [4u8; 32];
        let hash: [u8; 32] = Sha256::digest(secret).into();
        let mut t = locked_tracker([1u8; 32], hash);
        assert!(matches!(
            t.set_revealed([1u8; 32], [9u8; 32]),
            Err(SwapTrackerError::SecretHashMismatch { .. })
        ));
        t.set_revealed([1u8; 32], secret).unwrap();
    }

    #[test]
    fn refund_requires_counter_lock() {
        let mut t = SwapTracker::new();
        let init = CommitmentV1::new(
            [2u8; 32],
            AddressesV1::new("a".into(), "b".into(), "c".into()),
            AmountV1::new("1".into(), 8),
            [0u8; 32],
            0,
            1,
            0,
        );
        t.set_init_commitment([2u8; 32], init).unwrap();
        assert!(matches!(
            t.set_refunded([2u8; 32]),
            Err(SwapTrackerError::NotCounterLocked(_))
        ));
    }
}

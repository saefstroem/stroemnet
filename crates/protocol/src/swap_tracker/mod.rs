mod error;
mod result;

use crate::{clock::now_unix_secs, v1::CommitmentV1};
use ahash::AHashMap;
pub use error::SwapTrackerError;
use result::Result;
use std::fmt::Display;

#[derive(Debug, Clone, PartialEq, Eq)]
/// An internal record for an entire swap.
/// It is populated throughout the entire process of the swap. It contains the initial commitment
/// by some user, then a counter commitment and eventually a resolution which contains either the secret or a "refunded" string.
///
/// As all fields except init_commitment are optional, a swap is valid from the moment init_commitment is set.
/// This is because a swap in itself can be refunded, which is considered a valid transition within the protocol.
pub struct SwapRecord {
    /// Initial commitment of the swap, containing all the details of the swap and the initial lock.
    pub init_commitment: CommitmentV1,
    /// A counter commitment made by an LP who is taking the other side of the swap
    pub counter_commitment: Option<CommitmentV1>,
    /// The resolution of the swap, which can either be the secret used to redeem the funds or a "refunded" string in case of refunds.
    pub resolution: Option<String>,
    /// At what time the swap record was created. This is used for cleanup of old swaps after they are resolved.
    pub created_at: u64,
}

#[derive(Debug, Default)]
/// A tracker who maps swap id to swap records, allowing to keep track of
/// the state of each swap and its details throughout the entire process.
pub struct SwapTracker {
    swaps: AHashMap<[u8; 32], SwapRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// An enum representing the different stages of a swap, used for easy tracking and display purposes.
pub enum SwapStage {
    Initialized, // the swap stage has been initialized with the initial commitment, but no counter commitment has been made yet
    Locked, // a counter commitment has been made by an LP, locking the swap but it has not been resolved yet
    Completed, // the swap has been completed with the secret revealed and the funds redeemed by the user
    Refunded, // the swap has been refunded, either by the user before an LP took the other side, or by the LP after taking the other side but before the secret was revealed
}

impl Display for SwapStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SwapStage::Initialized => write!(f, "Initialized"),
            SwapStage::Locked => write!(f, "Locked"),
            SwapStage::Completed => write!(f, "Completed"),
            SwapStage::Refunded => write!(f, "Refunded"),
        }
    }
}

impl SwapTracker {
    pub fn new() -> Self {
        Self {
            swaps: AHashMap::new(),
        }
    }

    pub(crate) fn now() -> u64 {
        now_unix_secs()
    }

    /// A valid initial commitment is set for some swap. The swap tracker only allows one
    /// initial commitment per swap id.
    pub fn set_init_commitment(
        &mut self,
        swap_id: [u8; 32],
        commitment: CommitmentV1,
    ) -> Result<()> {
        // If a swap contains this key already we do not allow overwriting it
        if self.swaps.contains_key(&swap_id) {
            return Err(SwapTrackerError::DuplicateSwap(swap_id));
        }

        // Create a new swap record with the initial commitment and insert it into the tracker
        let record = SwapRecord {
            init_commitment: commitment,
            counter_commitment: None,
            resolution: None,
            created_at: Self::now(),
        };
        self.swaps.insert(swap_id, record);
        Ok(())
    }

    /// A counter commitment is set for some swap.
    /// This can only be done if an initial commitment already exists for the swap and if a counter commitment has not been set before.
    pub fn set_counter_commitment(
        &mut self,
        swap_id: [u8; 32],
        commitment: CommitmentV1,
    ) -> Result<()> {
        // Retrieve the initial commitment record for the swap, if it does not exist we cannot set a counter commitment
        let record = self
            .swaps
            .get_mut(&swap_id)
            .ok_or(SwapTrackerError::SwapNotFound(swap_id))?;

        // A counter commitment already exists, then we cannot set another one
        if record.counter_commitment.is_some() {
            return Err(SwapTrackerError::AlreadyCounterLocked(swap_id));
        }

        // The swap has already been resolved, we cannot set a counter commitment on a resolved swap
        if record.resolution.is_some() {
            return Err(SwapTrackerError::AlreadyResolved(swap_id));
        }

        // We retrieve the initial commitment to perform some validation against this claimed counter
        // commitment to ensure it is consistent with protocol rules.
        let init = &record.init_commitment;

        // If the initial commitment doesnt match the counter commitment,
        // these swaps are inherently for different swaps.
        if init.swap_id != commitment.swap_id {
            return Err(SwapTrackerError::ValidationFailed {
                swap_id,
                reason: "Commitment swap_id does not match InitLock".to_string(),
            });
        }

        // The initial commitments sender destination must match the counter commitment receiver
        // Because from the counter commitment the receiver should be the origins sender destination
        if init.addresses.sender_destination != commitment.addresses.receiver {
            return Err(SwapTrackerError::ValidationFailed {
                swap_id,
                reason: "Commitment receiver does not match InitLock sender_destination"
                    .to_string(),
            });
        }

        // Like above, the receiver of the initial commitment should be the
        // sender destination of the counter commitment, otherwise these swaps are inconsistent with each other.
        if init.addresses.receiver != commitment.addresses.sender_destination {
            return Err(SwapTrackerError::ValidationFailed {
                swap_id,
                reason: "Commitment sender_destination does not match InitLock receiver"
                    .to_string(),
            });
        }

        // Both commitments' secret hash should be the same otherwise
        // they would not be possible to unlock with the same secret and are thus inconsistent with each other.
        if init.secret_hash != commitment.secret_hash {
            return Err(SwapTrackerError::ValidationFailed {
                swap_id,
                reason: "Commitment secret_hash does not match InitLock secret_hash".to_string(),
            });
        }

        // Both commitments should have a matching source destination pairing.
        // The initial commiments source must be equal to the commitments destination
        // and vice versa.
        if init.source != commitment.destination || init.destination != commitment.source {
            return Err(SwapTrackerError::ValidationFailed {
                swap_id,
                reason: format!(
                    "source/destination mismatch: init(src={},dst={}) vs counter(src={},dst={})",
                    init.source, init.destination, commitment.source, commitment.destination,
                ),
            });
        }

        record.counter_commitment = Some(commitment);
        Ok(())
    }

    /// A swap is marked as revealed with the secret used to redeem the funds.
    /// This can only be done if the swap has been locked with a counter commitment and has not been resolved before.
    pub fn set_revealed(&mut self, swap_id: [u8; 32], secret: [u8; 32]) -> Result<()> {
        use sha2::{Digest, Sha256};

        // Retrieve the existing swap record
        let record = self
            .swaps
            .get_mut(&swap_id)
            .ok_or(SwapTrackerError::SwapNotFound(swap_id))?;

        // A swap can only be revealed if it has a counter commitment.
        let counter = record
            .counter_commitment
            .as_ref()
            .ok_or(SwapTrackerError::NotCounterLocked(swap_id))?;

        // A swap can only be revealed if it has not been resolved before.
        if record.resolution.is_some() {
            return Err(SwapTrackerError::AlreadyResolved(swap_id));
        }

        // Lets compute the hash of the claimed secret
        let mut hasher = Sha256::new();
        hasher.update(secret);
        let digest = hasher.finalize();

        // The hash must match what was originally set in the counter commitment
        // and because we checked counter hash == init hash, this also validates that.
        if digest.as_slice() != counter.secret_hash.as_slice() {
            return Err(SwapTrackerError::SecretHashMismatch { swap_id });
        }

        // All validations passed, swap is revealed.
        record.resolution = Some(hex::encode(secret));
        Ok(())
    }

    /// Marks a swap as refunded.
    pub fn set_refunded(&mut self, swap_id: [u8; 32]) -> Result<()> {
        // Retrieve the existing swap record
        let record = self
            .swaps
            .get_mut(&swap_id)
            .ok_or(SwapTrackerError::SwapNotFound(swap_id))?;

        // A swap can only be refunded if it has not been resolved before.
        // This includes both revealed and already refunded swaps.
        if record.counter_commitment.is_none() {
            return Err(SwapTrackerError::NotCounterLocked(swap_id));
        }

        // A swap can only be refunded if it has not been resolved before.
        // This includes both revealed and already refunded swaps.
        if record.resolution.is_some() {
            return Err(SwapTrackerError::AlreadyResolved(swap_id));
        }

        // All validations passed, swap is refunded.
        record.resolution = Some("refunded".to_string());
        Ok(())
    }

    /// Retrieve a swap from the tracker
    pub fn get_swap(&self, swap_id: &[u8; 32]) -> Option<&SwapRecord> {
        self.swaps.get(swap_id)
    }

    /// Retrieve all swaps from the tracker as an iterator of swap id and swap record pairs.
    pub fn all_swaps(&self) -> impl Iterator<Item = (&[u8; 32], &SwapRecord)> {
        self.swaps.iter()
    }

    /// Extract the current stage of a swap based on its record.
    /// This is determined by the presence of the counter commitment and resolution.
    pub fn stage(record: &SwapRecord) -> SwapStage {
        match &record.resolution {
            Some(r) if r == "refunded" => SwapStage::Refunded,
            Some(_) => SwapStage::Completed,
            None if record.counter_commitment.is_some() => SwapStage::Locked,
            None => SwapStage::Initialized,
        }
    }

    /// Compute whether a swap is expired based on the current time and the unlock time of its commitments.
    pub fn is_expired(&self, swap_id: &[u8; 32]) -> bool {
        // A swap is considered expired if the current time
        // is past the unlock time of its latest commitment
        // (counter commitment if it exists, otherwise initial commitment) and it has not been resolved yet.
        if let Some(record) = self.swaps.get(swap_id) {
            // If there is a resolution it means the swap has already been completed or refunded, so it cannot be expired.
            if record.resolution.is_some() {
                return false;
            }
            let now = Self::now();
            // We check the counter commitment unlock time if it exists, otherwise we check the initial commitment unlock time.
            if let Some(counter) = &record.counter_commitment {
                // By definition the counter commitment, is shorter which means that we should check against that
                // since if the counter is expired, the swap is expired regardless of the initial commitment unlock time.
                now >= counter.unlock_ts
            } else {
                // If the LP never locked, then the user can refund after the initial commitment unlock time, so we check against that.
                now >= record.init_commitment.unlock_ts
            }
        } else {
            false
        }
    }

    /// Compute the how many seconds are left until a swap can be refunded by the
    /// user
    pub fn time_until_init_refund(&self, swap_id: &[u8; 32]) -> Option<u64> {
        let record = self.swaps.get(swap_id)?;
        // If there is a resolution it means the swap has already been completed or refunded, so there is no refund time.
        if record.resolution.is_some() {
            return None;
        }
        let now = Self::now();
        Some(record.init_commitment.unlock_ts.saturating_sub(now))
    }

    /// Compute how many seconds are left until a swap can be refunded by the LP.
    pub fn time_until_ctpy_refund(&self, swap_id: &[u8; 32]) -> Option<u64> {
        let record = self.swaps.get(swap_id)?;
        if record.resolution.is_some() || record.counter_commitment.is_none() {
            return None;
        }
        let now = Self::now();
        let counter = record.counter_commitment.as_ref().unwrap();
        Some(counter.unlock_ts.saturating_sub(now))
    }

    /// Cleanup old swaps that have been resolved for a long time to prevent the tracker from growing indefinitely.
    pub fn cleanup_old_swaps(&mut self, max_age_secs: u64) {
        let now = Self::now();
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
mod tests;

use std::sync::Arc;

use ahash::AHashMap;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{RefundV1, RevealV1};

use super::codec::{DecodeOutcome, decode};
use crate::{AttemptState, SwapStore, UtxoScript};

#[derive(Default)]
/// Swaps that have been restored from disk after a restart
pub(crate) struct RestoredSwaps {
    /// Scripts keyed by swap id
    pub scripts: AHashMap<[u8; 32], UtxoScript>,
    /// Pending refund queue
    pub pending_refunds: Vec<(RefundV1, u64)>,
    /// Pending claim queue
    pub pending_claims: Vec<RevealV1>,
    /// Claim attemps by swap id
    pub claim_attempts: AHashMap<[u8; 32], AttemptState>,
    /// Refund attempts by swap id
    pub refund_attempts: AHashMap<[u8; 32], AttemptState>,
}

/// Restore swaps from an existing swap store and channel id
pub(crate) fn restore(
    swap_store: Option<&Arc<dyn SwapStore>>,
    channel_id: ChannelId,
) -> RestoredSwaps {
    let mut out = RestoredSwaps::default();
    // if we dont have a store we simply return the default empty struct
    let Some(store) = swap_store else {
        return out;
    };
    // Go over the store and try to load the channel which will return all swaps there
    for (swap_id, bytes) in store.load_channel(channel_id) {
        let rec = match decode(&bytes) {
            // attempt to decode
            DecodeOutcome::Current(rec) => *rec,
            DecodeOutcome::Corrupt(e) => {
                tracing::error!(
                    target: "settlement",
                    "seed swap {} on {channel_id} corrupt, quarantined: {e}",
                    hex::encode(swap_id)
                );
                store.quarantine(channel_id, swap_id, &bytes, &e.to_string());
                continue;
            }
        };

        // Populate all fields if they exist
        if let Some(s) = rec.script {
            out.scripts.insert(swap_id, s);
        }
        if let Some(pr) = rec.pending_refund {
            out.pending_refunds.push(pr);
        }
        if let Some(pc) = rec.pending_claim {
            out.pending_claims.push(pc);
        }
        if let Some(a) = rec.claim_attempt {
            out.claim_attempts.insert(swap_id, a);
        }
        if let Some(a) = rec.refund_attempt {
            out.refund_attempts.insert(swap_id, a);
        }
    }
    out // return the restored swaps
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::super::codec::encode;
    use super::*;
    use crate::PersistedSwap;
    use std::sync::Mutex;

    struct MockStore {
        rows: Vec<([u8; 32], Vec<u8>)>,
        quarantined: Mutex<Vec<[u8; 32]>>,
    }

    impl SwapStore for MockStore {
        fn load_channel(&self, _c: ChannelId) -> Vec<([u8; 32], Vec<u8>)> {
            self.rows.clone()
        }
        fn save(&self, _c: ChannelId, _s: [u8; 32], _r: &[u8]) {}
        fn delete(&self, _c: ChannelId, _s: [u8; 32]) {}
        fn quarantine(&self, _c: ChannelId, swap_id: [u8; 32], _raw: &[u8], _reason: &str) {
            self.quarantined.lock().unwrap().push(swap_id);
        }
    }

    #[test]
    fn keeps_valid_and_quarantines_corrupt_never_dropping() {
        let good = PersistedSwap {
            script: None,
            pending_refund: Some((RefundV1::new([1u8; 32]), 5)),
            pending_claim: None,
            claim_attempt: None,
            refund_attempt: None,
        };
        let mock = Arc::new(MockStore {
            rows: vec![
                ([1u8; 32], encode(&good).unwrap()),
                ([2u8; 32], vec![0x53, 1, 0xff]),
            ],
            quarantined: Mutex::new(Vec::new()),
        });
        let store: Arc<dyn SwapStore> = mock.clone();

        let restored = restore(Some(&store), ChannelId::KaspaTn10);

        assert_eq!(restored.pending_refunds.len(), 1);
        assert_eq!(*mock.quarantined.lock().unwrap(), vec![[2u8; 32]]);
    }
}

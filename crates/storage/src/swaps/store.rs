use std::sync::Arc;

use stroemnet_data::SwapStore;
use stroemnet_protocol::ChannelId;

use crate::PeerDb;

pub struct DbSwapStore {
    db: Arc<PeerDb>,
}

impl DbSwapStore {
    pub fn new(db: Arc<PeerDb>) -> Self {
        Self { db }
    }
}

impl SwapStore for DbSwapStore {
    fn load_channel(&self, channel_id: ChannelId) -> Vec<([u8; 32], Vec<u8>)> {
        match self.db.get_swaps_for_channel(channel_id) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("swap load failed for {channel_id}: {e}");
                Vec::new()
            }
        }
    }

    fn save(&self, channel_id: ChannelId, swap_id: [u8; 32], record: &[u8]) {
        if let Err(e) = self.db.set_swap(channel_id, &swap_id, record) {
            tracing::warn!(
                "swap persist failed for {channel_id} {}: {e}",
                hex::encode(swap_id)
            );
        }
    }

    fn delete(&self, channel_id: ChannelId, swap_id: [u8; 32]) {
        if let Err(e) = self.db.remove_swap(channel_id, &swap_id) {
            tracing::warn!(
                "swap delete failed for {channel_id} {}: {e}",
                hex::encode(swap_id)
            );
        }
    }

    fn quarantine(&self, channel_id: ChannelId, swap_id: [u8; 32], raw: &[u8], reason: &str) {
        match self.db.quarantine_swap(channel_id, &swap_id, raw) {
            Ok(()) => tracing::error!(
                target: "settlement",
                "quarantined corrupt swap {} on {channel_id}: {reason}",
                hex::encode(swap_id)
            ),
            Err(e) => {
                tracing::warn!(
                    "swap quarantine failed for {channel_id} {}: {e}",
                    hex::encode(swap_id)
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn store_roundtrips_via_trait() {
        let dir = tempdir().unwrap();
        let db = Arc::new(PeerDb::new(&dir.path().join("s.db")).unwrap());
        let store = DbSwapStore::new(db);
        store.save(ChannelId::KaspaTn10, [1u8; 32], &[1, 2, 3]);
        assert_eq!(
            store.load_channel(ChannelId::KaspaTn10),
            vec![([1u8; 32], vec![1, 2, 3])]
        );
        store.delete(ChannelId::KaspaTn10, [1u8; 32]);
        assert!(store.load_channel(ChannelId::KaspaTn10).is_empty());
    }
}

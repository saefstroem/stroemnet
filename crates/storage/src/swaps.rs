use std::sync::Arc;

use redb::{ReadableDatabase, TableDefinition};
use stroemnet_data::SwapStore;
use stroemnet_protocol::ChannelId;

use crate::{PeerDb, Result};

pub(crate) const SWAPS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("swaps");

fn swap_key(channel: ChannelId, swap_id: &[u8; 32]) -> [u8; 33] {
    let mut key = [0u8; 33];
    key[0] = channel as u8;
    key[1..].copy_from_slice(swap_id);
    key
}

impl PeerDb {
    pub fn get_swaps_for_channel(&self, channel: ChannelId) -> Result<Vec<([u8; 32], Vec<u8>)>> {
        let read_txn = self.inner.begin_read()?;
        let table = read_txn.open_table(SWAPS)?;
        let chan = channel as u8;
        let lo = [chan];
        let hi = [chan.wrapping_add(1)];
        let range = if chan == u8::MAX {
            table.range(lo.as_slice()..)?
        } else {
            table.range(lo.as_slice()..hi.as_slice())?
        };
        let mut out = Vec::new();
        for row in range {
            let (k, v) = row?;
            let key = k.value();
            if key.len() == 33 {
                let mut swap_id = [0u8; 32];
                swap_id.copy_from_slice(&key[1..]);
                out.push((swap_id, v.value().to_vec()));
            }
        }
        Ok(out)
    }

    pub fn set_swap(&self, channel: ChannelId, swap_id: &[u8; 32], record: &[u8]) -> Result<()> {
        let write_txn = self.inner.begin_write()?;
        {
            let mut table = write_txn.open_table(SWAPS)?;
            table.insert(swap_key(channel, swap_id).as_slice(), record)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn remove_swap(&self, channel: ChannelId, swap_id: &[u8; 32]) -> Result<()> {
        let write_txn = self.inner.begin_write()?;
        {
            let mut table = write_txn.open_table(SWAPS)?;
            table.remove(swap_key(channel, swap_id).as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }
}

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn swap_set_get_delete_roundtrip() {
        let dir = tempdir().unwrap();
        let db = PeerDb::new(&dir.path().join("s.db")).unwrap();
        let id = [7u8; 32];
        assert!(
            db.get_swaps_for_channel(ChannelId::IgraGalleon)
                .unwrap()
                .is_empty()
        );
        db.set_swap(ChannelId::IgraGalleon, &id, &[1, 2, 3]).unwrap();
        assert_eq!(
            db.get_swaps_for_channel(ChannelId::IgraGalleon).unwrap(),
            vec![(id, vec![1, 2, 3])]
        );
        db.set_swap(ChannelId::IgraGalleon, &id, &[9]).unwrap();
        assert_eq!(
            db.get_swaps_for_channel(ChannelId::IgraGalleon).unwrap(),
            vec![(id, vec![9])]
        );
        db.remove_swap(ChannelId::IgraGalleon, &id).unwrap();
        assert!(
            db.get_swaps_for_channel(ChannelId::IgraGalleon)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn load_channel_isolates_by_channel() {
        let dir = tempdir().unwrap();
        let db = PeerDb::new(&dir.path().join("s.db")).unwrap();
        let a = [1u8; 32];
        let b = [2u8; 32];
        db.set_swap(ChannelId::IgraGalleon, &a, &[0xaa]).unwrap();
        db.set_swap(ChannelId::KaspaTn10, &b, &[0xbb]).unwrap();
        assert_eq!(
            db.get_swaps_for_channel(ChannelId::IgraGalleon).unwrap(),
            vec![(a, vec![0xaa])]
        );
        assert_eq!(
            db.get_swaps_for_channel(ChannelId::KaspaTn10).unwrap(),
            vec![(b, vec![0xbb])]
        );
        assert!(
            db.get_swaps_for_channel(ChannelId::EthereumSepolia)
                .unwrap()
                .is_empty()
        );
    }
}

use redb::{ReadableDatabase, ReadableTableMetadata, TableDefinition};
use stroemnet_protocol::ChannelId;

use crate::{PeerDb, Result};

pub(crate) const QUARANTINE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("quarantine");

fn quarantine_key(channel: ChannelId, swap_id: &[u8; 32]) -> [u8; 33] {
    let mut key = [0u8; 33];
    key[0] = channel as u8;
    key[1..].copy_from_slice(swap_id);
    key
}

impl PeerDb {
    pub fn quarantine_swap(
        &self,
        channel: ChannelId,
        swap_id: &[u8; 32],
        raw: &[u8],
    ) -> Result<()> {
        let write_txn = self.inner.begin_write()?;
        {
            let mut table = write_txn.open_table(QUARANTINE)?;
            table.insert(quarantine_key(channel, swap_id).as_slice(), raw)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn quarantined_count(&self) -> Result<u64> {
        let read_txn = self.inner.begin_read()?;
        let table = read_txn.open_table(QUARANTINE)?;
        Ok(table.len()?)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn quarantine_persists_and_counts() {
        let dir = tempdir().unwrap();
        let db = PeerDb::new(&dir.path().join("q.db")).unwrap();
        assert_eq!(db.quarantined_count().unwrap(), 0);
        db.quarantine_swap(ChannelId::KaspaTn10, &[4u8; 32], &[0xde, 0xad])
            .unwrap();
        db.quarantine_swap(ChannelId::EthereumSepolia, &[5u8; 32], &[0xbe])
            .unwrap();
        assert_eq!(db.quarantined_count().unwrap(), 2);
    }
}

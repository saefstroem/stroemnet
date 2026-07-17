use std::sync::Arc;

use redb::{ReadableDatabase, TableDefinition};
use stroemnet_data::CursorStore;
use stroemnet_protocol::ChannelId;

use crate::{PeerDb, Result};

pub(crate) const CURSORS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("cursors");

impl PeerDb {
    pub fn get_cursor(&self, channel: ChannelId) -> Result<Option<Vec<u8>>> {
        let read_txn = self.inner.begin_read()?;
        let table = read_txn.open_table(CURSORS)?;
        let key = [channel as u8];
        match table.get(key.as_slice())? {
            Some(v) => Ok(Some(v.value().to_vec())),
            None => Ok(None),
        }
    }

    pub fn set_cursor(&self, channel: ChannelId, cursor: &[u8]) -> Result<()> {
        let write_txn = self.inner.begin_write()?;
        {
            let mut table = write_txn.open_table(CURSORS)?;
            let key = [channel as u8];
            table.insert(key.as_slice(), cursor)?;
        }
        write_txn.commit()?;
        Ok(())
    }
}

pub struct DbCursorStore {
    db: Arc<PeerDb>,
}

impl DbCursorStore {
    pub fn new(db: Arc<PeerDb>) -> Self {
        Self { db }
    }
}

impl CursorStore for DbCursorStore {
    fn load(&self, channel_id: ChannelId) -> Option<Vec<u8>> {
        self.db.get_cursor(channel_id).ok().flatten()
    }

    fn save(&self, channel_id: ChannelId, cursor: &[u8]) {
        if let Err(e) = self.db.set_cursor(channel_id, cursor) {
            tracing::warn!("cursor persist failed for {channel_id}: {e}");
        }
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
    fn cursor_set_get_roundtrip() {
        let dir = tempdir().unwrap();
        let db = PeerDb::new(&dir.path().join("c.db")).unwrap();
        assert_eq!(db.get_cursor(ChannelId::KaspaTn10).unwrap(), None);
        db.set_cursor(ChannelId::KaspaTn10, &[1, 2, 3, 4]).unwrap();
        assert_eq!(
            db.get_cursor(ChannelId::KaspaTn10).unwrap(),
            Some(vec![1, 2, 3, 4])
        );
        db.set_cursor(ChannelId::KaspaTn10, &[9, 9]).unwrap();
        assert_eq!(
            db.get_cursor(ChannelId::KaspaTn10).unwrap(),
            Some(vec![9, 9])
        );
        assert_eq!(db.get_cursor(ChannelId::EthereumSepolia).unwrap(), None);
    }
}

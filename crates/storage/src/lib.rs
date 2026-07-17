mod cursors;
mod error;
mod peers;
mod quarantine;
mod swaps;
use std::path::Path;

use redb::{Database, WriteTransaction};

pub use cursors::DbCursorStore;
pub use error::DbError;
pub use peers::Peer;
pub use swaps::DbSwapStore;

use crate::cursors::CURSORS;
use crate::peers::PEERS;
use crate::quarantine::QUARANTINE;
use crate::swaps::SWAPS;

pub type Result<T> = std::result::Result<T, DbError>;

pub struct PeerDb {
    pub(crate) inner: Database,
}

impl PeerDb {
    pub fn wtx(&self) -> Result<WriteTransaction> {
        Ok(self.inner.begin_write()?)
    }

    pub fn new(path: &Path) -> Result<Self> {
        let inner = Database::builder().create(path)?;
        {
            let write_txn = inner.begin_write()?;
            let _ = write_txn.open_table(PEERS)?;
            let _ = write_txn.open_table(CURSORS)?;
            let _ = write_txn.open_table(SWAPS)?;
            let _ = write_txn.open_table(QUARANTINE)?;
            write_txn.commit()?;
        }
        Ok(Self { inner })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn new_opens_all_tables_and_grants_write_txn() {
        let dir = tempdir().unwrap();
        let db = PeerDb::new(&dir.path().join("p.db")).unwrap();
        assert!(db.wtx().is_ok());
    }
}

mod cursors;
mod error;
mod peers;
use std::path::Path;

use redb::{Database, WriteTransaction};

pub use cursors::DbCursorStore;
pub use error::DbError;
pub use peers::Peer;

use crate::cursors::CURSORS;
use crate::peers::PEERS;

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
            write_txn.commit()?;
        }
        Ok(Self { inner })
    }
}

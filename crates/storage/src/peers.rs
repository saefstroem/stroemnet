use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use url::Url;

use crate::{PeerDb, Result};

#[derive(Debug, Clone)]
pub struct Peer {
    pub url: Url,
}

pub(crate) const PEERS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("peers");

fn parse_peer_row(key: &[u8], value: &[u8]) -> Option<Peer> {
    let s = match std::str::from_utf8(value) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                "PeerDb: skipping corrupt row (non-UTF-8 value, key={}): {e}",
                String::from_utf8_lossy(key)
            );
            return None;
        }
    };
    match Url::parse(s) {
        Ok(url) => Some(Peer { url }),
        Err(e) => {
            tracing::warn!("PeerDb: skipping corrupt row (invalid URL {s:?}): {e}");
            None
        }
    }
}

impl PeerDb {
    pub fn get_peers(&self) -> Result<Vec<Peer>> {
        let read_txn = self.inner.begin_read()?;
        let table = read_txn.open_table(PEERS)?;
        let mut peers = Vec::new();
        for item in table.iter()? {
            let (k, v) = item?;
            if let Some(p) = parse_peer_row(k.value(), v.value()) {
                peers.push(p);
            }
        }
        Ok(peers)
    }

    pub fn get_peer(&self, url: &Url) -> Result<Option<Peer>> {
        let read_txn = self.inner.begin_read()?;
        let table = read_txn.open_table(PEERS)?;
        let key = url.as_str().as_bytes();
        match table.get(key)? {
            Some(v) => Ok(parse_peer_row(key, v.value())),
            None => Ok(None),
        }
    }

    pub fn add_peer(&self, peer: Peer) -> Result<()> {
        let write_txn = self.inner.begin_write()?;
        {
            let mut table = write_txn.open_table(PEERS)?;
            let url_bytes = peer.url.as_str().as_bytes();
            table.insert(url_bytes, url_bytes)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn remove_peer(&self, url: &str) -> Result<()> {
        let write_txn = self.inner.begin_write()?;
        {
            let mut table = write_txn.open_table(PEERS)?;
            table.remove(url.as_bytes())?;
        }
        write_txn.commit()?;
        Ok(())
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
    use std::sync::OnceLock;
    use tempfile::tempdir;

    fn init_tracing() {
        static ONCE: OnceLock<()> = OnceLock::new();
        ONCE.get_or_init(|| {
            let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        });
    }

    #[test]
    fn add_get_remove_roundtrip() {
        init_tracing();
        let dir = tempdir().unwrap();
        let db = PeerDb::new(&dir.path().join("peers.db")).unwrap();

        let url = Url::parse("ws://example.com:9999/").unwrap();
        db.add_peer(Peer { url: url.clone() }).unwrap();

        let got = db.get_peer(&url).unwrap().unwrap();
        assert_eq!(got.url, url);

        let all = db.get_peers().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].url, url);

        db.remove_peer(url.as_str()).unwrap();
        assert!(db.get_peer(&url).unwrap().is_none());
        assert_eq!(db.get_peers().unwrap().len(), 0);
    }

    #[test]
    fn corrupt_row_skipped_not_panicked() {
        init_tracing();
        let dir = tempdir().unwrap();
        let path = dir.path().join("peers.db");
        let db = PeerDb::new(&path).unwrap();

        let good = Url::parse("ws://good.example.com/").unwrap();
        db.add_peer(Peer { url: good.clone() }).unwrap();

        {
            let wtx = db.inner.begin_write().unwrap();
            {
                let mut table = wtx.open_table(PEERS).unwrap();
                table
                    .insert(b"corrupt-key".as_ref(), &[0xFF, 0xFE, 0xFD][..])
                    .unwrap();
            }
            wtx.commit().unwrap();
        }

        {
            let wtx = db.inner.begin_write().unwrap();
            {
                let mut table = wtx.open_table(PEERS).unwrap();
                table
                    .insert(b"not-a-url".as_ref(), b"::::not a url::::".as_ref())
                    .unwrap();
            }
            wtx.commit().unwrap();
        }

        let peers = db.get_peers().unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].url, good);
    }

    #[test]
    fn get_peer_returns_none_for_missing() {
        init_tracing();
        let dir = tempdir().unwrap();
        let db = PeerDb::new(&dir.path().join("peers.db")).unwrap();
        let url = Url::parse("ws://nope.example.com/").unwrap();
        assert!(db.get_peer(&url).unwrap().is_none());
    }
}

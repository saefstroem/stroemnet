use std::collections::{HashSet, VecDeque};

use sha2::{Digest, Sha256};

/// A LRU set of seen message hashes for deduplication in the gossip protocol.
pub struct SeenSet {
    cap: usize,
    set: HashSet<[u8; 32]>,
    order: VecDeque<[u8; 32]>,
}

impl SeenSet {
    /// Create a new SeenSet with the given capacity.
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            set: HashSet::new(),
            order: VecDeque::new(),
        }
    }

    /// Insert a hash into the set. Returns true if it was not already present.
    pub fn insert(&mut self, hash: [u8; 32]) -> bool {
        if !self.set.insert(hash) {
            return false;
        }
        self.order.push_back(hash);
        if self.set.len() > self.cap
            && let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            }
        true
    }

    /// Get the number of hashes in the set.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Check if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Compute the hash of a payload.
    pub fn hash(payload: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(&Sha256::digest(payload));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup() {
        let mut s = SeenSet::new(3);
        let h = SeenSet::hash(b"hello");
        assert!(s.insert(h));
        assert!(!s.insert(h));
    }

    #[test]
    fn lru_eviction() {
        let mut s = SeenSet::new(2);
        let a = SeenSet::hash(b"a");
        let b = SeenSet::hash(b"b");
        let c = SeenSet::hash(b"c");
        s.insert(a);
        s.insert(b);
        s.insert(c);
        assert!(s.insert(a));
    }

    #[test]
    fn hash_deterministic() {
        assert_eq!(SeenSet::hash(b"x"), SeenSet::hash(b"x"));
        assert_ne!(SeenSet::hash(b"x"), SeenSet::hash(b"y"));
    }
}

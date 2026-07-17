use std::collections::{HashSet, VecDeque};

use sha2::{Digest, Sha256};

/// Stores all the messages that have been seen so far up to a cap
pub struct SeenSet {
    cap: usize,
    set: HashSet<[u8; 32]>,
    order: VecDeque<[u8; 32]>,
}

impl SeenSet {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            set: HashSet::new(),
            order: VecDeque::new(),
        }
    }

    /// Add a message to the seen set
    pub fn insert(&mut self, hash: [u8; 32]) -> bool {
        if !self.set.insert(hash) {
            return false;
        }
        self.order.push_back(hash);

        // After the cap we assume that we wont see this msg again
        if self.set.len() > self.cap
            && let Some(oldest) = self.order.pop_front()
        {
            self.set.remove(&oldest);
        }
        true
    }

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

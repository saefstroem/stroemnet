#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
/// The two variants that constitute an action, claim or refund
pub(crate) enum Action {
    Claim,
    Refund,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
/// An action key containing a swap id and the relevant action
pub(crate) struct ActionKey {
    pub swap_id: [u8; 32],
    pub action: Action,
}

impl ActionKey {
    /// Create a new action key which is a claim key
    pub(crate) fn claim(swap_id: [u8; 32]) -> Self {
        Self {
            swap_id,
            action: Action::Claim,
        }
    }

    /// Create a new refund action key
    pub(crate) fn refund(swap_id: [u8; 32]) -> Self {
        Self {
            swap_id,
            action: Action::Refund,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::AHashMap;

    #[test]
    fn claim_and_refund_keys_are_distinct() {
        let c = ActionKey::claim([1u8; 32]);
        let r = ActionKey::refund([1u8; 32]);
        assert_ne!(c, r);
        assert_eq!(c.action, Action::Claim);
        assert_eq!(r.action, Action::Refund);
    }

    #[test]
    fn keys_are_usable_as_map_keys() {
        let mut m = AHashMap::new();
        m.insert(ActionKey::claim([1u8; 32]), 1);
        m.insert(ActionKey::refund([1u8; 32]), 2);
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&ActionKey::claim([1u8; 32])), Some(&1));
    }
}

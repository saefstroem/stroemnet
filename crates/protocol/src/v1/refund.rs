use borsh::{BorshDeserialize, BorshSerialize};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct RefundV1 {
    pub swap_id: [u8; 32],
}

impl RefundV1 {
    pub fn new(swap_id: [u8; 32]) -> Self {
        Self { swap_id }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn new_sets_fields() {
        assert_eq!(RefundV1::new([5u8; 32]).swap_id, [5u8; 32]);
    }

    #[test]
    fn borsh_roundtrip() {
        let r = RefundV1::new([5u8; 32]);
        let bytes = borsh::to_vec(&r).unwrap();
        assert_eq!(RefundV1::try_from_slice(&bytes).unwrap(), r);
    }
}

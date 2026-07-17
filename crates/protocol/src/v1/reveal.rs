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
pub struct RevealV1 {
    pub swap_id: [u8; 32],
    pub secret: [u8; 32],
}

impl RevealV1 {
    pub fn new(swap_id: [u8; 32], secret: [u8; 32]) -> Self {
        Self { swap_id, secret }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn new_sets_fields() {
        let r = RevealV1::new([3u8; 32], [4u8; 32]);
        assert_eq!(r.swap_id, [3u8; 32]);
        assert_eq!(r.secret, [4u8; 32]);
    }

    #[test]
    fn borsh_roundtrip() {
        let r = RevealV1::new([3u8; 32], [4u8; 32]);
        let bytes = borsh::to_vec(&r).unwrap();
        assert_eq!(RevealV1::try_from_slice(&bytes).unwrap(), r);
    }
}

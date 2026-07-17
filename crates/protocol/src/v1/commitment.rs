use borsh::{BorshDeserialize, BorshSerialize};

use super::addresses::AddressesV1;
use super::amount::AmountV1;

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
pub struct CommitmentV1 {
    pub swap_id: [u8; 32],
    pub addresses: AddressesV1,
    pub amount: AmountV1,
    pub secret_hash: [u8; 32],
    pub unlock_ts: u64,
    pub source: u8,
    pub destination: u8,
}

impl CommitmentV1 {
    pub fn new(
        swap_id: [u8; 32],
        addresses: AddressesV1,
        amount: AmountV1,
        secret_hash: [u8; 32],
        unlock_ts: u64,
        source: u8,
        destination: u8,
    ) -> Self {
        Self {
            swap_id,
            addresses,
            amount,
            secret_hash,
            unlock_ts,
            source,
            destination,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn sample() -> CommitmentV1 {
        CommitmentV1::new(
            [1u8; 32],
            AddressesV1::new("sender".into(), "receiver".into(), "sender_dest".into()),
            AmountV1::new("100.0".into(), 18),
            [2u8; 32],
            1700000000,
            1,
            0,
        )
    }

    #[test]
    fn new_sets_all_fields() {
        let c = sample();
        assert_eq!(c.swap_id, [1u8; 32]);
        assert_eq!(c.addresses.sender, "sender");
        assert_eq!(c.amount.value, "100.0");
        assert_eq!(c.secret_hash, [2u8; 32]);
        assert_eq!(c.unlock_ts, 1700000000);
        assert_eq!(c.source, 1);
        assert_eq!(c.destination, 0);
    }

    #[test]
    fn borsh_roundtrip() {
        let c = sample();
        let bytes = borsh::to_vec(&c).unwrap();
        assert_eq!(CommitmentV1::try_from_slice(&bytes).unwrap(), c);
    }
}

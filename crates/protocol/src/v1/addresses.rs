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
pub struct AddressesV1 {
    pub sender: String,
    pub receiver: String,
    pub sender_destination: String,
}

impl AddressesV1 {
    pub fn new(sender: String, receiver: String, sender_destination: String) -> Self {
        Self {
            sender,
            receiver,
            sender_destination,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_all_fields() {
        let a = AddressesV1::new("s".into(), "r".into(), "d".into());
        assert_eq!(a.sender, "s");
        assert_eq!(a.receiver, "r");
        assert_eq!(a.sender_destination, "d");
    }
}

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
pub struct AmountV1 {
    pub value: String,
    pub decimals: u8,
}

impl AmountV1 {
    pub fn new(value: String, decimals: u8) -> Self {
        Self { value, decimals }
    }

    pub fn at_least(&self, required: &AmountV1) -> bool {
        if self.decimals != required.decimals {
            return false;
        }
        match (self.value.parse::<u128>(), required.value.parse::<u128>()) {
            (Ok(have), Ok(need)) => have >= need,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_least_compares_value_with_matching_decimals() {
        let need = AmountV1::new("1000000000000000000".into(), 18);
        assert!(AmountV1::new("1000000000000000000".into(), 18).at_least(&need));
        assert!(AmountV1::new("2000000000000000000".into(), 18).at_least(&need));
        assert!(!AmountV1::new("1".into(), 18).at_least(&need));
    }

    #[test]
    fn at_least_rejects_decimals_mismatch_and_unparseable() {
        let need = AmountV1::new("100".into(), 8);
        assert!(!AmountV1::new("100".into(), 18).at_least(&need));
        assert!(!AmountV1::new("abc".into(), 8).at_least(&need));
        assert!(!AmountV1::new("100".into(), 8).at_least(&AmountV1::new("xyz".into(), 8)));
    }
}

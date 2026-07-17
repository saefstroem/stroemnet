use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{AmountV1, CommitmentV1};

#[derive(Clone, Debug)]
pub struct PendingClaim {
    pub secret: [u8; 32],
    pub expected_counter_chain: ChannelId,
    pub expected_secret_hash: [u8; 32],
    pub expected_destination_address: String,
    pub expected_amount_out: AmountV1,
}

pub fn pending_claim_matches(claim: &PendingClaim, detected: &CommitmentV1) -> bool {
    claim.expected_secret_hash == detected.secret_hash
        && detected.source == claim.expected_counter_chain as u8
        && stroemnet_handler::normalised_address_eq(
            claim.expected_counter_chain,
            &detected.addresses.receiver,
            &claim.expected_destination_address,
        )
        && detected.amount.at_least(&claim.expected_amount_out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stroemnet_protocol::v1::AddressesV1;

    fn claim() -> PendingClaim {
        PendingClaim {
            secret: [1u8; 32],
            expected_counter_chain: ChannelId::KaspaTn10,
            expected_secret_hash: [2u8; 32],
            expected_destination_address: "kaspa:dest".into(),
            expected_amount_out: AmountV1::new("100".into(), 8),
        }
    }

    fn detected() -> CommitmentV1 {
        CommitmentV1::new(
            [9u8; 32],
            AddressesV1::new("kaspa:sender".into(), "kaspa:dest".into(), "0xx".into()),
            AmountV1::new("150".into(), 8),
            [2u8; 32],
            0,
            ChannelId::KaspaTn10 as u8,
            ChannelId::EthereumSepolia as u8,
        )
    }

    #[test]
    fn matches_when_hash_chain_address_and_amount_satisfied() {
        assert!(pending_claim_matches(&claim(), &detected()));
    }

    #[test]
    fn rejects_on_hash_or_amount_mismatch() {
        let mut wrong_hash = detected();
        wrong_hash.secret_hash = [7u8; 32];
        assert!(!pending_claim_matches(&claim(), &wrong_hash));

        let mut underpaid = detected();
        underpaid.amount = AmountV1::new("50".into(), 8);
        assert!(!pending_claim_matches(&claim(), &underpaid));
    }
}

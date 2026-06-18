use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{AmountV1, CommitmentV1};

#[derive(Clone, Debug)]
/// A pending claim, initialized by the wasm user who is waiting for a
/// counter chain commitment with the correct secret hash and destination address
pub struct PendingClaim {
    pub secret: [u8; 32],
    pub expected_counter_chain: ChannelId,
    pub expected_secret_hash: [u8; 32],
    pub expected_destination_address: String,
    pub expected_amount_out: AmountV1,
}

/// A pending claim matches a detected commitment if the expected secret hash, counter chain, and destination address all match.
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

use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct Identity {
    pub id: [u8; 32],
}

impl Identity {
    #[allow(clippy::expect_used)]
    pub fn generate() -> Self {
        let mut id = [0u8; 32];
        getrandom_03::fill(&mut id).expect("OS RNG unavailable while generating peer identity");
        Self { id }
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("id", &hex::encode(self.id))
            .finish()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn proposal_digest(
    swap_id: [u8; 32],
    origin: u8,
    destination: u8,
    amount_in: &str,
    amount_out: &str,
    sender_destination_address: &str,
    lp_sender_address: &str,
    commit_unlock_offset_secs: u64,
    lp_block_confirmations: u64,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"stroemnet:proposal:v3");
    h.update(swap_id);
    h.update([origin, destination]);
    h.update((amount_in.len() as u64).to_le_bytes());
    h.update(amount_in.as_bytes());
    h.update((amount_out.len() as u64).to_le_bytes());
    h.update(amount_out.as_bytes());
    h.update((sender_destination_address.len() as u64).to_le_bytes());
    h.update(sender_destination_address.as_bytes());
    h.update((lp_sender_address.len() as u64).to_le_bytes());
    h.update(lp_sender_address.as_bytes());
    h.update(commit_unlock_offset_secs.to_le_bytes());
    h.update(lp_block_confirmations.to_le_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&h.finalize());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_digest() -> [u8; 32] {
        proposal_digest(
            [0xAA; 32],
            0,
            1,
            "1000000000000000000",
            "990000000000000000",
            "0xUserSendAddr",
            "0xLpSenderAddr",
            300,
            12,
        )
    }

    #[test]
    fn deterministic_for_same_inputs() {
        let a = base_digest();
        let b = base_digest();
        assert_eq!(a, b);
    }

    #[test]
    fn changes_when_swap_id_changes() {
        let a = base_digest();
        let b = proposal_digest(
            [0xBB; 32],
            0,
            1,
            "1000000000000000000",
            "990000000000000000",
            "0xUserSendAddr",
            "0xLpSenderAddr",
            300,
            12,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn changes_when_amount_out_changes() {
        let a = base_digest();
        let b = proposal_digest(
            [0xAA; 32],
            0,
            1,
            "1000000000000000000",
            "999999999999999999",
            "0xUserSendAddr",
            "0xLpSenderAddr",
            300,
            12,
        );
        assert_ne!(a, b, "tampering with amount_out must change the digest");
    }

    #[test]
    fn changes_when_lp_sender_address_changes() {
        let a = base_digest();
        let b = proposal_digest(
            [0xAA; 32],
            0,
            1,
            "1000000000000000000",
            "990000000000000000",
            "0xUserSendAddr",
            "0xDifferentLpSender",
            300,
            12,
        );
        assert_ne!(a, b, "lp_sender_address must be covered by the digest");
    }

    #[test]
    fn distinguishes_field_boundaries() {
        let a = proposal_digest([0; 32], 0, 0, "AB", "C", "", "", 0, 0);
        let b = proposal_digest([0; 32], 0, 0, "A", "BC", "", "", 0, 0);
        assert_ne!(
            a, b,
            "length-prefixed framing must distinguish AB|C from A|BC"
        );
    }

    #[test]
    fn changes_when_chain_direction_swaps() {
        let a = base_digest();
        let b = proposal_digest(
            [0xAA; 32],
            1,
            0,
            "1000000000000000000",
            "990000000000000000",
            "0xUserSendAddr",
            "0xLpSenderAddr",
            300,
            12,
        );
        assert_ne!(a, b, "swapping origin/destination must change the digest");
    }

    #[test]
    fn changes_when_lp_block_confirmations_changes() {
        let a = base_digest();
        let b = proposal_digest(
            [0xAA; 32],
            0,
            1,
            "1000000000000000000",
            "990000000000000000",
            "0xUserSendAddr",
            "0xLpSenderAddr",
            300,
            13,
        );
        assert_ne!(a, b, "lp_block_confirmations must be covered by the digest");
    }
}

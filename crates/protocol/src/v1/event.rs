use super::commitment::CommitmentV1;
use super::refund::RefundV1;
use super::reveal::RevealV1;

#[derive(Debug, Clone)]
pub enum ChainEvent {
    Commitment(CommitmentV1),
    Reveal(RevealV1),
    Refund(RefundV1),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_wrap_their_payloads() {
        assert!(matches!(
            ChainEvent::Reveal(RevealV1::new([0u8; 32], [0u8; 32])),
            ChainEvent::Reveal(_)
        ));
        assert!(matches!(
            ChainEvent::Refund(RefundV1::new([0u8; 32])),
            ChainEvent::Refund(_)
        ));
    }
}

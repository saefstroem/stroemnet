use alloy::rpc::types::Log;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{AddressesV1, AmountV1, ChainEvent, CommitmentV1, RefundV1, RevealV1};

use super::contracts::StroemHTLCV1;

/// Decodes an EVM log into a canonical Stroem ChainEvent
pub(super) fn decode_log(log: &Log, channel_id: ChannelId) -> Option<ChainEvent> {
    // Attempt to decode into a commitmentv1
    if let Ok(decoded) = log.log_decode::<StroemHTLCV1::Commitment>() {
        let commitment = CommitmentV1::new(
            decoded.inner.swapId.into(),
            AddressesV1::new(
                format!("{}", decoded.inner.sender),
                format!("{}", decoded.inner.receiver),
                String::from_utf8_lossy(&decoded.inner.sender_destination_address).to_string(),
            ),
            AmountV1::new(decoded.inner.amount.to_string(), channel_id.decimals()),
            decoded.inner.secretHash.into(),
            decoded.inner.timelock.to::<u64>(),
            channel_id as u8,
            decoded.inner.destination,
        );
        Some(ChainEvent::Commitment(commitment))

    // Attempt to decode into a claim event
    } else if let Ok(decoded) = log.log_decode::<StroemHTLCV1::Claim>() {
        let swap_id: [u8; 32] = decoded.inner.swapId.into();
        Some(ChainEvent::Reveal(RevealV1::new(
            swap_id,
            decoded.inner.secret.into(),
        )))
    // Attempt to decode into a refund event
    } else if let Ok(decoded) = log.log_decode::<StroemHTLCV1::Refund>() {
        let swap_id: [u8; 32] = decoded.inner.swapId.into();
        Some(ChainEvent::Refund(RefundV1::new(swap_id)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrecognized_log_decodes_to_none() {
        let log = Log::default();
        assert!(decode_log(&log, ChannelId::EthereumSepolia).is_none());
    }
}

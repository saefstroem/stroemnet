use alloy::rpc::types::Log;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{AddressesV1, AmountV1, ChainEvent, CommitmentV1, RefundV1, RevealV1};

use super::contracts::StroemHTLCV1;

/// Decodes an EVM log into a ChainEvent
/// if it matches the HTLC contract's Commitment, Claim, or Refund events
pub(super) fn decode_log(log: &Log, channel_id: ChannelId) -> Option<ChainEvent> {
    if let Ok(decoded) = log.log_decode::<StroemHTLCV1::Commitment>() {
        // Decode the Commitment event into our protocol's CommitmentV1 struct
        let commitment = CommitmentV1::new(
            decoded.inner.swapId.into(), // swap id
            AddressesV1::new(
                // Compute addresses struct as per protocol
                format!("{}", decoded.inner.sender),
                format!("{}", decoded.inner.receiver),
                String::from_utf8_lossy(&decoded.inner.sender_destination_address).to_string(),
            ),
            AmountV1::new(decoded.inner.amount.to_string(), channel_id.decimals()), // amount with correct decimals
            decoded.inner.secretHash.into(), // the secret hash of this swap
            decoded.inner.timelock.to::<u64>(), // when the swap can be refunded
            channel_id as u8,                // source chain id is this EVM chain
            decoded.inner.destination,       // destination chain id as specified in the event
        );
        Some(ChainEvent::Commitment(commitment))
    } else if let Ok(decoded) = log.log_decode::<StroemHTLCV1::Claim>() {
        // Decode the Claim event into our protocol's RevealV1 struct
        let swap_id: [u8; 32] = decoded.inner.swapId.into();
        Some(ChainEvent::Reveal(RevealV1::new(
            swap_id,
            decoded.inner.secret.into(),
        )))
    } else if let Ok(decoded) = log.log_decode::<StroemHTLCV1::Refund>() {
        // Decode the Refund event into our protocol's RefundV1 struct
        let swap_id: [u8; 32] = decoded.inner.swapId.into();
        Some(ChainEvent::Refund(RefundV1::new(swap_id)))
    } else {
        None
    }
}

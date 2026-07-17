use borsh::{BorshDeserialize, to_vec};

use super::error::RecordError;
use super::result::Result;
use crate::PersistedSwap;

/// Magic number
const MAGIC: u8 = 0x53;
/// We only have one version for now
const VERSION_1: u8 = 1;

/// An enum for either a persisted swap or corrupted record
pub(crate) enum DecodeOutcome {
    Current(Box<PersistedSwap>),
    Corrupt(RecordError),
}

/// Encode a swap to bytes
pub(crate) fn encode(swap: &PersistedSwap) -> Result<Vec<u8>> {
    let body = to_vec(swap)?;
    let mut out = Vec::with_capacity(body.len() + 2);
    out.push(MAGIC);
    out.push(VERSION_1);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decode, match it by the magic number and try to decode it version safe
pub(crate) fn decode(bytes: &[u8]) -> DecodeOutcome {
    match bytes.split_first() {
        Some((&MAGIC, rest)) => decode_versioned(rest),
        _ => DecodeOutcome::Corrupt(RecordError::Truncated),
    }
}

fn decode_versioned(rest: &[u8]) -> DecodeOutcome {
    // Decode it based on the matching version
    match rest.split_first() {
        Some((&VERSION_1, body)) => match PersistedSwap::try_from_slice(body) {
            Ok(swap) => DecodeOutcome::Current(Box::new(swap)),
            Err(e) => DecodeOutcome::Corrupt(e.into()),
        },
        Some((&other, _)) => DecodeOutcome::Corrupt(RecordError::UnknownVersion(other)),
        None => DecodeOutcome::Corrupt(RecordError::Truncated),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;
    use stroemnet_protocol::v1::{RefundV1, RevealV1};

    fn sample() -> PersistedSwap {
        PersistedSwap {
            script: None,
            pending_refund: Some((RefundV1::new([3u8; 32]), 1700)),
            pending_claim: Some(RevealV1::new([3u8; 32], [9u8; 32])),
            claim_attempt: None,
            refund_attempt: None,
        }
    }

    #[test]
    fn current_roundtrips() {
        let swap = sample();
        let bytes = encode(&swap).unwrap();
        assert_eq!(bytes[0], MAGIC);
        assert_eq!(bytes[1], VERSION_1);
        match decode(&bytes) {
            DecodeOutcome::Current(got) => assert_eq!(got.pending_refund, swap.pending_refund),
            _ => panic!("expected Current"),
        }
    }

    #[test]
    fn corrupt_and_unknown_version_are_flagged_not_dropped() {
        assert!(matches!(
            decode(&[MAGIC, 9, 0, 0, 0, 0]),
            DecodeOutcome::Corrupt(RecordError::UnknownVersion(9))
        ));
        assert!(matches!(
            decode(&[MAGIC]),
            DecodeOutcome::Corrupt(RecordError::Truncated)
        ));
    }
}

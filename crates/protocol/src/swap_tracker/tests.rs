use super::*;
use crate::channels::ChannelId;
use crate::v1::{AddressesV1, AmountV1};

const TEST_SECRET: [u8; 32] = [0xAB; 32];

fn test_secret_hash() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let out = Sha256::digest(TEST_SECRET);
    let mut a = [0u8; 32];
    a.copy_from_slice(&out);
    a
}

fn mock_init_commitment(unlock_ts: u64) -> CommitmentV1 {
    CommitmentV1 {
        swap_id: [0u8; 32],
        addresses: AddressesV1::new(
            "0xUserEthSender".to_string(),
            "0xUserEthReceiver".to_string(),
            "kaspa:user_dest_address".to_string(),
        ),
        amount: AmountV1::new("1000".to_string(), 18),
        secret_hash: test_secret_hash(),
        unlock_ts,
        source: ChannelId::EthereumSepolia as u8,
        destination: ChannelId::KaspaTn10 as u8,
    }
}

fn mock_counter_commitment(unlock_ts: u64) -> CommitmentV1 {
    CommitmentV1 {
        swap_id: [0u8; 32],
        addresses: AddressesV1::new(
            "kaspa:mm_kaspa_sender".to_string(),
            "kaspa:user_dest_address".to_string(),
            "0xUserEthReceiver".to_string(),
        ),
        amount: AmountV1::new("2000".to_string(), 18),
        secret_hash: test_secret_hash(),
        unlock_ts,
        source: ChannelId::KaspaTn10 as u8,
        destination: ChannelId::EthereumSepolia as u8,
    }
}

fn create_tracker() -> SwapTracker {
    SwapTracker::new()
}

#[test]
fn test_init_creates_record() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    let c = mock_init_commitment(now + 600);
    t.set_init_commitment(id, c).unwrap();

    let record = t.get_swap(&id).unwrap();
    assert_eq!(SwapTracker::stage(record), SwapStage::Initialized);
    assert!(record.counter_commitment.is_none());
    assert!(record.resolution.is_none());
}

#[test]
fn test_duplicate_swap_fails() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    let result = t.set_init_commitment(id, mock_init_commitment(now + 600));
    assert!(matches!(result, Err(SwapTrackerError::DuplicateSwap(_))));
}

#[test]
fn test_counter_commitment_transitions_to_lock() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();

    let record = t.get_swap(&id).unwrap();
    assert_eq!(SwapTracker::stage(record), SwapStage::Locked);
    assert!(record.counter_commitment.is_some());
}

#[test]
fn test_cannot_counter_without_init() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    let result = t.set_counter_commitment(id, mock_counter_commitment(now + 300));
    assert!(matches!(result, Err(SwapTrackerError::SwapNotFound(_))));
}

#[test]
fn test_cannot_counter_twice() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();
    let result = t.set_counter_commitment(id, mock_counter_commitment(now + 300));
    assert!(matches!(
        result,
        Err(SwapTrackerError::AlreadyCounterLocked(_))
    ));
}

#[test]
fn test_reveal_transitions_from_lock() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();
    t.set_revealed(id, [0xAB; 32]).unwrap();

    let record = t.get_swap(&id).unwrap();
    assert_eq!(SwapTracker::stage(record), SwapStage::Completed);
    assert_eq!(
        record.resolution.as_deref(),
        Some(&hex::encode([0xAB; 32])[..])
    );
}

#[test]
fn test_cannot_reveal_from_init() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    let result = t.set_revealed(id, [0xAB; 32]);
    assert!(matches!(result, Err(SwapTrackerError::NotCounterLocked(_))));
}

#[test]
fn test_cannot_reveal_twice() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();
    t.set_revealed(id, [0xAB; 32]).unwrap();
    let result = t.set_revealed(id, [0xCD; 32]);
    assert!(matches!(result, Err(SwapTrackerError::AlreadyResolved(_))));
}

#[test]
fn test_refund_transitions_from_lock() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();
    t.set_refunded(id).unwrap();

    let record = t.get_swap(&id).unwrap();
    assert_eq!(SwapTracker::stage(record), SwapStage::Refunded);
    assert_eq!(record.resolution.as_deref(), Some("refunded"));
}

#[test]
fn test_cannot_refund_from_init() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    let result = t.set_refunded(id);
    assert!(matches!(result, Err(SwapTrackerError::NotCounterLocked(_))));
}

#[test]
fn test_cannot_refund_after_reveal() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();
    t.set_revealed(id, [0xAB; 32]).unwrap();
    let result = t.set_refunded(id);
    assert!(matches!(result, Err(SwapTrackerError::AlreadyResolved(_))));
}

#[test]
fn test_data_preserved_in_reveal() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();
    t.set_revealed(id, [0xAB; 32]).unwrap();

    let record = t.get_swap(&id).unwrap();
    assert_eq!(record.init_commitment.swap_id, [0u8; 32]);
    assert_eq!(
        record.counter_commitment.as_ref().unwrap().swap_id,
        [0u8; 32]
    );
    assert_eq!(
        record.resolution.as_deref(),
        Some(&hex::encode([0xAB; 32])[..])
    );
}

#[test]
fn test_data_preserved_in_refund() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();
    t.set_refunded(id).unwrap();

    let record = t.get_swap(&id).unwrap();
    assert_eq!(record.init_commitment.swap_id, [0u8; 32]);
    assert_eq!(
        record.counter_commitment.as_ref().unwrap().swap_id,
        [0u8; 32]
    );
    assert_eq!(record.resolution.as_deref(), Some("refunded"));
}

#[test]
fn test_is_expired_for_init() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now - 100))
        .unwrap();
    assert!(t.is_expired(&id));
}

#[test]
fn test_is_not_expired_for_future_unlock() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    assert!(!t.is_expired(&id));
}

#[test]
fn test_is_expired_checks_counter_unlock_ts() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now - 50))
        .unwrap();
    assert!(t.is_expired(&id));
}

#[test]
fn test_time_until_init_refund() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 300))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 150))
        .unwrap();
    let time_left = t.time_until_init_refund(&id).unwrap();
    assert!(time_left >= 299 && time_left <= 300);
}

#[test]
fn test_time_until_ctpy_refund() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 150))
        .unwrap();
    let time_left = t.time_until_ctpy_refund(&id).unwrap();
    assert!(time_left >= 149 && time_left <= 150);
}

#[test]
fn test_time_queries_return_none_for_reveal() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();
    t.set_revealed(id, [0xAB; 32]).unwrap();
    assert!(t.time_until_init_refund(&id).is_none());
    assert!(t.time_until_ctpy_refund(&id).is_none());
}

#[test]
fn test_time_queries_return_none_for_refund() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();
    t.set_refunded(id).unwrap();
    assert!(t.time_until_init_refund(&id).is_none());
    assert!(t.time_until_ctpy_refund(&id).is_none());
}

#[test]
fn test_cleanup_removes_old_completed_swaps() {
    let mut t = create_tracker();
    let now = SwapTracker::now();

    let id1 = [1u8; 32];
    t.set_init_commitment(id1, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id1, mock_counter_commitment(now + 300))
        .unwrap();
    t.set_revealed(id1, [0xAB; 32]).unwrap();

    let id2 = [2u8; 32];
    t.set_init_commitment(id2, mock_init_commitment(now + 600))
        .unwrap();

    t.cleanup_old_swaps(300);
    assert!(t.get_swap(&id1).is_some());
    assert!(t.get_swap(&id2).is_some());

    t.cleanup_old_swaps(0);
    assert!(t.get_swap(&id1).is_none());
    assert!(t.get_swap(&id2).is_some());
}

#[test]
fn test_multiple_swaps_independent() {
    let mut t = create_tracker();
    let now = SwapTracker::now();

    let id1 = [1u8; 32];
    let id2 = [2u8; 32];
    t.set_init_commitment(id1, mock_init_commitment(now + 600))
        .unwrap();
    t.set_init_commitment(id2, mock_init_commitment(now + 600))
        .unwrap();

    assert!(t.get_swap(&id1).is_some());
    assert!(t.get_swap(&id2).is_some());

    t.set_counter_commitment(id1, mock_counter_commitment(now + 300))
        .unwrap();

    assert_eq!(
        SwapTracker::stage(t.get_swap(&id1).unwrap()),
        SwapStage::Locked
    );
    assert_eq!(
        SwapTracker::stage(t.get_swap(&id2).unwrap()),
        SwapStage::Initialized
    );
}

#[test]
fn test_cannot_hijack_with_wrong_receiver() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();

    let malicious = CommitmentV1 {
        swap_id: [0u8; 32],
        addresses: AddressesV1::new(
            "kaspa:mm_kaspa_sender".to_string(),
            "kaspa:WRONG_receiver".to_string(),
            "kaspa:user_dest_address".to_string(),
        ),
        amount: AmountV1::new("2000".to_string(), 18),
        secret_hash: [0xCC; 32],
        unlock_ts: now + 300,
        source: ChannelId::KaspaTn10 as u8,
        destination: ChannelId::EthereumSepolia as u8,
    };

    let result = t.set_counter_commitment(id, malicious);
    assert!(result.is_err());
    if let Err(SwapTrackerError::ValidationFailed { reason, .. }) = result {
        assert!(reason.contains("receiver does not match"));
    }
    assert_eq!(
        SwapTracker::stage(t.get_swap(&id).unwrap()),
        SwapStage::Initialized
    );
}

#[test]
fn test_cannot_hijack_with_wrong_sender_destination() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();

    let malicious = CommitmentV1 {
        swap_id: [0u8; 32],
        addresses: AddressesV1::new(
            "kaspa:mm_kaspa_sender".to_string(),
            "kaspa:user_dest_address".to_string(),
            "0xWRONG_sender_dest".to_string(),
        ),
        amount: AmountV1::new("2000".to_string(), 18),
        secret_hash: [0xCC; 32],
        unlock_ts: now + 300,
        source: ChannelId::KaspaTn10 as u8,
        destination: ChannelId::EthereumSepolia as u8,
    };

    let result = t.set_counter_commitment(id, malicious);
    assert!(result.is_err());
    if let Err(SwapTrackerError::ValidationFailed { reason, .. }) = result {
        assert!(reason.contains("sender_destination does not match"));
    }
    assert_eq!(
        SwapTracker::stage(t.get_swap(&id).unwrap()),
        SwapStage::Initialized
    );
}

#[test]
fn test_cannot_hijack_with_both_addresses_swapped() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();

    let malicious = CommitmentV1 {
        swap_id: [0u8; 32],
        addresses: AddressesV1::new(
            "kaspa:mm_kaspa_sender".to_string(),
            "0xUserEthReceiver".to_string(),
            "kaspa:user_dest_address".to_string(),
        ),
        amount: AmountV1::new("2000".to_string(), 18),
        secret_hash: [0xCC; 32],
        unlock_ts: now + 300,
        source: ChannelId::KaspaTn10 as u8,
        destination: ChannelId::EthereumSepolia as u8,
    };

    let result = t.set_counter_commitment(id, malicious);
    assert!(result.is_err());
    assert_eq!(
        SwapTracker::stage(t.get_swap(&id).unwrap()),
        SwapStage::Initialized
    );
}

#[test]
fn test_cannot_hijack_with_different_secret_hash() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();

    let malicious = CommitmentV1 {
        swap_id: [0u8; 32],
        addresses: AddressesV1::new(
            "kaspa:mm_kaspa_sender".to_string(),
            "kaspa:user_dest_address".to_string(),
            "0xUserEthReceiver".to_string(),
        ),
        amount: AmountV1::new("2000".to_string(), 18),
        secret_hash: [0xDD; 32],
        unlock_ts: now + 300,
        source: ChannelId::KaspaTn10 as u8,
        destination: ChannelId::EthereumSepolia as u8,
    };

    let result = t.set_counter_commitment(id, malicious);
    assert!(result.is_err());
    if let Err(SwapTrackerError::ValidationFailed { reason, .. }) = result {
        assert!(reason.contains("secret_hash"));
    }
    assert_eq!(
        SwapTracker::stage(t.get_swap(&id).unwrap()),
        SwapStage::Initialized
    );
}

#[test]
fn test_cannot_hijack_with_different_swap_id() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();

    let malicious = CommitmentV1 {
        swap_id: [1u8; 32],
        addresses: AddressesV1::new(
            "kaspa:mm_kaspa_sender".to_string(),
            "kaspa:user_dest_address".to_string(),
            "0xUserEthReceiver".to_string(),
        ),
        amount: AmountV1::new("2000".to_string(), 18),
        secret_hash: [0xCC; 32],
        unlock_ts: now + 300,
        source: ChannelId::KaspaTn10 as u8,
        destination: ChannelId::EthereumSepolia as u8,
    };

    let result = t.set_counter_commitment(id, malicious);
    assert!(result.is_err());
    if let Err(SwapTrackerError::ValidationFailed { reason, .. }) = result {
        assert!(reason.contains("swap_id"));
    }
    assert_eq!(
        SwapTracker::stage(t.get_swap(&id).unwrap()),
        SwapStage::Initialized
    );
}

#[test]
fn test_cannot_lock_with_wrong_source_destination_mirror() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();

    let bad_mirror = CommitmentV1 {
        swap_id: [0u8; 32],
        addresses: AddressesV1::new(
            "kaspa:mm_kaspa_sender".to_string(),
            "kaspa:user_dest_address".to_string(),
            "0xUserEthReceiver".to_string(),
        ),
        amount: AmountV1::new("2000".to_string(), 18),
        secret_hash: test_secret_hash(),
        unlock_ts: now + 300,
        source: ChannelId::KaspaTn10 as u8,
        destination: ChannelId::KaspaTn10 as u8,
    };

    let result = t.set_counter_commitment(id, bad_mirror);
    assert!(result.is_err());
    if let Err(SwapTrackerError::ValidationFailed { reason, .. }) = result {
        assert!(reason.contains("source/destination mismatch"));
    }
    assert_eq!(
        SwapTracker::stage(t.get_swap(&id).unwrap()),
        SwapStage::Initialized
    );
}

#[test]
fn test_valid_counter_succeeds_after_failed_attacks() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();

    let _ = t.set_counter_commitment(
        id,
        CommitmentV1 {
            swap_id: [0u8; 32],
            addresses: AddressesV1::new(
                "kaspa:mm_kaspa_sender".to_string(),
                "kaspa:WRONG_receiver".to_string(),
                "0xUserEthReceiver".to_string(),
            ),
            amount: AmountV1::new("2000".to_string(), 18),
            secret_hash: [0xCC; 32],
            unlock_ts: now + 300,
            source: ChannelId::KaspaTn10 as u8,
            destination: ChannelId::EthereumSepolia as u8,
        },
    );

    let valid = mock_counter_commitment(now + 300);
    assert!(t.set_counter_commitment(id, valid).is_ok());
}

#[test]
fn test_all_swaps_returns_all() {
    let mut t = create_tracker();
    let now = SwapTracker::now();
    t.set_init_commitment([1u8; 32], mock_init_commitment(now + 600))
        .unwrap();
    t.set_init_commitment([2u8; 32], mock_init_commitment(now + 600))
        .unwrap();
    let all: Vec<_> = t.all_swaps().collect();
    assert_eq!(all.len(), 2);
}

#[test]
fn test_get_swap_returns_none_for_missing() {
    let t = create_tracker();
    assert!(t.get_swap(&[99u8; 32]).is_none());
}

#[test]
fn test_set_revealed_rejects_wrong_preimage() {
    let mut t = create_tracker();
    let id = [1u8; 32];
    let now = SwapTracker::now();
    t.set_init_commitment(id, mock_init_commitment(now + 600))
        .unwrap();
    t.set_counter_commitment(id, mock_counter_commitment(now + 300))
        .unwrap();

    let result = t.set_revealed(id, [0x99; 32]);
    assert!(matches!(
        result,
        Err(SwapTrackerError::SecretHashMismatch { .. })
    ));

    t.set_revealed(id, TEST_SECRET).unwrap();
    let record = t.get_swap(&id).unwrap();
    assert_eq!(SwapTracker::stage(record), SwapStage::Completed);
}

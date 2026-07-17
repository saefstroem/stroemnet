use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{RefundV1, RevealV1};

use crate::{AttemptState, MaybeSend, UtxoScript};

pub trait CursorStore: MaybeSend {
    fn load(&self, channel_id: ChannelId) -> Option<Vec<u8>>;
    fn save(&self, channel_id: ChannelId, cursor: &[u8]);
}

#[derive(Debug, Clone, Default, borsh::BorshSerialize, borsh::BorshDeserialize)]
/// A swap that is persisted to disk
pub struct PersistedSwap {
    pub script: Option<UtxoScript>,
    pub pending_refund: Option<(RefundV1, u64)>,
    pub pending_claim: Option<RevealV1>,
    pub claim_attempt: Option<AttemptState>,
    pub refund_attempt: Option<AttemptState>,
}

impl PersistedSwap {
    pub fn is_empty(&self) -> bool {
        self.script.is_none()
            && self.pending_refund.is_none()
            && self.pending_claim.is_none()
            && self.claim_attempt.is_none()
            && self.refund_attempt.is_none()
    }
}

pub trait SwapStore: MaybeSend {
    fn load_channel(&self, channel_id: ChannelId) -> Vec<([u8; 32], Vec<u8>)>;
    fn save(&self, channel_id: ChannelId, swap_id: [u8; 32], record: &[u8]);
    fn delete(&self, channel_id: ChannelId, swap_id: [u8; 32]);
    fn quarantine(&self, _channel_id: ChannelId, _swap_id: [u8; 32], _raw: &[u8], _reason: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_persisted_swap_is_empty() {
        assert!(PersistedSwap::default().is_empty());
        let s = PersistedSwap {
            pending_claim: Some(RevealV1 {
                swap_id: [0u8; 32],
                secret: [0u8; 32],
            }),
            ..Default::default()
        };
        assert!(!s.is_empty());

        let only_attempt = PersistedSwap {
            claim_attempt: Some(AttemptState::new(0)),
            ..Default::default()
        };
        assert!(!only_attempt.is_empty());
    }
}

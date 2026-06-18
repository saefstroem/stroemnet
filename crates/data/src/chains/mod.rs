pub(crate) mod evm;
pub(crate) mod kaspa;

use serde_json::Value;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{ChainEvent, RefundV1};

use std::sync::Arc;

use crate::chains::evm::Evm;
use crate::chains::kaspa::Kaspa;
use crate::{ChainDataBuffer, CursorStore, Result};

/// A factory function to build a chain data buffer based on the channel ID and configuration provided.
pub(crate) async fn build_buffer(
    channel_id: ChannelId,
    cfg: &Value,
    lp_key: Option<String>,
    cursor_store: Option<Arc<dyn CursorStore>>,
) -> Result<Box<dyn ChainDataBuffer>> {
    match channel_id {
        ChannelId::EthereumSepolia | ChannelId::IgraGalleon => Ok(Box::new(
            Evm::connect(channel_id, cfg, lp_key, cursor_store).await?,
        )),
        ChannelId::KaspaTn10 => Ok(Box::new(
            Kaspa::connect(channel_id, cfg, lp_key, cursor_store).await?,
        )),
    }
}

/// Used to either queue or dequeue a refund event
/// by matching the inner chain events to the pending refunds and the commitment events that trigger them
pub(crate) fn queue_dequeue_refund_event(
    pending: &mut Vec<(RefundV1, u64)>,
    event: &ChainEvent,
    participate_ccr: bool,
) {
    match event {
        ChainEvent::Commitment(c) => {
            // Since this is a commitment we track it if we participate in ccr
            // and if we already dont have it
            if participate_ccr && !pending.iter().any(|(r, _)| r.swap_id == c.swap_id) {
                pending.push((RefundV1::new(c.swap_id), c.unlock_ts));
            }
        }
        // if we see any of this we remove any tracked refunds for the swap id since
        // it means the swap has been resolved on-chain
        ChainEvent::Reveal(r) => pending.retain(|(p, _)| p.swap_id != r.swap_id),
        ChainEvent::Refund(r) => pending.retain(|(p, _)| p.swap_id != r.swap_id),
    }
}

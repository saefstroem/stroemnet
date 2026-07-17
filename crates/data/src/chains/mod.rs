pub(crate) mod evm;
pub(crate) mod kaspa;
pub(crate) mod net;
pub(crate) mod record;
pub(crate) mod settlement;

use serde_json::Value;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{ChainEvent, RefundV1, RevealV1};

use std::sync::Arc;

use crate::chains::evm::Evm;
use crate::chains::kaspa::Kaspa;
use crate::chains::settlement::SettlementMetrics;
use crate::{ChainDataBuffer, CursorStore, Result, SwapStore};

/// Build data buffer for a particular channel id given some parameters
pub(crate) async fn build_buffer(
    channel_id: ChannelId,                       // channel id
    cfg: &Value,                                 // configuration
    lp_key: Option<String>,                      // private key if they are lp or will do ccr
    cursor_store: Option<Arc<dyn CursorStore>>,  // trait backed store
    swap_store: Option<Arc<dyn SwapStore>>,      // trait backed store
    metrics: Option<Arc<dyn SettlementMetrics>>, // stats
) -> Result<Box<dyn ChainDataBuffer>> {
    match channel_id {
        ChannelId::EthereumSepolia | ChannelId::IgraGalleon => Ok(Box::new(
            // these are all evm chains
            Evm::connect(channel_id, cfg, lp_key, cursor_store, swap_store, metrics).await?,
        )),
        ChannelId::KaspaTn10 => Ok(Box::new(
            // kaspa tn10 is a kaspa network
            Kaspa::connect(channel_id, cfg, lp_key, cursor_store, swap_store, metrics).await?,
        )),
    }
}

///  Queue or deqeueu a refund event based on the received chain event
pub(crate) fn queue_dequeue_refund_event(
    pending: &mut Vec<(RefundV1, u64)>,
    event: &ChainEvent,
    participate_ccr: bool,
) {
    match event {
        ChainEvent::Commitment(c) => {
            // means we need to schedule refund
            if participate_ccr && !pending.iter().any(|(r, _)| r.swap_id == c.swap_id) {
                pending.push((RefundV1::new(c.swap_id), c.unlock_ts));
            }
        }
        // Either of these means that its finalized and we should remove the scheduling here.
        ChainEvent::Reveal(r) => pending.retain(|(p, _)| p.swap_id != r.swap_id),
        ChainEvent::Refund(r) => pending.retain(|(p, _)| p.swap_id != r.swap_id),
    }
}

/// Converts a chain event to its corresponding swap id
pub(crate) fn event_swap_id(event: &ChainEvent) -> [u8; 32] {
    match event {
        ChainEvent::Commitment(c) => c.swap_id,
        ChainEvent::Reveal(r) => r.swap_id,
        ChainEvent::Refund(r) => r.swap_id,
    }
}

/// Adds a reveal to the pending queue if it does not exist there
pub(crate) fn push_pending_claim(pending: &mut Vec<RevealV1>, reveal: &RevealV1) {
    if !pending.iter().any(|c| c.swap_id == reveal.swap_id) {
        pending.push(reveal.clone());
    }
}

/// Adds a refund to pending if its not there yet
pub(crate) fn push_pending_refund(
    pending: &mut Vec<(RefundV1, u64)>,
    swap_id: [u8; 32],
    unlock: u64,
) -> bool {
    if pending.iter().any(|(p, _)| p.swap_id == swap_id) {
        false
    } else {
        pending.push((RefundV1::new(swap_id), unlock));
        true
    }
}

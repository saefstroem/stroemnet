use std::sync::Arc;

use stroemnet_data::ChainDataSink;
use stroemnet_handler::{Effect, Handler};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::ChainEvent;

#[cfg(target_arch = "wasm32")]
use crate::coordinator::Coordinator;
#[cfg(target_arch = "wasm32")]
use crate::{PendingClaim, SwapStage, pending_claim_matches};
#[cfg(target_arch = "wasm32")]
use ahash::AHashMap;
#[cfg(target_arch = "wasm32")]
use tokio::sync::RwLock;

/// Applies an event to the system
pub(super) async fn apply_event(
    sink: &Arc<ChainDataSink>, // the data sink used to communicate with the blockchains
    handler: &Arc<Handler>,    // the handler that handles all swaps, tracks swaps
    #[cfg(target_arch = "wasm32")] coordinator: &Arc<Coordinator>,
    #[cfg(target_arch = "wasm32")] pending_claims: &Arc<RwLock<AHashMap<[u8; 32], PendingClaim>>>,
    source: ChannelId,
    event: ChainEvent,
) {
    // If this is a commitment
    #[cfg(target_arch = "wasm32")]
    if let ChainEvent::Commitment(c) = &event {
        let own_deposit = pending_claims
            .read()
            .await
            .get(&c.swap_id)
            .map(|claim| source != claim.expected_counter_chain)
            .unwrap_or(false);
        if own_deposit {
            coordinator.emit_status(c.swap_id, SwapStage::Locked);
        }
    }

    // Get the clock for all registered chains
    let clock = sink.chain_clock();

    // Compute the effects that arise from this event
    let effects = match handler.on_chain_event(source, event, &clock).await {
        Ok(effects) => effects,
        Err(e) => {
            tracing::warn!("on_chain_event: {e}");
            return;
        }
    };

    // Go over all effects
    for effect in effects {
        match effect {
            // If the effect is to broadcast to a specific event
            // then we broadcast it
            Effect::Broadcast(channel_id, ev) => {
                if let Err(e) = sink.broadcast_event(channel_id, &ev).await {
                    tracing::error!("broadcast_event to {channel_id}: {e}");
                }
            }
            // If its a reveal transmission then it means
            // we need to transmit the secret
            // this is only used in wasm since we are eagerly waiting
            // to submit the secret
            Effect::TransmitReveal(detected) => {
                #[cfg(target_arch = "wasm32")]
                {
                    let c = &detected;
                    let claim_to_fire = {
                        let mut map = pending_claims.write().await;
                        match map.remove(&c.swap_id) {
                            // ensures that the claim we computed that needs to
                            // be revealed is matching with the one we have stored in the
                            // storage
                            Some(claim) if pending_claim_matches(&claim, c) => Some(claim),
                            Some(claim) => {
                                map.insert(c.swap_id, claim);
                                None
                            }
                            None => None,
                        }
                    };

                    // If it passed all checks we should broadcast it on the p2p net
                    if let Some(claim) = claim_to_fire {
                        coordinator.spawn_reveal_broadcast(c.swap_id, claim);
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                let _ = detected;
            }
        }
    }
}

use std::sync::Arc;

use stroemnet_data::ChainDataSink;
use stroemnet_handler::Handler;
use stroemnet_p2p::P2p;
use stroemnet_p2p::wire::message::{P2pMsg, ScriptAnnounce};
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;

use stroemnet_p2p::network::NetEvent;

use super::apply::apply_event;
use crate::coordinator::Coordinator;

#[cfg(target_arch = "wasm32")]
use crate::PendingClaim;
#[cfg(target_arch = "wasm32")]
use ahash::AHashMap;
#[cfg(target_arch = "wasm32")]
use tokio::sync::RwLock;

/// How often to poll events from the sink
const DRIVER_TICK_MS: u64 = 1000;

/// Spawns the main processing loop and coordinator loop
pub(super) fn spawn_processing(
    coordinator: Arc<Coordinator>,
    sink: Arc<ChainDataSink>,
    handler: Arc<Handler>,
    network: Arc<P2p>,
    net_events: futures::channel::mpsc::Receiver<NetEvent>,
    #[cfg(target_arch = "wasm32")] pending_claims: Arc<RwLock<AHashMap<[u8; 32], PendingClaim>>>,
    #[cfg(not(target_arch = "wasm32"))] tasks: &mut Vec<JoinHandle<()>>,
) {
    let handle = coordinator.clone().spawn_dispatch_loop(net_events);
    #[cfg(not(target_arch = "wasm32"))]
    tasks.extend(handle);
    #[cfg(target_arch = "wasm32")]
    let _ = handle;
    spawn_driver_loop(
        sink,
        handler,
        network,
        #[cfg(target_arch = "wasm32")]
        coordinator,
        #[cfg(target_arch = "wasm32")]
        pending_claims,
        #[cfg(not(target_arch = "wasm32"))]
        tasks,
    );
}

pub(super) fn spawn_driver_loop(
    sink: Arc<ChainDataSink>,
    handler: Arc<Handler>,
    network: Arc<P2p>,
    #[cfg(target_arch = "wasm32")] coordinator: Arc<Coordinator>,
    #[cfg(target_arch = "wasm32")] pending_claims: Arc<RwLock<AHashMap<[u8; 32], PendingClaim>>>,
    #[cfg(not(target_arch = "wasm32"))] tasks: &mut Vec<JoinHandle<()>>,
) {
    let fut = async move {
        loop {
            // infinitely loop the next finalized chunk of data which can span across many blocks
            match sink.finalized_chunk().await {
                Ok(events) => {
                    for (source, event) in events {
                        // for each of the events, apply them one by one to our state.
                        apply_event(
                            &sink,
                            &handler,
                            #[cfg(target_arch = "wasm32")]
                            &coordinator,
                            #[cfg(target_arch = "wasm32")]
                            &pending_claims,
                            source,
                            event,
                        )
                        .await;
                    }
                }
                Err(e) => tracing::warn!("finalized_chunk: {e}"),
            }
            // Compute all new utxo scripts that we have, and broadcast them over p2p
            // todo: this may lead to excessive data transmission
            for a in sink.take_utxo_script_announcements() {
                let msg = P2pMsg::ScriptAnnounce(ScriptAnnounce {
                    address: a.address,
                    swap_id: a.swap_id,
                    redeem_script: a.script.redeem_script,
                    unlock_ts: a.script.unlock_ts,
                    deposit_target: a.script.deposit_target,
                });
                if let Err(e) = network.broadcast(&msg).await {
                    tracing::warn!("script-announce broadcast failed: {e}");
                }
            }
            stroemnet_protocol::sleep_ms(DRIVER_TICK_MS).await;
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    tasks.push(tokio::spawn(fut));
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(fut);
}

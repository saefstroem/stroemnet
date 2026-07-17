#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use tokio::sync::Semaphore;

#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;

use stroemnet_p2p::P2p;

#[cfg(not(target_arch = "wasm32"))]
/// Spawn connection handler that binds to a listening address
/// and handles incoming peer connections
pub(crate) async fn spawn_accept(
    bind_addr: SocketAddr, // the address where we are listening
    network: Arc<P2p>, // the p2p network object
    counter: Arc<AtomicUsize>, // peer count trackers
    tasks: &mut Vec<JoinHandle<()>>, // a mutable shared vector where we store tasks to cancel
) {
    use super::read::read_from_peer_tracked;

    // Listen and get an rx receiver
    match network.clone().listen(bind_addr).await {
        Ok(mut inbound_rx) => {
            tracing::info!("P2P listener bound on {bind_addr}");
            let net_for_inbound = network.clone();
            let counter_for_inbound = counter.clone();

            // Limit the maximum amount of connections at a time
            let limiter = Arc::new(Semaphore::new(network.config.max_inbound));
            // Spawn a task and add it to the DS
            tasks.push(tokio::spawn(async move {
                // Wait for an inbound connection
                while let Some(peer) = inbound_rx.recv().await {
                    let url = peer.url.clone();
                    // Limit number of connections
                    let Ok(permit) = limiter.clone().try_acquire_owned() else {
                        tracing::warn!("inbound peer {url} rejected: max_inbound reached");
                        continue;
                    };

                    // Clone the relevant data needed for handling this connections
                    let events_tx = net_for_inbound.events_tx.clone();
                    let net2 = net_for_inbound.clone();
                    let c2 = counter_for_inbound.clone();
                    tokio::spawn(async move { // spawn a new task holding the permit
                        // and handle the connection
                        let _permit = permit;
                        read_from_peer_tracked(net2, peer, events_tx, c2, true).await;
                        tracing::info!("inbound peer {url} disconnected");
                    });
                }
            }));
        }
        Err(e) => tracing::warn!("Could not bind P2P listener on {bind_addr}: {e}"),
    }
}

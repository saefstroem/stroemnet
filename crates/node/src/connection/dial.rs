#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

#[cfg(not(target_arch = "wasm32"))]
use parking_lot::Mutex;

use super::read::read_from_peer_tracked;
use stroemnet_p2p::P2p;
use stroemnet_protocol::sleep_secs;

#[cfg(not(target_arch = "wasm32"))]
struct InFlightGuard {
    set: Arc<Mutex<std::collections::HashSet<String>>>,
    url_norm: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.set.lock().remove(&self.url_norm);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn spawn_addr_dial_driver(
    network: Arc<P2p>,
    url: String,
    counter: Arc<AtomicUsize>,
    in_flight: Arc<Mutex<HashSet<String>>>,
    url_norm: String,
) {
    // Create a guard which the in flight set
    let guard = InFlightGuard {
        set: in_flight,
        url_norm,
    };
    stroemnet_protocol::spawn(async move {
        // the guard has a drop immpl meaning that when
        // it is dropped the peer that we are dialling will be removed from
        // the dedup set
        let _guard = guard;
        // Attempt to dial the peer with a 10 second timeout
        let dialed =
            tokio::time::timeout(std::time::Duration::from_secs(10), network.dial(&url)).await;
        match dialed {
            Ok(Ok(peer)) => {
                // The peer is connected so now we should read from the peer 
                let events_tx = network.events_tx.clone();
                read_from_peer_tracked(network, peer, events_tx, counter, false).await;
            }
            Ok(Err(e)) => tracing::debug!("discovery: dial {url} failed: {e}"),
            Err(_) => tracing::debug!("discovery: dial {url} timed out"),
        }
    });
}

/// Spawn a task and attempt to spawn a redial for bootstrap peers
pub(crate) fn spawn_bootstrap_with_counter(network: Arc<P2p>, counter: Arc<AtomicUsize>) {
    let urls = network.config.bootstrap_peers.clone();
    if urls.is_empty() {
        tracing::info!("no bootstrap peers configured");
        return;
    }
    // Go over all urls
    for url in urls {
        let net = network.clone();
        let c = counter.clone();
        stroemnet_protocol::spawn(async move {
            // redial the bootstrap node and attempt to establish connection
            bootstrap_redial_loop(net, url, c).await;
        });
    }
}

/// A loop to try to contact a bootstrap node at least 5 times
async fn bootstrap_redial_loop(network: Arc<P2p>, url: String, counter: Arc<AtomicUsize>) {
    let url_norm = stroemnet_p2p::normalize_listen_addr(&url);
    loop {
        if network.is_connected_peer(&url_norm).await {
            sleep_secs(30).await;
            continue;
        }
        let Some(peer) = network.dial_with_backoff(&url).await else {
            tracing::warn!("bootstrap: giving up on {url} after 5 failed attempts");
            return;
        };
        let events_tx = network.events_tx.clone();
        read_from_peer_tracked(network.clone(), peer, events_tx, counter.clone(), false).await;
        sleep_secs(1).await;
    }
}

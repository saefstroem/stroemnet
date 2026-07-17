use parking_lot::Mutex;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use stroemnet_handler::Handler;
use stroemnet_p2p::P2p;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::connection::{spawn_accept, spawn_addr_dial_driver};
use crate::coordinator::Role;
use crate::oracle::Oracle;
use crate::result::Result;

/// Maximum amount of dials to new peers
const MAX_INFLIGHT_DIALS: usize = 64;

pub(super) fn spawn_discovery_drainer(
    network: Arc<P2p>,
    peer_count: Arc<AtomicUsize>,
    mut rx: mpsc::UnboundedReceiver<String>,
    tasks: &mut Vec<JoinHandle<()>>,
) {
    let drainer = async move {
        // Create a DS to hold which peers we are currently calling to prevent calling the same peer
        // multiple times at the same time
        let in_flight: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        while let Some(url) = rx.recv().await {
            // as we get a new peer to dial
            let url_norm = stroemnet_p2p::normalize_listen_addr(&url);
            // If they are already connected we are not interested in dialling
            if network.is_connected_peer(&url_norm).await {
                continue;
            }
            {
                // Ensure that we have are within the max amount of peers
                // and also that we are not already dialling this peer
                let mut set = in_flight.lock();
                if set.len() >= MAX_INFLIGHT_DIALS || !set.insert(url_norm.clone()) {
                    continue;
                }
            }
            // Spawn the driver that will attempt to dial this peer and establish comms
            spawn_addr_dial_driver(
                network.clone(),
                url,
                peer_count.clone(),
                in_flight.clone(),
                url_norm,
            );
        }
        tracing::info!("discovery: dial channel closed, dialer exiting");
    };
    tasks.push(tokio::spawn(drainer));
}

/// Spawns services that are mainly used in native code execution
/// i.e. for LP nodes.
pub(super) async fn spawn_native_services(
    handler: Arc<Handler>,                    // handler which tracks swaps
    network: Arc<P2p>,                        // the p2p network entrypoint
    peer_count: Arc<AtomicUsize>,             // number of connected peers
    role: Role,                               // either we are lp or observer node
    oracle_interval_secs: u64,                // how often to update the prices of oracle
    bind_addr: Option<SocketAddr>,            // where to listen for data
    dial_rx: mpsc::UnboundedReceiver<String>, // where we listen for new peers that we need to dial
    tasks: &mut Vec<JoinHandle<()>>,          // all tasks that we are running
) -> Result<()> {
    // Spawn the task that is responsible for discovering new peers and communicating with them
    spawn_discovery_drainer(network.clone(), peer_count.clone(), dial_rx, tasks);
    if role == Role::Lp {
        // If we are an lp we also create a price oracle and poll it for updates
        let oracle = Oracle::new(handler.price_storage.clone(), oracle_interval_secs)?;
        tasks.push(oracle.run_loop());
    }
    let tracker = handler.swap_tracker.clone();
    tasks.push(tokio::spawn(async move {
        loop {
            // Every 1 hour we remove old swaps from the state
            stroemnet_protocol::sleep_secs(3600).await;
            tracker.write().await.cleanup_old_swaps(86400);
        }
    }));

    // If we have an address that we listen to we spawn the acceptance loop
    if let Some(bind_addr) = bind_addr {
        spawn_accept(bind_addr, network, peer_count, tasks).await;
    }
    Ok(())
}

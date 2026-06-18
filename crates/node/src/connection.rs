#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::SinkExt;
use futures::channel::mpsc as futures_mpsc;
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;

use stroemnet_p2p::P2p;
use stroemnet_p2p::network::NetEvent;
use stroemnet_protocol::{now_millis, sleep_secs};

const MAX_MESSAGES_PER_SECOND: u32 = 100;

/// Read from a peer connection, forwarding messages to the provided channel, and enforcing a rate limit to prevent abuse.
async fn read_from_peer_tracked(
    network: Arc<P2p>,
    peer: stroemnet_p2p::peer::ConnectedPeer,
    mut events_tx: futures_mpsc::Sender<NetEvent>,
    counter: Arc<AtomicUsize>,
    is_inbound: bool,
) {
    let url = peer.url.clone();
    let peer_node_id = peer.node_id;
    let our_node_id = network.config.node_id();

    // Check if this peer is blacklisted
    if network.is_blacklisted(&peer_node_id).await {
        tracing::warn!(
            "rejecting connection from blacklisted peer node_id={} (url={url})",
            hex::encode(peer_node_id)
        );
        let _ = peer.disconnect().await;
        return;
    }

    {
        // We connect and now we need to deterministically compute
        // which connection should be kept based on node IDs

        // Compute whether we are the lower node ID, which will determine which connection to keep in case of a duplicate
        let we_are_lower = our_node_id < peer_node_id;

        // We only want to keep connections that were initiated by the peer with the lower node ID
        // which they are if either its an outbound connection and we are lower, or an inbound connection and we are higher
        let new_is_lower_initiated = (!is_inbound && we_are_lower) || (is_inbound && !we_are_lower);

        // Check if this is an connected peer
        let existing_connected_peer = {
            let connected_peers = network.connected_peers.lock().await;
            connected_peers
                .iter()
                .position(|p| p.node_id == peer_node_id)
                .map(|i| (i, connected_peers[i].is_inbound))
        };
        if let Some((idx, existing_is_inbound)) = existing_connected_peer {
            // If the existing connection is initiated by lower id node
            let existing_is_lower_initiated =
                (!existing_is_inbound && we_are_lower) || (existing_is_inbound && !we_are_lower);

            // If the new connection is not initiated by the lower id node, we reject it
            if !new_is_lower_initiated {
                tracing::info!(
                    "rejecting duplicate peer node_id={} (higher-id-initiated, lower wins)",
                    hex::encode(peer_node_id)
                );
                return;
            }

            // This means that lower id node is initiated
            // If this existing is also lower initiated we have a tie and simply reject the new one
            if existing_is_lower_initiated {
                tracing::info!(
                    "rejecting duplicate peer node_id={} (existing already lower-id-initiated)",
                    hex::encode(peer_node_id)
                );
                return;
            }

            // If the existing one is not lower initiated, we replace the existing connection
            // with the new one, which is the one initiated by the lower id node
            let old = {
                let mut peers = network.connected_peers.lock().await;
                if idx < peers.len() && peers[idx].node_id == peer_node_id {
                    // Find the idx
                    Some(peers.remove(idx)) // Remove if it still exists and return it
                } else {
                    None
                }
            };

            // If we removed an existing peer, we need to disconnect it
            if let Some(p) = old {
                tracing::info!(
                    "replacing higher-id-initiated peer node_id={} with lower-id-initiated connection",
                    hex::encode(peer_node_id)
                );
                let _ = p.disconnect().await;
            }
        }
    }

    // Access the known peers from this peer
    let known_peers = peer.known_peers.clone();

    // Add the new peer to the network's list of connected peers
    network.add_connected_peer(peer.clone()).await;

    // increment the peer count
    counter.fetch_add(1, Ordering::SeqCst);

    // now process all the peers from the peer we just discovered
    network.process_peer_addrs(known_peers).await;

    // Create timing in order to keep track how many messages we receive from this peer per second
    let mut window_start = now_millis();
    let mut count_in_window: u32 = 0;
    loop {
        match peer.recv_msg().await {
            Ok(msg) => {
                let now = now_millis();
                if now.saturating_sub(window_start) >= 1000 || now < window_start {
                    // if one second has passed, we reset the window
                    window_start = now;
                    count_in_window = 0;
                }
                count_in_window += 1;
                if count_in_window > MAX_MESSAGES_PER_SECOND {
                    // if the rate exceeds rate limit, disconnect and blacklist peer
                    tracing::warn!(
                        "rate-limit exceeded for peer {url} (node_id={}); blacklisting until reboot",
                        hex::encode(peer_node_id)
                    );
                    network.blacklist_peer(peer_node_id).await;
                    let _ = peer.disconnect().await;
                    break;
                }
                // otherwise parse the message and forward it to the channel
                let evt = NetEvent {
                    from: url.clone(),
                    msg,
                };
                if events_tx.send(evt).await.is_err() {
                    tracing::debug!("event channel closed; ending peer task for {url}");
                    break;
                }
            }
            Err(e) => {
                tracing::info!("peer {url} disconnected: {e}");
                break;
            }
        }
    }
    // If we exit this loop, it means the peer has disconnected, so we need to clean up
    counter.fetch_sub(1, Ordering::SeqCst);
    network.remove_connected_peer(&url).await;
}

#[cfg(not(target_arch = "wasm32"))]
/// Spawns a task to dial a peer at a given URL and if it
/// is successful we read from this peer and get data.
///
/// Regardless if it fails or not we remove it from the "pending" dial set
/// which is used to prevent multiple concurrent dial attempts to the same peer.
pub(crate) fn spawn_addr_dial_driver(
    network: Arc<P2p>,
    url: String,
    counter: Arc<AtomicUsize>,
    in_flight: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    url_norm: String,
) {
    stroemnet_protocol::spawn(async move {
        match network.dial(&url).await {
            Ok(peer) => {
                tracing::info!(
                    "discovery: dialed {url} (node_id={})",
                    hex::encode(peer.node_id)
                );
                let events_tx = network.events_tx.clone();
                // Read from the peer what it says to us
                read_from_peer_tracked(network, peer, events_tx, counter, false).await;
            }
            Err(e) => tracing::debug!("discovery: dial {url} failed: {e}"),
        }

        // Regardless of the result, remove the URL from the in-flight set
        let _ = in_flight.lock().map(|mut s| s.remove(&url_norm));
    });
}

/// Spawns a task for each bootstrap peer to continuously attempt to connect.
/// This is so that we always try to have an entrypoint with the network.
pub(crate) fn spawn_bootstrap_with_counter(network: Arc<P2p>, counter: Arc<AtomicUsize>) {
    // We clone the bootstrap URLS from the provided network configuration.
    let urls = network.config.bootstrap_peers.clone();
    if urls.is_empty() {
        tracing::info!("no bootstrap peers configured");
        return;
    }
    // For all the bootstrap nodes spawn a redial loop.
    for url in urls {
        let net = network.clone();
        let c = counter.clone();
        stroemnet_protocol::spawn(async move {
            bootstrap_redial_loop(net, url, c).await;
        });
    }
}

/// Create a loop that continuously attempts to keep a connection to a bootstrap peer
async fn bootstrap_redial_loop(network: Arc<P2p>, url: String, counter: Arc<AtomicUsize>) {
    // Remove any trailing slashes and lowercase the URL for consistent comparison
    let url_norm = url.trim_end_matches('/').to_ascii_lowercase();
    loop {
        // Check if we have already connected to this peer
        if network.is_connected_peer(&url_norm).await {
            sleep_secs(30).await;
            continue;
        }

        tracing::info!("bootstrap: dialing {url}");

        // Dial with retry and backoff
        let Some(peer) = network.dial_with_backoff(&url).await else {
            tracing::warn!("bootstrap: giving up on {url} after 5 failed attempts");
            return;
        };
        tracing::info!("bootstrap: connected to {url}");
        let events_tx = network.events_tx.clone();

        // If we are able to dial, lets read from the connection
        read_from_peer_tracked(network.clone(), peer, events_tx, counter.clone(), false).await;
        tracing::info!("bootstrap: peer {url} disconnected — redialing");
        // Wait a bit before trying to redial to avoid tight loop in case of persistent failure
        sleep_secs(1).await;
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// On native, we are allowing incoming connections, this fn
/// binds a listener to the binding adfdress so that we can accept incoming p2p
/// connections.
pub(crate) async fn spawn_accept(
    bind_addr: SocketAddr,
    network: Arc<P2p>,
    counter: Arc<AtomicUsize>,
    tasks: &mut Vec<JoinHandle<()>>, // a list of managed tasks, that we manage from the main loop and cancel when needed
) {
    match network.clone().listen(bind_addr).await {
        Ok(mut inbound_rx) => {
            tracing::info!("P2P listener bound on {bind_addr}");
            let net_for_inbound = network.clone();
            let counter_for_inbound = counter.clone();
            // Spawn a task to accept incoming connections and read from them
            tasks.push(tokio::spawn(async move {
                while let Some(peer) = inbound_rx.recv().await {
                    let url = peer.url.clone();
                    let events_tx = net_for_inbound.events_tx.clone();
                    let net2 = net_for_inbound.clone();
                    let c2 = counter_for_inbound.clone();
                    // For each incoming connection we spawn a new tokio task to handle it.
                    tokio::spawn(async move {
                        read_from_peer_tracked(net2, peer, events_tx, c2, true).await;
                        tracing::info!("inbound peer {url} disconnected");
                    });
                }
            }));
        }
        Err(e) => {
            tracing::warn!("Could not bind P2P listener on {bind_addr}: {e}");
        }
    }
}

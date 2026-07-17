use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::SinkExt;
use futures::channel::mpsc as futures_mpsc;

use super::resolve::should_proceed;
use stroemnet_p2p::P2p;
use stroemnet_p2p::network::NetEvent;
use stroemnet_p2p::peer::ConnectedPeer;
use stroemnet_protocol::now_millis;

/// Maximum messages per second from a peer
const MAX_MESSAGES_PER_SECOND: u32 = 100;

/// Read data from a connected peer
pub(super) async fn read_from_peer_tracked(
    network: Arc<P2p>,
    peer: ConnectedPeer,
    mut events_tx: futures_mpsc::Sender<NetEvent>,
    counter: Arc<AtomicUsize>,
    is_inbound: bool,
) {
    let url = peer.url.clone();
    let peer_node_id = peer.node_id;

    // If this peer id is blacklisted, no need to talk
    if network.is_blacklisted(&peer_node_id).await {
        let _ = peer.disconnect().await;
        return;
    }
    if !should_proceed(&network, peer_node_id, is_inbound).await {
        return;
    }

    let known_peers = peer.known_peers.clone();

    // Add this peer as a connected peer
    network.add_connected_peer(peer.clone()).await;
    counter.fetch_add(1, Ordering::SeqCst);

    // Process the peers known address
    network.process_peer_addrs(known_peers).await;

    let mut window_start = now_millis();
    let mut count_in_window: u32 = 0;
    loop {
        // Read a message from the peer
        match peer.recv_msg().await {
            Ok(msg) => {
                let now = now_millis();
                // If the window has expired, we restart the window and count again
                if now.saturating_sub(window_start) >= 1000 || now < window_start {
                    window_start = now;
                    count_in_window = 0;
                }
                count_in_window += 1;
                // If the count within the window exceeds allowed,
                // we disconnect from this peer
                if count_in_window > MAX_MESSAGES_PER_SECOND {
                    tracing::warn!("rate-limit exceeded for {url}; blacklisting until reboot");
                    network.blacklist_peer(peer_node_id).await;
                    let _ = peer.disconnect().await;
                    break;
                }
                // Transmit the received data to the internal handler
                if events_tx
                    .send(NetEvent {
                        from: url.clone(),
                        msg,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Err(e) => {
                // if we disconnect then we exit this loop
                tracing::info!("peer {url} disconnected: {e}");
                break;
            }
        }
    }
    counter.fetch_sub(1, Ordering::SeqCst);
    network.remove_connected_peer(&url).await;
}

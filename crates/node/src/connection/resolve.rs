use std::sync::Arc;

use stroemnet_p2p::P2p;

/// Whether or not the lower node initiated the connection
/// as we only want to maintain one of them
/// 
/// Either the connection is inbound and we are not lower which means
/// the other aprty is lower
/// 
/// Or the connection is outbound and we are lower, which means we are lower
fn lower_initiated(is_inbound: bool, we_are_lower: bool) -> bool {
    (!is_inbound && we_are_lower) || (is_inbound && !we_are_lower)
}

/// Compute whether the connection should proceed or be aborted.
pub(super) async fn should_proceed(
    network: &Arc<P2p>, // network instance
    peer_node_id: [u8; 32], // id of the peer
    is_inbound: bool, // whether the connection is inbound
) -> bool {
    // If our node our id is lower then we are lower
    let we_are_lower = network.config.node_id() < peer_node_id;

    // Whether this connection is initiated by the lower id node
    let new_is_lower_initiated = lower_initiated(is_inbound, we_are_lower);

    // Check if we already have an existing connection with this node id
    let existing = {
        let peers = network.connected_peers.lock().await;
        peers
            .iter()
            .enumerate()
            .find(|(_, p)| p.node_id == peer_node_id)
            .map(|(i, p)| (i, p.is_inbound))
    };

    // If we dont have an existing then we should continue otherwise continue checking
    let Some((idx, existing_is_inbound)) = existing else {
        return true;
    };
    // If the existing connection is already initiated by the 
    // lower node then we dont need to continue
    let existing_is_lower_initiated = lower_initiated(existing_is_inbound, we_are_lower);
    if !new_is_lower_initiated || existing_is_lower_initiated {
        tracing::info!(
            "rejecting duplicate peer node_id={} (lower-id-initiated wins)",
            hex::encode(peer_node_id)
        );
        return false;
    }


    // The old connection was not lower initiated so we update to use this connection instead
    // but for that we need to remove the old connection
    let old = {
        let mut peers = network.connected_peers.lock().await;
        if peers.get(idx).map(|p| p.node_id) == Some(peer_node_id) {
            Some(peers.remove(idx))
        } else {
            None
        }
    };

    // disconnect from the old connection
    if let Some(p) = old {
        let _ = p.disconnect().await;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_initiated_truth_table() {
        assert!(lower_initiated(false, true));
        assert!(lower_initiated(true, false));
        assert!(!lower_initiated(false, false));
        assert!(!lower_initiated(true, true));
    }
}

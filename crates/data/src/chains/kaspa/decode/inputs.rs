use ahash::AHashMap;
use kaspa_addresses::Prefix;
use kaspa_rpc_core::RpcTransaction;
use std::sync::Arc;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::ChainEvent;
use tokio::sync::RwLock;

use super::classify::{classify_spend, derive_p2sh_addr, last_redeem};
use crate::UtxoScript;

/// Compute the swap id from this event by inspecting the chain events
fn event_swap_id(event: &ChainEvent) -> Option<[u8; 32]> {
    match event {
        ChainEvent::Reveal(r) => Some(r.swap_id),
        ChainEvent::Refund(r) => Some(r.swap_id),
        _ => None,
    }
}

/// Scan all the inputs and create chainevents out of them
pub(super) async fn scan_inputs(
    tx: &RpcTransaction,                                      // the rpc transaction
    utxo_scripts: &Arc<RwLock<AHashMap<String, UtxoScript>>>, // a map of utxo scripts
    prefix: Prefix,                                           // the kaspa network prefix
    channel_id: ChannelId,                                    // channel id
    events: &mut Vec<ChainEvent>,                             // events containing chainevents
) {
    let mut closed_addrs: Vec<String> = Vec::new();
    let mut emit_dedup: AHashMap<[u8; 32], bool> = AHashMap::new();

    // Go over all inputs in this transaction
    for input in tx.inputs.iter() {
        // Try to parse a redeem script
        let Some(redeem_script) = last_redeem(&input.signature_script) else {
            continue;
        };

        // Compute the p2sh address
        let Some(addr) = derive_p2sh_addr(&redeem_script, prefix) else {
            continue;
        };

        // Try and extract the full utxo script verifyin that we have known about this script before
        let utxo_script = {
            let scripts = utxo_scripts.read().await;
            match scripts.get(&addr) {
                Some(us) => us.clone(),
                None => continue,
            }
        };

        tracing::info!("Detected HTLC spend at {addr}");

        // Classify the type of htlc spend
        match classify_spend(&input.signature_script, &utxo_script, prefix, channel_id) {
            Ok((Some(event), closed)) => {
                // If its closed then it means the swap is finalized
                if closed {
                    // Push it as a finalized swap address
                    closed_addrs.push(addr.clone());
                }

                // Check if we have already seen this swap
                let already = event_swap_id(&event)
                    .map(|id| emit_dedup.insert(id, true).is_some())
                    .unwrap_or(false);
                if !already {
                    // If we havent seen this swap we push it to detected events
                    events.push(event);
                }
            }
            Ok((None, _)) => {
                tracing::error!("{addr} could not parse as reveal or refund");
            }
            Err(e) => {
                tracing::error!("Error parsing HTLC spend sig_script at {addr}: {e}");
            }
        }
    }

    // For all the closed p2sh addresses that have been fulfilled (fulfilled swaps)
    // we simply remove them from the tracked scripts
    if !closed_addrs.is_empty() {
        let mut scripts = utxo_scripts.write().await;
        for addr in &closed_addrs {
            if scripts.remove(addr).is_some() {
                tracing::info!("swap closed — cleared utxo_scripts at {addr}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stroemnet_protocol::v1::{RefundV1, RevealV1};

    #[test]
    fn event_swap_id_reads_reveal_and_refund() {
        assert_eq!(
            event_swap_id(&ChainEvent::Reveal(RevealV1 {
                swap_id: [1u8; 32],
                secret: [0u8; 32]
            })),
            Some([1u8; 32])
        );
        assert_eq!(
            event_swap_id(&ChainEvent::Refund(RefundV1 { swap_id: [2u8; 32] })),
            Some([2u8; 32])
        );
    }
}

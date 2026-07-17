use ahash::AHashMap;
use kaspa_addresses::Prefix;
use kaspa_wrpc_client::prelude::RpcBlock;
use std::sync::Arc;
use stroemnet_protocol::ChannelId;
use tokio::sync::RwLock;

use super::inputs::scan_inputs;
use super::outputs::scan_outputs;
use super::parse::BlockOutcomes;
use crate::UtxoScript;
use crate::chains::kaspa::error::Result;

/// Handle the addition of a block
pub(crate) async fn handle_block_added(
    safe_block: &Arc<RpcBlock>, // a safe block that has been confirmed
    utxo_scripts: &Arc<RwLock<AHashMap<String, UtxoScript>>>, // the utxo scripts inside this block
    prefix: Prefix,             // chain prefix
    channel_id: ChannelId,      // the channel id we are working on
) -> Result<BlockOutcomes> {
    let mut events = Vec::new();
    let mut refunds = Vec::new();

    let known_count = utxo_scripts.read().await.len();
    tracing::debug!(
        "parser: scanning block {} txs against {known_count} registered HTLC scripts",
        safe_block.transactions.len()
    );

    // Go over all the transactions
    for tx in safe_block.transactions.iter() {
        scan_outputs(
            // scan the outputs
            tx,
            utxo_scripts,
            prefix,
            channel_id,
            &mut events,
            &mut refunds,
        )
        .await?;
        // scan the inputs
        scan_inputs(tx, utxo_scripts, prefix, channel_id, &mut events).await;
    }

    Ok(BlockOutcomes { events, refunds })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use kaspa_consensus_core::tx::ScriptPublicKey;

    #[test]
    fn spk_json_serialization_format() {
        let script_bytes = vec![0xaa, 0x20, 0x01, 0x02, 0x03, 0x04, 0x05];
        let spk = ScriptPublicKey::new(0, script_bytes.clone().into());

        let serialized = serde_json::to_vec(&spk).unwrap();
        let serialized_str = String::from_utf8(serialized.clone()).unwrap();

        let expected_hex = format!("0000{}", hex::encode(&script_bytes));
        let expected_json = format!("\"{}\"", expected_hex);

        assert_eq!(
            serialized_str, expected_json,
            "ScriptPublicKey JSON serialization must be a quoted hex string"
        );
    }
}

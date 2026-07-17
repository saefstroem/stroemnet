use ahash::AHashMap;
use kaspa_addresses::Prefix;
use kaspa_rpc_core::RpcTransaction;
use kaspa_txscript::extract_script_pub_key_address;
use std::sync::Arc;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::ChainEvent;
use tokio::sync::RwLock;

use super::super::contracts::extract_commitment;
use super::parse::parse_script;
use crate::UtxoScript;
use crate::chains::kaspa::error::Result;

/// Check if the target is a valid u64
fn deposit_target_valid(target: &str) -> Option<u64> {
    match target.parse::<u64>() {
        Ok(v) if v > 0 => Some(v),
        _ => None,
    }
}

/// Scan the outputs to detect potential commitments
pub(super) async fn scan_outputs(
    tx: &RpcTransaction,
    utxo_scripts: &Arc<RwLock<AHashMap<String, UtxoScript>>>,
    prefix: Prefix,
    channel_id: ChannelId,
    events: &mut Vec<ChainEvent>,
    refunds: &mut Vec<([u8; 32], u64)>,
) -> Result<()> {
    // The scripts we have successfully matched to be scripts that we have known about
    let mut matched: Vec<(usize, String, u64, UtxoScript)> = Vec::new();
    {
        // Acquire read lock on scripts
        let scripts = utxo_scripts.read().await;

        // Go over all outputs
        for (output_idx, output) in tx.outputs.iter().enumerate() {
            // Compute the p2sh address for some spk
            let address = match extract_script_pub_key_address(&output.script_public_key, prefix) {
                Ok(a) => a,
                Err(e) => {
                    tracing::trace!(
                        "parser: skip output {output_idx} — address derive failed: {e}"
                    );
                    continue;
                }
            };
            let key = address.to_string();

            // If the utxo script is known then we have matched it and we add it the DS
            if let Some(utxo_script) = scripts.get(&key) {
                matched.push((output_idx, key, output.value, utxo_script.clone()));
            }
        }
    }

    // For all those outputs we have matched
    for (output_idx, p2sh_addr, value, utxo_script) in matched {
        tracing::info!("matched HTLC output {output_idx} value={value} at {p2sh_addr}");
        let commitment = {
            // Extract the opcodes from the raw script
            let script = parse_script(&utxo_script.redeem_script)
                .collect::<std::result::Result<Vec<_>, _>>()?;

            // Attempt to extract the commitment from the script
            match extract_commitment(
                &script,
                utxo_script.deposit_target.clone(),
                prefix,
                channel_id,
            ) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("redeem script parse failed: {e}");
                    continue;
                }
            }
        };
        // Parse the deposit target to u64
        let target = match deposit_target_valid(&utxo_script.deposit_target) {
            Some(v) => v,
            None => {
                tracing::warn!(
                    "skipping match for swap {}: deposit_target unparseable or zero ({})",
                    hex::encode(commitment.swap_id),
                    utxo_script.deposit_target
                );
                continue;
            }
        };
        // Schedule a proactive refund for this swap
        refunds.push((commitment.swap_id, commitment.unlock_ts));

        // Ensure the value of this htlc is geq than the target
        if value >= target {
            tracing::info!(
                "swap {} funded ({value} >= {target}) at {p2sh_addr} — emitting Commitment",
                hex::encode(commitment.swap_id),
            );
            events.push(ChainEvent::Commitment(commitment));
        } else {
            // if user didnt pay we simply have scheduled a refund
            tracing::info!(
                "swap {} underpaid in tx ({value} < {target}) at {p2sh_addr} — refund scheduled",
                hex::encode(commitment.swap_id),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deposit_target_valid_requires_positive_integer() {
        assert_eq!(deposit_target_valid("100"), Some(100));
        assert_eq!(deposit_target_valid("0"), None);
        assert_eq!(deposit_target_valid("abc"), None);
    }
}

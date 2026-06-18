use crate::UtxoScript;
use crate::chains::kaspa::contracts::contract_v1::VerifiableTransactionMock;
use crate::chains::kaspa::contracts::contract_v1::{
    extract_commitment, extract_reveal_secret, validate_refund_sig,
};
use crate::chains::kaspa::error::Result;
use ahash::AHashMap;
use itertools::Itertools;
use kaspa_addresses::Prefix;
use kaspa_consensus_core::hashing::sighash::{SigHashReusedValues, SigHashReusedValuesUnsync};
use kaspa_consensus_core::tx::VerifiableTransaction;
use kaspa_txscript::opcodes::{OpCodeImplementation, deserialize_next_opcode};
use kaspa_txscript::{extract_script_pub_key_address, pay_to_script_hash_script};
use kaspa_txscript_errors::TxScriptError;
use kaspa_wrpc_client::prelude::RpcBlock;
use std::sync::Arc;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{ChainEvent, RefundV1, RevealV1};
use tokio::sync::RwLock;

type DynOpcodeImplementation<Tx, Reused> = Box<dyn OpCodeImplementation<Tx, Reused>>;

pub(super) fn parse_script<T: VerifiableTransaction, Reused: SigHashReusedValues>(
    script: &[u8],
) -> impl Iterator<Item = std::result::Result<DynOpcodeImplementation<T, Reused>, TxScriptError>> + '_
{
    script.iter().batching(|it| deserialize_next_opcode(it))
}

#[derive(Debug)]
/// A container for the outcomes of processing a block,
/// including detected chain events and scheduled refunds.
pub(super) struct BlockOutcomes {
    pub events: Vec<ChainEvent>,
    pub refunds: Vec<([u8; 32], u64)>,
}

/// An isolated function that takes a block and the current known HTLC scripts,
/// and returns the detected events and refunds in that block.
/// This is the core of our UTXO parsing logic, and is designed to be
/// easily testable in isolation from the rest of the system.
pub(super) async fn handle_block_added(
    safe_block: &Arc<RpcBlock>,
    utxo_scripts: &Arc<RwLock<AHashMap<String, UtxoScript>>>,
    prefix: Prefix,
    chain_id: ChannelId,
) -> Result<BlockOutcomes> {
    // A container for all events that we extract from this block.
    let mut events: Vec<ChainEvent> = Vec::new();
    let mut refunds: Vec<([u8; 32], u64)> = Vec::new();

    // Log the number of transactions and the number of known HTLC scripts for debugging and monitoring purposes.
    let known_count = utxo_scripts.read().await.len();
    tracing::debug!(
        "parser: scanning block {} txs against {known_count} registered HTLC scripts",
        safe_block.transactions.len()
    );

    // Go over all transactions in the block.
    for tx in safe_block.transactions.iter() {
        // Create a container to store all the outputs for this transaction that match known HTLC scripts.
        let mut matched_outputs: Vec<(usize, String, u64, UtxoScript)> = Vec::new();
        {
            // Acquire read lock on the script storage.
            let scripts: tokio::sync::RwLockReadGuard<'_, AHashMap<String, UtxoScript>> =
                utxo_scripts.read().await;
            for (output_idx, output) in tx.outputs.iter().enumerate() {
                let script_pubkey = &output.script_public_key;

                // Attempt to extract the address from the script pubkey. If this fails, we skip this output.
                let address = match extract_script_pub_key_address(script_pubkey, prefix) {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::trace!(
                            "parser: skip output {output_idx} — address derive failed: {e}"
                        );
                        continue;
                    }
                };

                // Attempt to retrieve the UTXO script associated with this address.
                // If there is a match, it means this output is an HTLC output that we are interested in,
                // and we store it in the matched_outputs container for further processing.
                let key = address.to_string();
                if let Some(utxo_script) = scripts.get(&key) {
                    matched_outputs.push((output_idx, key, output.value, utxo_script.clone()));
                }
            }
        }

        // Go over all the outputs that matched known HTLC scripts and attempt to parse them as commitments.
        for (output_idx, p2sh_addr, value, utxo_script) in matched_outputs {
            tracing::info!("matched HTLC output {output_idx} value={value} at {p2sh_addr}");

            let commitment_extract = {
                let script = parse_script(&utxo_script.redeem_script)
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                extract_commitment(
                    &script,
                    utxo_script.deposit_target.clone(),
                    prefix,
                    chain_id,
                )
            };

            // If the parsing fails, we log an error and skip this output.
            let commitment = match commitment_extract {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("redeem script parse failed: {e}");
                    continue;
                }
            };

            // Parse the deposit target from the UTXO script. If this fails or is zero, we log a warning and skip this output.
            let target = match utxo_script.deposit_target.parse::<u64>() {
                Ok(v) if v > 0 => v,
                _ => {
                    tracing::warn!(
                        "skipping match for swap {}: deposit_target unparseable or zero ({})",
                        hex::encode(commitment.swap_id),
                        utxo_script.deposit_target
                    );
                    continue;
                }
            };

            // Record the refund information for this commitment.
            // A refund is always proactively scheduled for every detected commitment,
            refunds.push((commitment.swap_id, commitment.unlock_ts));

            // We only create a comitment event if the output value meets
            // or exceeds the target amount specified in the UTXO script.
            if value >= target {
                tracing::info!(
                    "swap {} funded ({value} >= {target}) at {p2sh_addr} — emitting Commitment",
                    hex::encode(commitment.swap_id),
                );
                events.push(ChainEvent::Commitment(commitment));
            } else {
                tracing::info!(
                    "swap {} underpaid in tx ({value} < {target}) at {p2sh_addr} — refund scheduled, no commitment",
                    hex::encode(commitment.swap_id),
                );
            }
        }

        // A container for swaps that we detected in this transaction as being revealed or refunded
        let mut closed_addrs: Vec<String> = Vec::new();
        let mut emit_dedup: AHashMap<[u8; 32], bool> = AHashMap::new();

        // Go over all inputs which would capture reveals or refunds.
        for input in tx.inputs.iter() {
            let redeem_script: Vec<u8> = {
                // Extract all the opcodes from the signature script
                let opcodes = match parse_script::<
                    VerifiableTransactionMock,
                    SigHashReusedValuesUnsync,
                >(&input.signature_script)
                .collect::<std::result::Result<Vec<_>, _>>()
                {
                    Ok(o) => o,
                    Err(_) => continue,
                };
                // We want to extract the redeem script, which should be the last
                // element in the sig script.
                match opcodes.last() {
                    Some(op) => {
                        // The redeem script should be a data opcode
                        let d = op.get_data();
                        if d.is_empty() {
                            continue;
                        }
                        d.to_vec()
                    }
                    None => continue,
                }
            };

            // Extract the p2sh address from the redeem script,
            // we will check it against the locally computed one.
            let addr = match extract_script_pub_key_address(
                &pay_to_script_hash_script(&redeem_script),
                prefix,
            ) {
                Ok(a) => a.to_string(),
                Err(_) => continue,
            };

            // Acquire read lock and retrieve a stored UtxoScript if we have it
            let utxo_script = {
                let scripts = utxo_scripts.read().await;
                match scripts.get(&addr) {
                    Some(us) => us.clone(),
                    None => continue,
                }
            };

            tracing::info!("Detected HTLC spend at {addr}");

            // It seems that the script we have is indeed one that has been
            // broadcasted to us before, this means we need to try and parse it into a proper
            // chain event.
            let event_result: Result<Option<ChainEvent>> = (|| -> Result<Option<ChainEvent>> {
                // Retrieve the sig script opcodes again
                let sig_script_opcodes = parse_script(&input.signature_script)
                    .collect::<std::result::Result<Vec<_>, _>>()?;

                // Retrieve the redeem opcodes.
                let redeem_opcodes = parse_script(&utxo_script.redeem_script)
                    .collect::<std::result::Result<Vec<_>, _>>()?;

                // Regardless of whether we have a refund or claim transaction
                // we always operate on the redeem script. As such, we should be able to
                // compute the swap id from it by extracting it from the script.
                let swap_id = extract_commitment(
                    &redeem_opcodes,
                    utxo_script.deposit_target.clone(),
                    prefix,
                    chain_id,
                )
                .ok()
                .map(|c| c.swap_id);

                // Attempt to extract a reveal secret (if this is a claim transaction)
                if let Ok(secret) = extract_reveal_secret(&sig_script_opcodes) {
                    let id = match swap_id {
                        Some(id) => id,
                        None => return Ok(None),
                    };
                    // If we are able to we should mark this address as "closed"
                    // as such, the swap is completed after this stage. At least
                    // from this party's perspective.
                    closed_addrs.push(addr.clone());
                    return Ok(Some(ChainEvent::Reveal(RevealV1 {
                        swap_id: id,
                        secret,
                    })));
                }

                // Attempt to validate that this is a refund transaction
                // in which case we also mark it as closed
                // Since a refund is a terminal state, just as a claim.
                if validate_refund_sig(&sig_script_opcodes).is_ok() {
                    let id = match swap_id {
                        Some(id) => id,
                        None => return Ok(None),
                    };
                    closed_addrs.push(addr.clone());
                    return Ok(Some(ChainEvent::Refund(RefundV1 { swap_id: id })));
                }

                // If we end up here all the parsing attempts failed,
                // which means that this script does not contain any useful or actionable message for us.
                Ok(None)
            })();

            // Match the output of the event
            match event_result {
                Ok(Some(event)) => {
                    let event_swap_id = match &event {
                        ChainEvent::Reveal(r) => Some(r.swap_id),
                        ChainEvent::Refund(r) => Some(r.swap_id),
                        _ => None,
                    };

                    // Check if this event is already in the dedup set
                    // since theoretically we can expect multiple duplicate transactions
                    let already = event_swap_id
                        .map(|id| emit_dedup.insert(id, true).is_some())
                        .unwrap_or(false);

                    // If its not in dedup set, push it to the events container.
                    if !already {
                        events.push(event);
                    }
                }
                Ok(None) => {
                    tracing::error!("{addr} could not parse as reveal or refund");
                }
                Err(e) => {
                    tracing::error!("Error parsing HTLC spend sig_script at {addr}: {e}");
                }
            }
        }

        // If we have closed a swap in this parsing, we should remove the UTXO scripts
        // from storage
        if !closed_addrs.is_empty() {
            let mut scripts = utxo_scripts.write().await;
            for addr in &closed_addrs {
                if scripts.remove(addr).is_some() {
                    tracing::info!("swap closed — cleared utxo_scripts at {addr}");
                }
            }
        }
    }
    Ok(BlockOutcomes { events, refunds })
}

#[cfg(test)]
mod tests {
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

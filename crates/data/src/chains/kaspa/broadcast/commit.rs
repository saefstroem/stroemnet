use std::sync::Arc;

use kaspa_consensus_core::subnets::SUBNETWORK_ID_NATIVE;
use kaspa_consensus_core::tx::{MutableTransaction, Transaction, TransactionOutput, UtxoEntry};
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_txscript::{extract_script_pub_key_address, pay_to_script_hash_script};
use kaspa_wrpc_client::KaspaRpcClient;
use stroemnet_protocol::v1::CommitmentV1;

use super::super::error::{KaspaError, Result};
use super::fee::calculate_priority_fee;
use super::htlc::{Announce, htlc_script_from_commitment};
use super::signer::Signer340;
use super::utxo::{prefix_for, rpc_utxo_to_entry, select_funding_utxos, to_inputs};
use crate::chains::net::{NETWORK_TIMEOUT, retry_timed, timed};

/// Submits a commitment across the kaspa network
pub(crate) async fn submit_commitment(
    client: &Arc<KaspaRpcClient>, // the kaspa rpc client
    private_key: &str,            // private key
    coinbase_maturity: u64, // number of daa scores needed for coinbase maturity (miners can be LPs)
    commitment: &CommitmentV1, // the commmitment to commit to onchain
) -> Result<Announce> {
    // Retrieve the network refix
    let prefix = prefix_for(client).await?;

    // Compute the signer from the prefix and private key
    let signer = Signer340::derive(private_key, prefix)?;

    // Conver the commitment into a kaspa canonical htlc script
    let (htlc_script, _sender_spk, _receiver_spk, _unlock_ts_ms) =
        htlc_script_from_commitment(commitment)?;

    // Compute the p2sh spk of the script
    let htlc_spk = pay_to_script_hash_script(&htlc_script);

    // Retrieve out p2pk spk
    let our_spk = signer.spk();

    // Retrieve the utxos that are available for our address
    let utxos = client
        .get_utxos_by_addresses(vec![signer.address()])
        .await?;

    // If we do not have any utxos then we need to error
    if utxos.is_empty() {
        return Err(KaspaError::NoUtxos);
    }

    // Retrieve the block dag info to get the dag data
    let dag_info = retry_timed("get_block_dag_info", || client.get_block_dag_info())
        .await
        .ok_or_else(|| KaspaError::Other("get_block_dag_info: timed out".into()))?;
    let current_daa = dag_info.virtual_daa_score;

    // Parse the amount needed for the commitment
    let amount: u64 = commitment.amount.value.parse()?;

    // Select those utxos which have sufficient maturity and satisfy the value requirement
    let (selected_utxos, total_input) =
        select_funding_utxos(utxos, amount, coinbase_maturity, current_daa)?;

    // Convert the selected utxos to tx inputs
    let inputs = to_inputs(&selected_utxos);

    // Compute the change that we probably will get
    let preliminary_change = total_input.saturating_sub(amount);

    // Create an output for the htlc
    let mut outputs = vec![TransactionOutput::new(amount, htlc_spk.clone())];
    if preliminary_change > 0 {
        // And the change to our address
        outputs.push(TransactionOutput::new(preliminary_change, our_spk.clone()));
    }
    // Create a preliminary tx for mass estimation
    let preliminary_tx = Transaction::new(
        0,
        inputs.clone(),
        outputs,
        0,
        SUBNETWORK_ID_NATIVE,
        0,
        vec![],
    );

    // Compute the fee
    let fee = calculate_priority_fee(client, &preliminary_tx, 0).await?;

    if total_input < amount + fee {
        return Err(KaspaError::InsufficientFunds {
            needed: amount + fee,
            available: total_input,
        });
    }

    // Now compute the final change that we get after accounting for fee
    let change = total_input.saturating_sub(amount).saturating_sub(fee);
    let mut final_outputs = vec![TransactionOutput::new(amount, htlc_spk.clone())];
    if change > 0 {
        // Add the change to the final outputs
        final_outputs.push(TransactionOutput::new(change, our_spk.clone()));
    }

    // Create the final transaction
    let tx = Transaction::new(0, inputs, final_outputs, 0, SUBNETWORK_ID_NATIVE, 0, vec![]);

    // Convert the selected utxos to rpc compatible utxo entries
    let utxo_entries: Vec<UtxoEntry> = selected_utxos.iter().map(rpc_utxo_to_entry).collect();

    // Create a mutable transaction
    let mut mutable_tx = MutableTransaction::with_entries(tx, utxo_entries);
    let mut signed_scripts = Vec::with_capacity(selected_utxos.len());

    // Go over all the utxos and sign them
    for i in 0..selected_utxos.len() {
        signed_scripts.push(signer.sign_input(&mutable_tx, i)?);
    }

    // Then update the signature scripts with the signatures
    for (input, script) in mutable_tx.tx.inputs.iter_mut().zip(signed_scripts) {
        input.signature_script = script;
    }

    // Convert the mutable tx into an rpc tx finalizing it
    let rpc_tx = (&mutable_tx.tx).into();

    // Submit the transaction over rpc with a timeout
    let tx_id = timed(NETWORK_TIMEOUT, client.submit_transaction(rpc_tx, false))
        .await
        .ok_or_else(|| KaspaError::Other("submit_transaction: timed out".into()))??;
    tracing::info!("Kaspa HTLC commitment submitted: txid {tx_id}");

    // Create the announcement that we have committed to the specified p2sh address
    let address = extract_script_pub_key_address(&htlc_spk, prefix)?.to_string();
    Ok(Announce {
        address,
        redeem_script: htlc_script,
    })
}

use std::sync::Arc;

use kaspa_consensus_core::subnets::SUBNETWORK_ID_NATIVE;
use kaspa_consensus_core::tx::{MutableTransaction, ScriptPublicKey, Transaction};
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::KaspaRpcClient;

use super::super::contracts::SOLVER_REWARD;
use super::super::error::{KaspaError, Result};
use super::fee::calculate_priority_fee;
use super::prepare::HtlcSpend;
use super::txbuild::{spend_inputs, spend_outputs};
use super::utxo::rpc_utxo_to_entry;
use crate::chains::net::{NETWORK_TIMEOUT, timed};

const MIN_REWARD_OUTPUT_SOMPI: u64 = 10_000;

/// The parameters needed in order to spend an htlc
pub(super) struct SpendParams<'a> {
    /// Destination spk what we are spending
    pub dest_spk: &'a ScriptPublicKey,
    /// The lock time for the transaction
    pub lock_time: u64,
    /// The extra sig bytes to account for non standard sig script
    pub extra_sig_bytes: u64,
    /// The sig script to execute the wanted branch of the htlc contract
    pub branch_sig_script: Vec<u8>,
    /// Label for logs
    pub log_label: &'a str,
}

/// Submit a generic htlc spending across the kaspa network
pub(super) async fn submit_htlc_spend(
    client: &Arc<KaspaRpcClient>, // the kaspa rpc client
    ctx: &HtlcSpend,              // needed context for spending
    params: SpendParams<'_>,      // spending parameters (which branch to exec and so forth)
) -> Result<()> {
    // retrieve our spk
    let our_spk = ctx.signer.spk();

    // We need to spend all htlc utxos on their own
    // the htlc contract has strict requirements for inputs and output for safety purposes
    for utxo in ctx.htlc_utxos.iter() {
        // Compute the destination amount which is the htlc value - solver reward
        let dest_amount = utxo
            .utxo_entry
            .amount
            .checked_sub(SOLVER_REWARD as u64)
            .ok_or(KaspaError::InsufficientFunds {
                needed: SOLVER_REWARD as u64,
                available: utxo.utxo_entry.amount,
            })?;

        // Compute the overall usable capital including the solver reward
        let reward_pre = (SOLVER_REWARD as u64)
            .checked_add(ctx.fee_utxo.utxo_entry.amount)
            .ok_or_else(|| KaspaError::Other("Solver reward + fee UTXO overflow".to_string()))?;

        // Compute the tx inputs
        let inputs = spend_inputs(utxo, &ctx.fee_utxo);

        // Create the transaction
        let prelim = Transaction::new(
            0,
            inputs.clone(),
            spend_outputs(dest_amount, reward_pre, params.dest_spk, &our_spk), // create the outputs
            params.lock_time,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );
        // Compute the fee
        let fee = calculate_priority_fee(client, &prelim, params.extra_sig_bytes).await?;

        // Compute the final reward after accounting for the fee
        let reward = reward_pre
            .checked_sub(fee)
            .ok_or(KaspaError::InsufficientFunds {
                needed: fee,
                available: reward_pre,
            })?;

        // If the fee is below 10k sompi its not viable to fulfill this swap
        if reward < MIN_REWARD_OUTPUT_SOMPI {
            return Err(KaspaError::InsufficientFunds {
                needed: MIN_REWARD_OUTPUT_SOMPI,
                available: reward,
            });
        }

        // Create a finalized transaction with the outputs
        let tx = Transaction::new(
            0,
            inputs,
            spend_outputs(dest_amount, reward, params.dest_spk, &our_spk),
            params.lock_time,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );

        // Compute the entries of the utxos
        let entries = vec![rpc_utxo_to_entry(utxo), rpc_utxo_to_entry(&ctx.fee_utxo)];

        // Create a mutable transaction with those entries
        let mut mtx = MutableTransaction::with_entries(tx, entries);

        // Sign the fee utxo which is always at index 1
        let fee_sig = ctx.signer.sign_input(&mtx, 1)?;

        // Ensure that we only have two inputs
        match mtx.tx.inputs.as_mut_slice() {
            [htlc_in, fee_in] => {
                htlc_in.signature_script = params.branch_sig_script.clone();
                fee_in.signature_script = fee_sig;
            }
            _ => {
                return Err(KaspaError::Other(
                    "htlc spend tx must have exactly 2 inputs".into(),
                ));
            }
        }

        // Conver the tx into finalized rpc transaction
        let rpc_tx = (&mtx.tx).into();

        // Submit the transaction across the network with a timeout.
        let tx_id = timed(NETWORK_TIMEOUT, client.submit_transaction(rpc_tx, false))
            .await
            .ok_or_else(|| KaspaError::Other("submit_transaction: timed out".into()))??;
        tracing::info!("Kaspa {} submitted: txid {tx_id}", params.log_label);
    }
    Ok(())
}

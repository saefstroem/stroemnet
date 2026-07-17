use std::sync::Arc;

use kaspa_txscript::{opcodes::codes::OpFalse, script_builder::ScriptBuilder};
use kaspa_wrpc_client::KaspaRpcClient;
use stroemnet_protocol::v1::CommitmentV1;

use super::super::error::{Result, script_err};
use super::prepare::prepare_htlc_spend;
use super::spend::{SpendParams, submit_htlc_spend};

/// Submit the refund of a htlc leg on kaspa
pub(crate) async fn submit_refund(
    client: &Arc<KaspaRpcClient>, // the kaspa rpc client
    private_key: &str,            // the private key of signer
    coinbase_maturity: u64,       // how many daa score to wait until a miner utxo is spendable
    commitment: &CommitmentV1,    // the commitment for the htlc swap leg
) -> Result<()> {
    // Prepare the htlc for spending
    let ctx = prepare_htlc_spend(client, private_key, coinbase_maturity, commitment).await?;

    // Create the calldata for executing the htlc branch to refund the swap
    let branch_sig_script = ScriptBuilder::new()
        .add_op(OpFalse)
        .map_err(script_err)?
        .add_data(&ctx.htlc_script)
        .map_err(script_err)?
        .drain();

    // Submit the htlc to be spent
    // we have already encoded the sig script so its a generic dispatch fn
    submit_htlc_spend(
        client,
        &ctx,
        SpendParams {
            dest_spk: &ctx.sender_spk,
            lock_time: ctx.unlock_ts_ms,
            extra_sig_bytes: 260,
            branch_sig_script,
            log_label: "refund",
        },
    )
    .await
}

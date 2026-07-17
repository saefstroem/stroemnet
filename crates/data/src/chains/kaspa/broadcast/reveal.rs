use std::sync::Arc;

use kaspa_txscript::{opcodes::codes::OpTrue, script_builder::ScriptBuilder};
use kaspa_wrpc_client::KaspaRpcClient;
use stroemnet_protocol::v1::{CommitmentV1, RevealV1};

use super::super::error::{Result, script_err};
use super::prepare::prepare_htlc_spend;
use super::spend::{SpendParams, submit_htlc_spend};

/// Submit a reveal across the kaspa network effectively claiming the swap
pub(crate) async fn submit_reveal(
    client: &Arc<KaspaRpcClient>, // the kaspa rpc client
    private_key: &str,            // the private key of signer
    coinbase_maturity: u64,       // how many daa to wait until the miner utxo is spendable
    commitment: &CommitmentV1,    // the initial commitment for this chain
    reveal: &RevealV1,            // the reveal details
) -> Result<()> {
    // prepare the commitment for spending
    let ctx = prepare_htlc_spend(client, private_key, coinbase_maturity, commitment).await?;

    // Prepare the calldata executing the claim branch and providing the secret of the htlc hash
    let branch_sig_script = ScriptBuilder::new()
        .add_data(&reveal.secret)
        .map_err(script_err)?
        .add_op(OpTrue)
        .map_err(script_err)?
        .add_data(&ctx.htlc_script)
        .map_err(script_err)?
        .drain();

    // Submit the htlc spend onchain
    submit_htlc_spend(
        client,
        &ctx,
        SpendParams {
            dest_spk: &ctx.receiver_spk,
            lock_time: 0,
            extra_sig_bytes: 300,
            branch_sig_script,
            log_label: "CCR reveal",
        },
    )
    .await
}

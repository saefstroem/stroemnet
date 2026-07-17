use std::sync::Arc;

use kaspa_consensus_core::tx::ScriptPublicKey;
use kaspa_rpc_core::RpcUtxosByAddressesEntry;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_txscript::{extract_script_pub_key_address, pay_to_script_hash_script};
use kaspa_wrpc_client::KaspaRpcClient;
use stroemnet_protocol::v1::CommitmentV1;

use super::super::error::{KaspaError, Result};
use super::htlc::htlc_script_from_commitment;
use super::signer::Signer340;
use super::utxo::{prefix_for, utxo_is_mature};
use crate::chains::net::retry_timed;

/// A struct organizing the expenditure of an HTLC
/// that is ready to be spend onchain
pub(super) struct HtlcSpend {
    /// The signer who is spending this HTLC
    pub signer: Signer340,
    /// The HTLC script to be spent
    pub htlc_script: Vec<u8>,
    /// The senders spk
    pub sender_spk: ScriptPublicKey,
    /// The receivers spk
    pub receiver_spk: ScriptPublicKey,
    /// The timestamp at which the htlc is refundable
    pub unlock_ts_ms: u64,
    /// All the UTXOs locked to this particular HTLC
    pub htlc_utxos: Vec<RpcUtxosByAddressesEntry>,
    /// The fee utxos that are to be used to spend this htlc
    pub fee_utxo: RpcUtxosByAddressesEntry,
}

/// Prepared the htlc to be spent onchain
pub(super) async fn prepare_htlc_spend(
    client: &Arc<KaspaRpcClient>, // the kaspa rpc client
    private_key: &str,            // private keyof signer
    coinbase_maturity: u64,       // how many daa score a coinbase utxo has to be matured
    commitment: &CommitmentV1,    // commitment of the htlc
) -> Result<HtlcSpend> {
    // Compute the prefix for this network
    let prefix = prefix_for(client).await?;

    // Derive the signer
    let signer = Signer340::derive(private_key, prefix)?;

    // Compute the htlc script from the provided commitment
    let (htlc_script, sender_spk, receiver_spk, unlock_ts_ms) =
        htlc_script_from_commitment(commitment)?;

    // The htlc spk
    let htlc_spk = pay_to_script_hash_script(&htlc_script);

    // The htlc p2sh address
    let htlc_address = extract_script_pub_key_address(&htlc_spk, prefix)?;

    // Retrieve all the utxos locked with this p2sh address
    let htlc_utxos = retry_timed("get_utxos htlc", || {
        client.get_utxos_by_addresses(vec![htlc_address.clone()])
    })
    .await
    .ok_or_else(|| KaspaError::Other("get_utxos htlc: timed out".into()))?;
    if htlc_utxos.is_empty() {
        return Err(KaspaError::HtlcUtxoNotFound(commitment.swap_id));
    }

    // Retrieve the signers utxos that will be used to pay transaction fees
    let our_utxos = retry_timed("get_utxos self", || {
        client.get_utxos_by_addresses(vec![signer.address()])
    })
    .await
    .ok_or_else(|| KaspaError::Other("get_utxos self: timed out".into()))?;

    // Retrieve the dag info so that we can know the daa score of this node
    let dag_info = retry_timed("get_block_dag_info", || client.get_block_dag_info())
        .await
        .ok_or_else(|| KaspaError::Other("get_block_dag_info: timed out".into()))?;
    let current_daa = dag_info.virtual_daa_score;

    // Retrieve all the fee utxos that are mature to be used for fee subsidy
    let fee_utxo = our_utxos
        .iter()
        .find(|u| utxo_is_mature(u, coinbase_maturity, current_daa))
        .ok_or(KaspaError::NoUtxos)?
        .clone();

    Ok(HtlcSpend {
        signer,
        htlc_script,
        sender_spk,
        receiver_spk,
        unlock_ts_ms,
        htlc_utxos,
        fee_utxo,
    })
}

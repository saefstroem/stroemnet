use std::sync::Arc;

use k256::schnorr::SigningKey;
use k256::schnorr::signature::hazmat::PrehashSigner;
use kaspa_addresses::{Address, Prefix, Version};
use kaspa_consensus_core::constants::{STORAGE_MASS_PARAMETER, TRANSIENT_BYTE_TO_MASS_FACTOR};
use kaspa_consensus_core::hashing::sighash::{
    SigHashReusedValuesUnsync, calc_schnorr_signature_hash,
};
use kaspa_consensus_core::hashing::sighash_type::SIG_HASH_ALL;
use kaspa_consensus_core::mass::MassCalculator;
use kaspa_consensus_core::subnets::SUBNETWORK_ID_NATIVE;
use kaspa_consensus_core::tx::{
    MutableTransaction, ScriptPublicKey, Transaction, TransactionInput, TransactionOutpoint,
    TransactionOutput, UtxoEntry,
};
use kaspa_rpc_core::RpcUtxosByAddressesEntry;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_txscript::{
    SEQUENCE_LOCK_TIME_DISABLED, extract_script_pub_key_address, opcodes::codes::OpFalse,
    opcodes::codes::OpTrue, pay_to_address_script, pay_to_script_hash_script,
    script_builder::ScriptBuilder,
};
use kaspa_wrpc_client::KaspaRpcClient;
use stroemnet_protocol::v1::{CommitmentV1, RevealV1};

use super::contracts::contract_v1::{SOLVER_REWARD, create_htlc_script};
use super::error::{KaspaError, Result};
// Mass parameters for fee calculation.
const MASS_PER_TX_BYTE: u64 = 1;
const MASS_PER_SCRIPT_PUB_KEY_BYTE: u64 = 10;
const MASS_PER_SIG_OP: u64 = 1000;
const SCHNORR_SIG_SCRIPT_SIZE: u64 = 66;

/// The resulting HTLC address and redeem script after
/// preparing and submitting a commitment over the network.
pub(super) struct Announce {
    pub address: String,
    pub redeem_script: Vec<u8>,
}

/// A bip340 signer that derives the signing key and
/// public key from a given private key string and network prefix.
/// To produce schnorr signatures for Kaspa transactions, which are used in HTLC scripts.
struct Signer340 {
    key: SigningKey,
    pubkey: [u8; 32],
    prefix: Prefix,
}

impl Signer340 {
    /// Derive `Self` from a hex-encoded private key string and a Kaspa network prefix.
    fn derive(private_key: &str, prefix: Prefix) -> Result<Self> {
        // Remove any kind of 0x prefix if its present
        let secret = hex::decode(private_key.trim_start_matches("0x"))
            .map_err(|e| KaspaError::Other(format!("private key hex: {e}")))?;
        let key = SigningKey::from_bytes(&secret)
            .map_err(|e| KaspaError::Other(format!("schnorr signing key: {e}")))?;
        let pubkey: [u8; 32] = key
            .verifying_key()
            .to_bytes()
            .as_slice()
            .try_into()
            .map_err(|_| KaspaError::Other("verifying key not 32 bytes".into()))?;
        Ok(Self {
            key,
            pubkey,
            prefix,
        })
    }

    // Retrieve the kaspa address
    fn address(&self) -> Address {
        Address::new(self.prefix, Version::PubKey, &self.pubkey)
    }

    // Compute the script public key for the signer's address
    fn spk(&self) -> ScriptPublicKey {
        pay_to_address_script(&self.address())
    }

    /// Sign the input at the given index of the provided mutable transaction, returning the signature script.
    fn sign_input(
        &self,
        mutable_tx: &MutableTransaction<Transaction>,
        index: usize,
    ) -> Result<Vec<u8>> {
        let reused_values = SigHashReusedValuesUnsync::new();
        let sig_hash = calc_schnorr_signature_hash(
            &mutable_tx.as_verifiable(),
            index,
            SIG_HASH_ALL,
            &reused_values,
        );
        let sig: k256::schnorr::Signature =
            self.key
                .sign_prehash(sig_hash.as_bytes().as_slice())
                .map_err(|e| KaspaError::Other(format!("schnorr sign: {e}")))?;
        let mut signature = Vec::with_capacity(65);
        signature.extend_from_slice(&sig.to_bytes());
        signature.push(SIG_HASH_ALL.to_u8());
        Ok(ScriptBuilder::new()
            .add_data(&signature)
            .map_err(|e| KaspaError::ScriptBuilder(format!("{e:?}")))?
            .drain())
    }
}

/// Compute the appropriate address prefix for the Kaspa network we are connected to (mainnet, testnet, etc.)
async fn prefix_for(client: &Arc<KaspaRpcClient>) -> Result<Prefix> {
    let network_type = client.get_server_info().await?.network_id.network_type;
    Ok(network_type.into())
}

/// Calculate the priority fee for a transaction based on its mass and the current fee estimates from the Kaspa network.
async fn calculate_priority_fee(
    client: &Arc<KaspaRpcClient>,
    tx: &Transaction,
    extra_sig_script_bytes: u64,
) -> Result<u64> {
    // Retrieve the fee estimate from the Kaspa network, which includes the feerate for the priority bucket.
    let fee_estimate = client.get_fee_estimate().await?;
    let feerate = fee_estimate.priority_bucket.feerate;

    // Instantiate the mass calculator
    let mass_calc = MassCalculator::new(
        MASS_PER_TX_BYTE,
        MASS_PER_SCRIPT_PUB_KEY_BYTE,
        MASS_PER_SIG_OP,
        STORAGE_MASS_PARAMETER,
    );

    // Compute the noncontextual masses
    let non_contextual = mass_calc.calc_non_contextual_masses(tx);

    // Now compute the esimated schnorr signature size based on the number of inputs and their sig op counts
    let schnorr_sig_bytes: u64 = tx
        .inputs
        .iter()
        .filter(|input| input.sig_op_count > 0)
        .count() as u64
        * SCHNORR_SIG_SCRIPT_SIZE;

    // Compute the total signature bytes.
    let total_sig_bytes = schnorr_sig_bytes + extra_sig_script_bytes;

    // Finally compute both the compute and transient mass used for fee estimation
    let compute_mass = non_contextual.compute_mass + total_sig_bytes * MASS_PER_TX_BYTE;
    let transient_mass =
        non_contextual.transient_mass + total_sig_bytes * TRANSIENT_BYTE_TO_MASS_FACTOR;

    // Take whatever is bigger
    let mass = compute_mass.max(transient_mass);

    // Multiply the mass by the feerate and round up to the nearest integer, ensuring a minimum fee of 1.
    Ok(((mass as f64 * feerate).ceil() as u64).max(1))
}

/// Converts a Kaspa spk to a byte vector, prefixing it with its version.
pub(super) fn spk_to_vec(spk: &ScriptPublicKey) -> Vec<u8> {
    let mut v = Vec::with_capacity(2 + spk.script().len());
    v.extend_from_slice(&spk.version.to_be_bytes());
    v.extend_from_slice(spk.script());
    v
}

/// Convert and RPC UTXO to a UtxoEntry
fn rpc_utxo_to_entry(u: &RpcUtxosByAddressesEntry) -> UtxoEntry {
    UtxoEntry::new(
        u.utxo_entry.amount,
        ScriptPublicKey::new(
            u.utxo_entry.script_public_key.version,
            u.utxo_entry.script_public_key.script().into(),
        ),
        u.utxo_entry.block_daa_score,
        u.utxo_entry.is_coinbase,
    )
}
/// Returns whether a utxo is mature which is when it is not a coinbase
/// or if it is a coinbase it has enough confirmations based on the current DAA score and the coinbase maturity parameter.
fn utxo_is_mature(
    utxo: &RpcUtxosByAddressesEntry,
    coinbase_maturity: u64,
    current_daa: u64,
) -> bool {
    !utxo.utxo_entry.is_coinbase
        || utxo.utxo_entry.block_daa_score + coinbase_maturity <= current_daa
}

/// Construct a raw HTLC script and its associated sender and receiver script public keys from a given commitment.
fn htlc_script_from_commitment(
    commitment: &CommitmentV1,
) -> Result<(Vec<u8>, ScriptPublicKey, ScriptPublicKey, u64)> {
    // Convert the unlock timestamp to milliseconds, as Kaspa uses millisecond precision for lock times in scripts.
    let unlock_ts_ms = commitment.unlock_ts.saturating_mul(1000);

    // Compute the sender spk
    let sender_spk =
        pay_to_address_script(&Address::try_from(commitment.addresses.sender.clone())?);

    // Compute the receiver spk
    let receiver_spk =
        pay_to_address_script(&Address::try_from(commitment.addresses.receiver.clone())?);

    // Create the HTLC redeem script using the provided commitment details,
    // including the sender and receiver script public keys, secret hash, unlock time, destination, and swap ID.
    let htlc_script = create_htlc_script(
        &spk_to_vec(&sender_spk),
        commitment.addresses.sender_destination.as_bytes(),
        &spk_to_vec(&receiver_spk),
        &commitment.secret_hash,
        unlock_ts_ms,
        commitment.destination,
        commitment.swap_id,
    )
    .map_err(|e| KaspaError::ScriptBuilder(format!("{e:?}")))?;

    Ok((htlc_script, sender_spk, receiver_spk, unlock_ts_ms))
}

/// Submits an HTLC commitment, locking funds in a script until
/// they are either claimed by the receiver with the preimage or refunded to the sender after timeout.
pub(super) async fn submit_commitment(
    client: &Arc<KaspaRpcClient>,
    private_key: &str,
    coinbase_maturity: u64,
    commitment: &CommitmentV1,
) -> Result<Announce> {
    // Compute the kaspa network prefix
    // since on kaspa testnet and mainnet have different address prefixes
    let prefix = prefix_for(client).await?;

    // Derive the signer from the provided private key and network prefix
    let signer = Signer340::derive(private_key, prefix)?;

    // Create the htlc script, we dont need sender,receiver and unlock time for this
    // those are used more often for reveal and refund, arguably those could be extracted to their own
    // helpers. But we will keep it simple for now.
    let (htlc_script, _sender_spk, _receiver_spk, _unlock_ts_ms) =
        htlc_script_from_commitment(commitment)?;

    // Now compute the spk of the htlc we just created
    let htlc_spk = pay_to_script_hash_script(&htlc_script);
    let our_spk = signer.spk();

    // We need to fund the HTLC output and therefore we need to select some of our UTXOs as inputs for the transaction.
    let utxos = client
        .get_utxos_by_addresses(vec![signer.address()])
        .await?;
    if utxos.is_empty() {
        // if there are no utxos then there are no funds
        return Err(KaspaError::NoUtxos);
    }

    // Retrieve dag info to get the current DAA score,
    // which we will use to filter out immature coinbase UTXOs and ensure selected UTXOs are mature enough to be spent.
    // since technically some LP's could be miners as well
    let dag_info = client.get_block_dag_info().await?;
    let current_daa = dag_info.virtual_daa_score;
    let amount: u64 = commitment.amount.value.parse()?;

    // Create a container for the selected utxos and total input amount
    // so that we can compute the appropriate change amount
    let mut selected_utxos = Vec::new();
    let mut total_input: u64 = 0;
    for utxo in utxos {
        // If this is a coinbase UTXO we need to ensure it is mature before trying to spend it.
        if !utxo_is_mature(&utxo, coinbase_maturity, current_daa) {
            continue;
        }
        // Add the UTXO to our selection and update the total input amount.
        total_input += utxo.utxo_entry.amount;

        // Add it as a selected UTXO
        selected_utxos.push(utxo);

        // If we have enough total input, we can stop here.
        if total_input >= amount {
            break;
        }
    }
    if total_input < amount {
        return Err(KaspaError::InsufficientFunds {
            needed: amount,
            available: total_input,
        });
    }

    // Compute the transaction inputs from the selected UTXOs,
    // creating a TransactionInput for each one with an empty signature script for now.
    let inputs: Vec<TransactionInput> = selected_utxos
        .iter()
        .enumerate()
        .map(|(seq, utxo)| TransactionInput {
            previous_outpoint: TransactionOutpoint::new(
                utxo.outpoint.transaction_id,
                utxo.outpoint.index,
            ),
            signature_script: vec![],
            sequence: seq as u64,
            sig_op_count: 1,
        })
        .collect();

    // Create output containing just the htlc output for now, we will add change later
    let mut outputs = vec![TransactionOutput::new(amount, htlc_spk.clone())];
    let preliminary_change = total_input.saturating_sub(amount);
    if preliminary_change > 0 {
        // if there is change, we should add a change output to our own spk.
        outputs.push(TransactionOutput::new(preliminary_change, our_spk.clone()));
    }

    // Create a preliminary transaction with the selected inputs and outputs,
    // which we will use to calculate the appropriate fee based on its mass.
    let preliminary_tx = Transaction::new(
        0,
        inputs.clone(),
        outputs,
        0,
        SUBNETWORK_ID_NATIVE,
        0,
        vec![],
    );

    // Compute the priority fee needed for this transaction.
    let fee = calculate_priority_fee(client, &preliminary_tx, 0).await?;

    // If we dont have enough total input to cover both the amount and the fee, we need to return an error.
    if total_input < amount + fee {
        return Err(KaspaError::InsufficientFunds {
            needed: amount + fee,
            available: total_input,
        });
    }

    // Now compute the actual change amount after accounting for the fee,
    // and construct the final outputs for the transaction.
    let change = total_input.saturating_sub(amount).saturating_sub(fee);
    let mut final_outputs = vec![TransactionOutput::new(amount, htlc_spk.clone())];
    if change > 0 {
        // If there is change, add the change output to the final outputs.
        final_outputs.push(TransactionOutput::new(change, our_spk.clone()));
    }

    // Create the final transaction with the final inputs and outputs
    let tx = Transaction::new(0, inputs, final_outputs, 0, SUBNETWORK_ID_NATIVE, 0, vec![]);

    // Compute all the utxo entries for the selected UTXOs, which we will need to sign the transaction.
    let utxo_entries: Vec<UtxoEntry> = selected_utxos.iter().map(rpc_utxo_to_entry).collect();

    // Create a mutable tx that we can sign
    let mut mutable_tx = MutableTransaction::with_entries(tx, utxo_entries);

    // For each of the inputs, sign the input and populate the signature script
    // using our Signer340, which produces schnorr signatures for Kaspa transactions.
    for i in 0..selected_utxos.len() {
        mutable_tx.tx.inputs[i].signature_script = signer.sign_input(&mutable_tx, i)?;
    }

    // Convert the mutable transaction into an RpcTransaction
    let rpc_tx = (&mutable_tx.tx).into();

    // Broadcast the transaction to the Kaspa network using the RPC client, and retrieve the resulting transaction ID.
    let tx_id = client.submit_transaction(rpc_tx, false).await?;
    tracing::info!("Kaspa HTLC commitment submitted: txid {tx_id}");

    // Finally, return the address and redeem script of the HTLC so
    // that the receiver can monitor for it and claim it with the preimage.
    let address = extract_script_pub_key_address(&htlc_spk, prefix)?.to_string();
    Ok(Announce {
        address,
        redeem_script: htlc_script,
    })
}

/// A container for all the necessary information to prepare and submit an HTLC spend transaction,
struct HtlcSpend {
    signer: Signer340,
    htlc_script: Vec<u8>,
    sender_spk: ScriptPublicKey,
    receiver_spk: ScriptPublicKey,
    unlock_ts_ms: u64,
    htlc_utxos: Vec<RpcUtxosByAddressesEntry>,
    fee_utxo: RpcUtxosByAddressesEntry,
}

/// Prepares an HTLC to be spent
async fn prepare_htlc_spend(
    client: &Arc<KaspaRpcClient>,
    private_key: &str,
    coinbase_maturity: u64,
    commitment: &CommitmentV1,
    stored_script: Option<&[u8]>,
) -> Result<HtlcSpend> {
    // Compute the kaspa network prefix for the connected client.
    let prefix = prefix_for(client).await?;

    // Derive the signer from the provided private key and network prefix.
    let signer = Signer340::derive(private_key, prefix)?;

    let (derived_script, sender_spk, receiver_spk, unlock_ts_ms) =
        htlc_script_from_commitment(commitment)?;
    let htlc_script = match stored_script {
        Some(s) => s.to_vec(),
        None => derived_script,
    };

    // Compute the spk of the htlc.
    let htlc_spk = pay_to_script_hash_script(&htlc_script);

    // Extract the htlc address from the htlc spk so that we can query for the UTXOs
    let htlc_address = extract_script_pub_key_address(&htlc_spk, prefix)?;

    // Retrieve the UTXOs for the HTLC address, which are essentially the locked funds
    // we need to unlock, either due to CCR or because this is our counter that we should claim
    let htlc_utxos = client.get_utxos_by_addresses(vec![htlc_address]).await?;
    if htlc_utxos.is_empty() {
        return Err(KaspaError::HtlcUtxoNotFound(commitment.swap_id));
    }

    // Because the HTLC enforces output to the owner of the swap we need to provide a fee from our side to claim the
    // HTLC
    let our_utxos = client
        .get_utxos_by_addresses(vec![signer.address()])
        .await?;

    // Retrieve dag info
    let dag_info = client.get_block_dag_info().await?;
    let current_daa = dag_info.virtual_daa_score;
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

/// Parameters required in order to spend an HTLC,
/// either for a reveal or a refund, which have different script paths
/// but largely the same requirements in terms of inputs and signing.
struct SpendParams<'a> {
    /// The destination script public key where the funds will be sent after claiming the HTLC,
    dest_spk: &'a ScriptPublicKey,
    /// Sequence for the htcl input
    htlc_sequence: u64,
    /// Sequence for the fee input, just a regular utxo
    fee_sequence: u64,
    /// The unlock time in milliseconds interchangeable with `unlock_ts_ms`
    lock_time: u64,
    /// An estimate of the extra bytes that will be added to the transaction by the signature scripts,
    extra_sig_bytes: u64,
    /// The sig script which contains information about
    /// whether to take the reveal path or the refund path in the HTLC script,
    /// as well as the preimage in case of reveal.
    branch_sig_script: Vec<u8>,
    /// For loggin only
    log_label: &'a str,
}

/// A helper function to submit a htlc spending transaction either
/// as a reveal or a refund depending on the provided `branch_sig_script`
async fn submit_htlc_spend(
    client: &Arc<KaspaRpcClient>,
    ctx: &HtlcSpend,
    params: SpendParams<'_>,
) -> Result<()> {
    let our_spk = ctx.signer.spk();

    // There could be multiple utxos for the same htlc either by accident,
    // griefing attempt, so we need to go over all utxos that match the htlc address
    for utxo in ctx.htlc_utxos.iter() {
        // Compute the destination amount which is the amount locked in the HTLC minus the solver reward,
        let dest_amount = utxo
            .utxo_entry
            .amount
            .checked_sub(SOLVER_REWARD as u64)
            .ok_or(KaspaError::InsufficientFunds {
                needed: SOLVER_REWARD as u64,
                available: utxo.utxo_entry.amount,
            })?;

        // Solver reward technically includes the fee utxo as well
        let solver_reward_before_fee = (SOLVER_REWARD as u64)
            .checked_add(ctx.fee_utxo.utxo_entry.amount)
            .ok_or_else(|| KaspaError::Other("Solver reward + fee UTXO overflow".to_string()))?;

        // Create inputs for the transaction where as per protocol
        // the first input is always the HTLC utxo and the second
        // input is the fee utxo from our wallet that we will use to pay for the transaction.
        let inputs = vec![
            TransactionInput {
                previous_outpoint: TransactionOutpoint::new(
                    utxo.outpoint.transaction_id,
                    utxo.outpoint.index,
                ),
                signature_script: vec![],
                sequence: params.htlc_sequence,
                sig_op_count: 0,
            },
            TransactionInput {
                previous_outpoint: TransactionOutpoint::new(
                    ctx.fee_utxo.outpoint.transaction_id,
                    ctx.fee_utxo.outpoint.index,
                ),
                signature_script: vec![],
                sequence: params.fee_sequence,
                sig_op_count: 1,
            },
        ];

        // Create preliminary outputs for the transaction,
        // which include the destination output for the HTLC claim and a solver reward output to our own spk,
        let preliminary_outputs = vec![
            TransactionOutput::new(dest_amount, params.dest_spk.clone()),
            TransactionOutput::new(solver_reward_before_fee, our_spk.clone()),
        ];

        // Create a preliminary tx so that we can estimate priority fees
        let preliminary_tx = Transaction::new(
            0,
            inputs.clone(),
            preliminary_outputs,
            params.lock_time,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );

        // Compute the priority fee
        let fee = calculate_priority_fee(client, &preliminary_tx, params.extra_sig_bytes).await?;

        // Now compute the solver reward after accounting for the fee
        let solver_reward =
            solver_reward_before_fee
                .checked_sub(fee)
                .ok_or(KaspaError::InsufficientFunds {
                    needed: fee,
                    available: solver_reward_before_fee,
                })?;

        // Compute the final outputs
        let outputs = vec![
            TransactionOutput::new(dest_amount, params.dest_spk.clone()),
            TransactionOutput::new(solver_reward, our_spk.clone()),
        ];

        // Create the final transaction with the finalized inputs and outputs, and the provided lock time.
        let tx = Transaction::new(
            0,
            inputs,
            outputs,
            params.lock_time,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );

        // Create the utxo entries used for signing
        let utxo_entries = vec![rpc_utxo_to_entry(utxo), rpc_utxo_to_entry(&ctx.fee_utxo)];

        // Now create a mutable transaction that we can sign for
        let mut mutable_tx = MutableTransaction::with_entries(tx, utxo_entries);

        // The htlc input should be signed with the signature script in the branch signature script
        mutable_tx.tx.inputs[0].signature_script = params.branch_sig_script.clone();

        // The signature script for the fee utxo is a regular utxo so therefore
        // it should use the signer to produce a schnorr signature for the input
        mutable_tx.tx.inputs[1].signature_script = ctx.signer.sign_input(&mutable_tx, 1)?;

        // Finalize the transaction by converting to rpc transaction
        let rpc_tx = (&mutable_tx.tx).into();

        // Broadcast the transaction over p2p
        let tx_id = client.submit_transaction(rpc_tx, false).await?;
        tracing::info!("Kaspa {} submitted: txid {tx_id}", params.log_label);
    }
    Ok(())
}

/// Submit the reveal transaction
pub(super) async fn submit_reveal(
    client: &Arc<KaspaRpcClient>,
    private_key: &str,
    coinbase_maturity: u64,
    commitment: &CommitmentV1,
    reveal: &RevealV1,
    stored_script: Option<&[u8]>,
) -> Result<()> {
    // Prepare the htlc for spending which essentially means gathering
    // all the necessary information and UTXOs for signing and broadcasting the transaction
    let ctx =
        prepare_htlc_spend(client, private_key, coinbase_maturity, commitment, stored_script)
            .await?;

    // We want to execute the branch that is the claim branch
    // and for that we need to push optrue and preimage as a signature
    // followed by the original htlc script as `redeem script` for the p2sh
    let branch_sig_script = ScriptBuilder::new()
        .add_data(&reveal.secret)
        .map_err(|e| KaspaError::ScriptBuilder(format!("{e:?}")))?
        .add_op(OpTrue)
        .map_err(|e| KaspaError::ScriptBuilder(format!("{e:?}")))?
        .add_data(&ctx.htlc_script)
        .map_err(|e| KaspaError::ScriptBuilder(format!("{e:?}")))?
        .drain();

    // Now we can just submit it to the helper which will
    // submit it across the network
    submit_htlc_spend(
        client,
        &ctx,
        SpendParams {
            dest_spk: &ctx.receiver_spk,
            htlc_sequence: 0,     // htlc sequence 0
            fee_sequence: 1,      // fee sequence 1
            lock_time: 0,         // we dont need lock time
            extra_sig_bytes: 300, // estimate roughly 300 bytes for the reveal todo:have exact value
            branch_sig_script,
            log_label: "CCR reveal", // logging only
        },
    )
    .await
}

/// Submit the refund transaction
pub(super) async fn submit_refund(
    client: &Arc<KaspaRpcClient>,
    private_key: &str,
    coinbase_maturity: u64,
    commitment: &CommitmentV1,
    stored_script: Option<&[u8]>,
) -> Result<()> {
    // Prepare the htlc for spending which essentially means gathering
    // all the necessary information and UTXOs for signing and broadcasting the transaction
    let ctx =
        prepare_htlc_spend(client, private_key, coinbase_maturity, commitment, stored_script)
            .await?;

    // This time we want to trigger refund branch which effectively just means
    // passing opfalse and setting proper lock time for the transaction
    let branch_sig_script = ScriptBuilder::new()
        .add_op(OpFalse)
        .map_err(|e| KaspaError::ScriptBuilder(format!("{e:?}")))?
        .add_data(&ctx.htlc_script)
        .map_err(|e| KaspaError::ScriptBuilder(format!("{e:?}")))?
        .drain();

    // Submit the htlc for spending
    submit_htlc_spend(
        client,
        &ctx,
        SpendParams {
            dest_spk: &ctx.sender_spk,
            htlc_sequence: SEQUENCE_LOCK_TIME_DISABLED,
            fee_sequence: SEQUENCE_LOCK_TIME_DISABLED,
            lock_time: ctx.unlock_ts_ms,
            extra_sig_bytes: 260,
            branch_sig_script,
            log_label: "refund",
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaspa_consensus_core::tx::VerifiableTransaction;
    use kaspa_hashes::Hash;
    use kaspa_txscript::{TxScriptEngine, caches::Cache};

    #[test]
    fn production_signer_p2pk_input_verifies_on_engine() {
        let signer = Signer340::derive(
            "1111111111111111111111111111111111111111111111111111111111111111",
            Prefix::Testnet,
        )
        .unwrap();
        let spk = signer.spk();
        let input_value = 1_000_000u64;
        let input = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(1), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 1,
        };
        let output = TransactionOutput::new(input_value - 1_000, spk.clone());
        let tx = Transaction::new(
            0,
            vec![input],
            vec![output],
            0,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );
        let entry = UtxoEntry::new(input_value, spk.clone(), 0, false);
        let mut mutable_tx = MutableTransaction::with_entries(tx, vec![entry]);
        mutable_tx.tx.inputs[0].signature_script = signer.sign_input(&mutable_tx, 0).unwrap();

        let reused = SigHashReusedValuesUnsync::new();
        let verifiable = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let utxo_entry = verifiable.utxo(0).unwrap().clone();
        let mut vm = TxScriptEngine::from_transaction_input(
            &verifiable,
            &verifiable.inputs()[0],
            0,
            &utxo_entry,
            &reused,
            &sig_cache,
        );
        vm.execute()
            .expect("production-signed P2PK input must satisfy CHECKSIG");
    }
}

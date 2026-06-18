use k256::schnorr::signature::{Signer, Verifier};
use k256::schnorr::{Signature, SigningKey, VerifyingKey};
use kaspa_addresses::{Address, Prefix, Version};
use kaspa_consensus_core::hashing::sighash::SigHashReusedValuesUnsync;
use kaspa_consensus_core::tx::ScriptPublicKey;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_txscript::opcodes::{OpCodeImplementation, deserialize_next_opcode};
use kaspa_txscript::{
    extract_script_pub_key_address, pay_to_address_script, pay_to_script_hash_script,
};
use kaspa_wrpc_client::KaspaRpcClient;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::CommitmentV1;

use crate::ProposalVerification;

use super::contracts::contract_v1::{
    VerifiableTransactionMock, create_htlc_script, extract_commitment,
};
use super::error::{KaspaError, Result};

/// Compute the network prefix from a string to a network prefix enum
fn parse_network_prefix(network_id: &str) -> Result<Prefix> {
    match network_id {
        s if s.starts_with("mainnet") => Ok(Prefix::Mainnet),
        s if s.starts_with("testnet") => Ok(Prefix::Testnet),
        s if s.starts_with("simnet") => Ok(Prefix::Simnet),
        s if s.starts_with("devnet") => Ok(Prefix::Devnet),
        other => Err(KaspaError::Other(format!(
            "unknown kaspa network id: {other}"
        ))),
    }
}

/// Convert a ScriptPublicKey to the byte format expected by the HTLC script builder
fn spk_to_bytes(spk: &ScriptPublicKey) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + spk.script().len());
    out.extend_from_slice(&spk.version.to_be_bytes());
    out.extend_from_slice(spk.script());
    out
}

/// Compute a signing key from a hex-encoded private key string
fn signing_key(private_key: &str) -> Result<SigningKey> {
    let secret_bytes = hex::decode(private_key.trim_start_matches("0x"))
        .map_err(|e| KaspaError::Other(format!("private key hex: {e}")))?;
    SigningKey::from_bytes(&secret_bytes)
        .map_err(|e| KaspaError::Other(format!("schnorr signing key: {e}")))
}

/// Compute the public key bytes from a signing key
fn pubkey_bytes(key: &SigningKey) -> Result<[u8; 32]> {
    key.verifying_key()
        .to_bytes()
        .as_slice()
        .try_into()
        .map_err(|_| KaspaError::Other("verifying key not 32 bytes".into()))
}

/// Derive the LP's address from the private key and network ID
pub(super) fn lp_address_from_private_key(network_id: &str, private_key: &str) -> Result<String> {
    // retrieve the signing key from the provided private key string
    let key = signing_key(private_key)?;
    let prefix = parse_network_prefix(network_id)?;
    // construct the address from the public key and network prefix
    Ok(Address::new(prefix, Version::PubKey, &pubkey_bytes(&key)?).to_string())
}

/// Queries the balance of the given address by summing the amounts of all UTXOs associated with it
async fn query_balance(client: &KaspaRpcClient, address: &str) -> Result<u64> {
    let parsed_addr = Address::try_from(address)
        .map_err(|e| KaspaError::Other(format!("invalid kaspa address {address}: {e}")))?;
    let utxos = client
        .get_utxos_by_addresses(vec![parsed_addr])
        .await
        .map_err(|e| KaspaError::Other(format!("get_utxos_by_addresses: {e}")))?;
    Ok(utxos.into_iter().map(|u| u.utxo_entry.amount).sum())
}

/// Signs a message and simultaneously verifies that the address in question
/// has sufficient balance to cover the required amount, which is a prerequisite for the LP's commitment to be valid.
pub(super) async fn sign_message(
    client: &KaspaRpcClient,
    network_id: &str,
    private_key: &str,
    digest: [u8; 32],
    required_balance: u64,
) -> Result<(String, Vec<u8>)> {
    // Compute the signing key
    let key = signing_key(private_key)?;

    // Parse network prefix
    let prefix = parse_network_prefix(network_id)?;

    // Create a kaspa address
    let address = Address::new(prefix, Version::PubKey, &pubkey_bytes(&key)?);
    let address_str = address.to_string();

    // Retrieve the balance and ensure it meets the required threshold for the commitment
    let balance = query_balance(client, &address_str).await?;
    if balance < required_balance {
        return Err(KaspaError::Other(format!(
            "insufficient balance at {address_str}: have {balance}, need {required_balance}"
        )));
    }

    // Sign the message digest with the derived signing key
    let signature: Signature = key.sign(&digest);
    Ok((address_str, signature.to_bytes().to_vec()))
}

/// Verifies the LP's signature against the provided digest and claimed address
fn verify_lp_signature(
    digest: [u8; 32],
    claimed_address: &str,
    signature_bytes: &[u8],
) -> Result<bool> {
    // Parse the claimed address
    let claimed = Address::try_from(claimed_address)
        .map_err(|e| KaspaError::Other(format!("invalid claimed address: {e}")))?;

    // ensure it is p2pk
    if claimed.version != Version::PubKey {
        return Err(KaspaError::Other(format!(
            "claimed address is not P2PK (version={:?})",
            claimed.version
        )));
    }

    // Compute the pubkey
    let pubkey: [u8; 32] = claimed.payload.as_slice().try_into().map_err(|_| {
        KaspaError::Other(format!(
            "claimed address payload is {} bytes, expected 32",
            claimed.payload.len()
        ))
    })?;

    // Parse verifying key from the pubkey bytes and verify the signature against the digest
    let verifying_key = VerifyingKey::from_bytes(&pubkey)
        .map_err(|e| KaspaError::Other(format!("verifying key: {e}")))?;

    // Parse the signature bytes into a Schnorr signature and verify it against the digest using the verifying key
    let signature = Signature::try_from(signature_bytes)
        .map_err(|e| KaspaError::Other(format!("signature parse: {e}")))?;
    Ok(verifying_key.verify(&digest, &signature).is_ok())
}

/// Verifies the LP's signature and checks that the claimed address has sufficient balance to cover the required amount
pub(super) async fn verify_message(
    client: &KaspaRpcClient,
    digest: [u8; 32],
    claimed_address: &str,
    signature_bytes: &[u8],
    required_balance: u64,
) -> Result<ProposalVerification> {
    // First verify the signature to ensure the message was signed by the owner of the claimed address
    let address_matches = verify_lp_signature(digest, claimed_address, signature_bytes)?;
    // Then query the balance of the claimed address to ensure it meets the required threshold for the commitment
    let balance = query_balance(client, claimed_address).await?;
    Ok(ProposalVerification {
        address_matches,
        balance_sufficient: balance >= required_balance,
    })
}

/// Computes the p2sh address and redeem script for a given commitment,
/// which are necessary for the LP to monitor the HTLC on-chain and react to events such as deposits or refunds
pub(super) fn p2sh_components(
    network_id: &str,
    commitment: &CommitmentV1,
) -> Result<(String, Vec<u8>)> {
    // Parse the prefix for this network
    let prefix = parse_network_prefix(network_id)?;

    // Parse the sender and receiver addresses from the commitment and convert them to script public keys
    let sender_addr = Address::try_from(commitment.addresses.sender.as_str())
        .map_err(|e| KaspaError::Other(format!("sender address: {e}")))?;
    let receiver_addr = Address::try_from(commitment.addresses.receiver.as_str())
        .map_err(|e| KaspaError::Other(format!("receiver address: {e}")))?;
    let sender_spk = pay_to_address_script(&sender_addr);
    let receiver_spk = pay_to_address_script(&receiver_addr);

    // Compute the unlock timestamp as milliseconds
    let unlock_ts_ms = commitment.unlock_ts.saturating_mul(1000);

    // Create the redeem script for the HTLC using the provided parameters from the commitment
    let redeem_script = create_htlc_script(
        &spk_to_bytes(&sender_spk),
        commitment.addresses.sender_destination.as_bytes(),
        &spk_to_bytes(&receiver_spk),
        &commitment.secret_hash,
        unlock_ts_ms,
        commitment.destination,
        commitment.swap_id,
    )
    .map_err(|e| KaspaError::ScriptBuilder(format!("{e:?}")))?;

    // now compute the spk p2sh for the redeemscript/htlc script
    let p2sh_spk = pay_to_script_hash_script(&redeem_script);
    // Now derive the address so that the LP can monitor it on-chain for deposits and react accordingly
    let p2sh_addr = extract_script_pub_key_address(&p2sh_spk, prefix)
        .map_err(|e| KaspaError::Other(format!("p2sh address derive: {e:?}")))?;

    // return
    Ok((p2sh_addr.to_string(), redeem_script))
}

/// Validate that a redeem script corresponds to the expected
/// swap id, expiration time, and announced address, which is crucial for the LP to ensure
/// that the HTLC they are monitoring on-chain matches the terms of
/// the off-chain commitment they have agreed to with the counterparty.
pub(super) fn validate_script_announce(
    network_id: &str,
    channel_id: ChannelId,
    announced_address: &str,
    redeem_script: &[u8],
    expected_swap_id: [u8; 32],
    expected_expiration_secs: u64,
) -> Result<()> {
    // Parse the network prefix for address derivation
    let prefix = parse_network_prefix(network_id)?;
    // Create an iterator and a container for all the opcodes parsed from the redeem script
    let mut iter = redeem_script.iter();
    let mut opcodes: Vec<
        Box<dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>>,
    > = Vec::new();

    // Parse the redeem script into its constituent opcodes,
    // which allows us to analyze the structure of the script and extract the relevant information for validation
    while let Some(parsed) = deserialize_next_opcode(&mut iter) {
        opcodes.push(parsed.map_err(KaspaError::TxScript)?);
    }

    // Attempt to extract a commitment from the parsed opcodes
    // the "0" amount is a placeholder since the redeem script itself does not contain the amount
    // but it does contain the swap id, unlock timestamp, and other relevant information
    // todo: in the future the part that is extractable from redeem script should be isolated as a sub-struct
    // within commitmentv1.
    let extracted = extract_commitment(&opcodes, "0".to_string(), prefix, channel_id)?;

    // validate swap id
    if extracted.swap_id != expected_swap_id {
        return Err(KaspaError::ScriptAnnounceSwapIdMismatch(
            extracted.swap_id,
            expected_swap_id,
        ));
    }

    // validate unlock timestamp for the swap
    if extracted.unlock_ts != expected_expiration_secs {
        return Err(KaspaError::ScriptAnnounceTimelockMismatch {
            script_secs: extracted.unlock_ts,
            announced_secs: expected_expiration_secs,
        });
    }

    // Compute the p2sh address from the redeem script and validate that it matches the announced address,
    let p2sh_spk = pay_to_script_hash_script(redeem_script);
    let derived = extract_script_pub_key_address(&p2sh_spk, prefix)
        .map_err(|e| KaspaError::Other(format!("p2sh derive: {e:?}")))?;
    let derived_str = derived.to_string();

    // Ensure the address matches what was announced by the counterparty or us.
    if derived_str != announced_address {
        return Err(KaspaError::ScriptAnnounceAddressMismatch {
            announced: announced_address.to_string(),
            derived: derived_str,
        });
    }
    Ok(())
}

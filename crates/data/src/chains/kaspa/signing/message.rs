use k256::schnorr::signature::{Signer, Verifier};
use k256::schnorr::{Signature, VerifyingKey};
use kaspa_addresses::{Address, Version};
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::KaspaRpcClient;

use super::super::error::{KaspaError, Result};
use super::keys::{parse_network_prefix, pubkey_bytes, signing_key};
use crate::ProposalVerification;

/// Query the balance of some address accumulating all their utxos
async fn query_balance(client: &KaspaRpcClient, address: &str) -> Result<u64> {
    let parsed_addr = Address::try_from(address)
        .map_err(|e| KaspaError::Other(format!("invalid kaspa address {address}: {e}")))?;
    let utxos = client
        .get_utxos_by_addresses(vec![parsed_addr])
        .await
        .map_err(|e| KaspaError::Other(format!("get_utxos_by_addresses: {e}")))?;
    Ok(utxos.into_iter().map(|u| u.utxo_entry.amount).sum())
}

/// Sign a message i.e. a swap ensuring the balance has the required balance in order to
/// fulfill the swap
pub(crate) async fn sign_message(
    client: &KaspaRpcClient,
    network_id: &str,
    private_key: &str,
    digest: [u8; 32],
    required_balance: u64,
) -> Result<(String, Vec<u8>)> {
    // Retrieve the signing key
    let key = signing_key(private_key)?;
    // Compute the network prefix
    let prefix = parse_network_prefix(network_id)?;

    // Compute the address
    let address = Address::new(prefix, Version::PubKey, &pubkey_bytes(&key)?);
    let address_str = address.to_string();

    // Ensure that the address has the required minimum amount
    let balance = query_balance(client, &address_str).await?;

    // If not we failed this signing
    if balance < required_balance {
        return Err(KaspaError::Other(format!(
            "insufficient balance at {address_str}: have {balance}, need {required_balance}"
        )));
    }

    let signature: Signature = key.sign(&digest);
    Ok((address_str, signature.to_bytes().to_vec()))
}

/// Verify the LP's signature
fn verify_lp_signature(
    digest: [u8; 32],
    claimed_address: &str,
    signature_bytes: &[u8],
) -> Result<bool> {
    // Compute the address which they claim to be
    let claimed = Address::try_from(claimed_address)
        .map_err(|e| KaspaError::Other(format!("invalid claimed address: {e}")))?;

    // We can only validate p2pk
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

    // Compute verifying key
    let verifying_key = VerifyingKey::from_bytes(&pubkey)
        .map_err(|e| KaspaError::Other(format!("verifying key: {e}")))?;

    // Verify that the signature is valid for claimed address
    let signature = Signature::try_from(signature_bytes)
        .map_err(|e| KaspaError::Other(format!("signature parse: {e}")))?;
    Ok(verifying_key.verify(&digest, &signature).is_ok())
}

/// Verify a message from an LP whilst also ensuring that is has enough balance
/// to cover the swap
pub(crate) async fn verify_message(
    client: &KaspaRpcClient,
    digest: [u8; 32],
    claimed_address: &str,
    signature_bytes: &[u8],
    required_balance: u64,
) -> Result<ProposalVerification> {
    let address_matches = verify_lp_signature(digest, claimed_address, signature_bytes)?;
    let balance = query_balance(client, claimed_address).await?;
    Ok(ProposalVerification {
        address_matches,
        balance_sufficient: balance >= required_balance,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::super::keys::{pubkey_bytes, signing_key};
    use super::verify_lp_signature;
    use k256::schnorr::Signature;
    use k256::schnorr::signature::Signer;
    use kaspa_addresses::{Address, Prefix, Version};

    #[test]
    fn lp_signature_roundtrips() {
        let key = signing_key("0101010101010101010101010101010101010101010101010101010101010101")
            .unwrap();
        let digest = [7u8; 32];
        let sig: Signature = key.sign(&digest);
        let addr = Address::new(
            Prefix::Testnet,
            Version::PubKey,
            &pubkey_bytes(&key).unwrap(),
        );
        assert!(verify_lp_signature(digest, &addr.to_string(), &sig.to_bytes()).unwrap());

        let other = Address::new(Prefix::Testnet, Version::PubKey, &[9u8; 32]);
        assert!(!verify_lp_signature(digest, &other.to_string(), &sig.to_bytes()).unwrap());
    }
}

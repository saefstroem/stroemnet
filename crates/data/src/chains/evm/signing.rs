use std::str::FromStr;

use alloy::primitives::{Address, Signature, U256};
use alloy::providers::Provider;
use alloy::signers::Signer;
use alloy::signers::local::{LocalSignerError, PrivateKeySigner};

use crate::chains::net::retry_timed;
use crate::{DataError, ProposalVerification, Result};

/// Convert private key to an address in string format
pub(super) fn address_from_private_key(private_key: &str) -> Result<String> {
    let signer: PrivateKeySigner = private_key
        .parse()
        .map_err(|e: LocalSignerError| DataError::Sign(format!("local signer: {e}")))?;
    Ok(format!("{}", signer.address()))
}

/// Verify the signature of an LP (address)
/// based on the provided message hash and signature
pub(super) fn verify_lp_signature(
    digest: [u8; 32],
    claimed_address: &str,
    signature_bytes: &[u8],
) -> Result<(String, bool)> {
    let claimed = Address::from_str(claimed_address.trim())
        .map_err(|e| DataError::Sign(format!("invalid claimed address: {e}")))?;
    let signature = Signature::from_raw(signature_bytes)
        .map_err(|e| DataError::Sign(format!("signature parse: {e}")))?;
    let recovered = signature
        .recover_address_from_msg(&digest as &[u8])
        .map_err(|e| DataError::Sign(format!("recover address: {e}")))?;
    Ok((format!("{recovered}"), recovered == claimed))
}

/// Query the balance of an address
pub(super) async fn query_balance<P: Provider>(provider: &P, address: &str) -> Result<U256> {
    let addr = Address::from_str(address.trim())
        .map_err(|e| DataError::Rpc(format!("invalid address {address}: {e}")))?;
    retry_timed("get_balance", || provider.get_balance(addr))
        .await
        .ok_or_else(|| DataError::Rpc("get_balance: timed out".into()))
}

/// Sign a message with a private key and also
/// ensure that the private key has enough balance to cover the minimum requirement
pub(super) async fn sign_message<P: Provider>(
    provider: &P,
    private_key: &str,
    digest: [u8; 32],
    required_balance: U256,
) -> Result<(String, Vec<u8>)> {
    // Parse the signer
    let signer: PrivateKeySigner = private_key
        .parse()
        .map_err(|e: LocalSignerError| DataError::Sign(format!("local signer: {e}")))?;

    let address = signer.address();

    // Query the balance of the address
    let balance = query_balance(provider, &address.to_string()).await?;

    // Having this check here ensures that is irrepresentable to provide a valid signature of a
    // swap quote without having enough balance (assuming a valid an honest node)
    // But the user will verify sig anyway so it doesnt really matter
    if balance < required_balance {
        return Err(DataError::Sign(format!(
            "insufficient balance at {address}: have {balance}, need {required_balance}"
        )));
    }

    // Sign the message with the private key
    let signature = signer
        .sign_message(&digest)
        .await
        .map_err(|e| DataError::Sign(format!("sign_message: {e}")))?;
    Ok((format!("{address}"), signature.as_bytes().to_vec()))
}

/// Verify a message by ensuring that the signature corresponds to the claimed address
/// and also that the claimed address has enough funds to cover the swap amount
pub(super) async fn verify_message<P: Provider>(
    provider: &P,
    digest: [u8; 32],
    claimed_address: &str,
    signature_bytes: &[u8],
    required_balance: U256,
) -> Result<ProposalVerification> {
    let (_signer_address, address_matches) =
        verify_lp_signature(digest, claimed_address, signature_bytes)?;

    let balance = query_balance(provider, claimed_address).await?;

    Ok(ProposalVerification {
        address_matches,
        balance_sufficient: balance >= required_balance,
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;

    fn fixture_digest() -> [u8; 32] {
        let mut d = [0u8; 32];
        for (i, byte) in d.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(7).wrapping_add(13);
        }
        d
    }

    fn random_pk() -> String {
        let s = PrivateKeySigner::random();
        hex::encode(s.credential().to_bytes())
    }

    async fn sign_offline(private_key: &str, digest: [u8; 32]) -> Vec<u8> {
        let signer: PrivateKeySigner = private_key.parse().unwrap();
        signer
            .sign_message(&digest)
            .await
            .unwrap()
            .as_bytes()
            .to_vec()
    }

    #[tokio::test]
    async fn lp_signature_round_trip_recovers_correct_address() {
        let pk = random_pk();
        let signer_addr = address_from_private_key(&pk).unwrap();
        let digest = fixture_digest();
        let sig = sign_offline(&pk, digest).await;
        let (recovered, matches) = verify_lp_signature(digest, &signer_addr, &sig).unwrap();
        assert!(matches);
        assert_eq!(recovered.to_lowercase(), signer_addr.to_lowercase());
    }

    #[tokio::test]
    async fn lp_signature_rejects_wrong_claimed_address() {
        let pk_a = random_pk();
        let pk_b = random_pk();
        let addr_b = address_from_private_key(&pk_b).unwrap();
        let digest = fixture_digest();
        let sig = sign_offline(&pk_a, digest).await;
        let (_recovered, matches) = verify_lp_signature(digest, &addr_b, &sig).unwrap();
        assert!(!matches);
    }

    #[tokio::test]
    async fn lp_signature_rejects_tampered_payload() {
        let pk = random_pk();
        let addr = address_from_private_key(&pk).unwrap();
        let digest = fixture_digest();
        let sig = sign_offline(&pk, digest).await;
        let mut tampered_digest = digest;
        tampered_digest[0] ^= 0x01;
        let (_, matches) = verify_lp_signature(tampered_digest, &addr, &sig).unwrap();
        assert!(!matches);
    }

    #[tokio::test]
    async fn lp_signature_rejects_corrupted_bytes() {
        let pk = random_pk();
        let addr = address_from_private_key(&pk).unwrap();
        let digest = fixture_digest();
        let mut sig = sign_offline(&pk, digest).await;
        sig[10] ^= 0xFF;
        match verify_lp_signature(digest, &addr, &sig) {
            Err(_) => {}
            Ok((_, matches)) => assert!(!matches),
        }
    }

    #[test]
    fn address_derivation_is_deterministic() {
        let seed = [0x42u8; 32];
        let pk = hex::encode(seed);
        let a = address_from_private_key(&pk).unwrap();
        let b = address_from_private_key(&pk).unwrap();
        assert_eq!(a, b);
        assert!(a.starts_with("0x"));
        assert_eq!(a.len(), 42);
    }
}

use alloy::primitives::{Address, FixedBytes};
use alloy::providers::Provider;
use sha2::{Digest, Sha256};
use stroemnet_protocol::v1::RevealV1;

use super::super::GasPayment;
use super::super::contracts::StroemHTLCV1;
use super::super::provider::current_block_timestamp;
use super::apply_gas_and_nonce;
use crate::chains::net::{NETWORK_TIMEOUT, RECEIPT_TIMEOUT, timed};
use crate::{DataError, Result};

/// Submits an EVM HTLC claim over the EVM network
pub(crate) async fn submit_claim<P: Provider>(
    provider: &P,            // a provider
    htlc_address: Address,   // the contract address for the htlc
    reveal: &RevealV1,       // the reveal for the htlc
    nonce: u64, // the nonce for this transaction, we pass it in manually in order to support RBF
    gas_price: u128, // manual specification of gas price for legacy networks
    gas_payment: GasPayment, // Whether we use legacy or eip1559
) -> Result<()> {
    // Create a new HTLC instance
    let stroem_htlc = StroemHTLCV1::new(htlc_address, provider);

    // Create a timed call to query the swap that we are trying to claim
    let swap = timed(
        NETWORK_TIMEOUT,
        stroem_htlc.swaps(FixedBytes::from(reveal.swap_id)).call(),
    )
    .await
    .ok_or_else(|| DataError::Broadcast("swaps lookup: timed out".into()))?
    .map_err(|e| DataError::Broadcast(format!("swaps lookup: {e}")))?;

    // Validate that the secret matches the swap
    let secret_ok = secret_matches(&reveal.secret, swap.secretHash.as_slice());

    // Read the current block timestamp from the provider
    let chain_now = current_block_timestamp(provider).await;

    // Compute the timelock for which the swap is refundable
    let timelock = u64::try_from(swap.timelock).unwrap_or(u64::MAX);

    // Check if this swap has to be skipped
    if let Some(reason) = claim_skip_reason(
        swap.initialized,
        swap.finalized,
        secret_ok,
        chain_now,
        timelock,
    ) {
        tracing::warn!(
            "EVM claim skipped for {}: {reason}",
            hex::encode(reveal.swap_id)
        );
        return Ok(());
    }

    // Build the call
    let base = stroem_htlc.claim(
        FixedBytes::from(reveal.swap_id),
        FixedBytes::from(reveal.secret),
    );

    // Apply the gas for the call
    let call = apply_gas_and_nonce(base, nonce, gas_price, gas_payment);

    // Create a timed blockchain transmission fo the transaction
    let pending = timed(NETWORK_TIMEOUT, call.send())
        .await
        .ok_or_else(|| DataError::Broadcast("claim send: timed out".into()))?
        .map_err(|e| DataError::Broadcast(format!("claim send: {e}")))?;

    // Similarly for the blockchain transmission create a timed call to get the receipt
    let receipt = timed(RECEIPT_TIMEOUT, pending.get_receipt())
        .await
        .ok_or_else(|| DataError::Broadcast("claim receipt: timed out".into()))?
        .map_err(|e| DataError::Broadcast(format!("claim receipt: {e}")))?;

    // If the receipt shows a non-ok status, we revert as well.
    if !receipt.status() {
        return Err(DataError::Broadcast(format!(
            "claim reverted: tx {:?}",
            receipt.transaction_hash
        )));
    }

    // Log and return, todo in the future this should probably be downgraded to a lower log level
    tracing::info!(
        "EVM claim mined in block {:?}, tx {:?}",
        receipt.block_number,
        receipt.transaction_hash
    );
    Ok(())
}

/// Compute a hash and return bool if it matches the claimed secret.
fn secret_matches(secret: &[u8; 32], secret_hash: &[u8]) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(secret);
    hasher.finalize().as_slice() == secret_hash
}

/// A function that quickly determines
/// whether a claim should be skipped. Mostly if its not initialized or finalized
/// or if its expired and should be refunded instead.
fn claim_skip_reason(
    initialized: bool,
    finalized: bool,
    secret_ok: bool,
    chain_now: Option<u64>,
    timelock: u64,
) -> Option<&'static str> {
    if !initialized || finalized {
        return Some("not claimable");
    }
    if !secret_ok {
        return Some("revealed secret does not match on-chain hash");
    }
    match chain_now {
        Some(now) if now >= timelock => Some("timelock expired"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_matches_known_hash() {
        use sha2::{Digest, Sha256};
        let secret = [7u8; 32];
        let hash: [u8; 32] = Sha256::digest(secret).into();
        assert!(secret_matches(&secret, &hash));
        assert!(!secret_matches(&[1u8; 32], &hash));
    }

    #[test]
    fn claim_skip_reason_covers_each_guard() {
        assert_eq!(
            claim_skip_reason(false, false, true, None, 0),
            Some("not claimable")
        );
        assert_eq!(
            claim_skip_reason(true, true, true, None, 0),
            Some("not claimable")
        );
        assert_eq!(
            claim_skip_reason(true, false, false, None, 0),
            Some("revealed secret does not match on-chain hash")
        );
        assert_eq!(
            claim_skip_reason(true, false, true, Some(100), 50),
            Some("timelock expired")
        );
        assert_eq!(claim_skip_reason(true, false, true, Some(10), 50), None);
        assert_eq!(claim_skip_reason(true, false, true, None, 50), None);
    }
}

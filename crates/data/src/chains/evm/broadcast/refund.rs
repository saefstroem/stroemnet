use alloy::primitives::{Address, FixedBytes};
use alloy::providers::Provider;

use super::super::GasPayment;
use super::super::contracts::StroemHTLCV1;
use super::apply_gas_and_nonce;
use crate::chains::net::{NETWORK_TIMEOUT, RECEIPT_TIMEOUT, timed};
use crate::{DataError, Result};

/// Submits a refund over the blockchain for a specific HTLCv1 swap
pub(crate) async fn submit_refund<P: Provider>(
    provider: &P,            // the evm provider
    htlc_address: Address,   // the htlc addresss
    swap_id: [u8; 32],       // the swap id
    nonce: u64,              // a nonce for this transaction (rbf)
    gas_price: u128,         // a gas price
    gas_payment: GasPayment, // variant of how we are going to pay gas for this transaction legacy or eip1559
) -> Result<()> {
    // Create a new stroem htlc v1 instance
    let stroem_htlc = StroemHTLCV1::new(htlc_address, provider);

    // Build the refund call
    let base = stroem_htlc.refund(FixedBytes::from(swap_id));

    // Apply the gas and the nonce to the transaction
    let call = apply_gas_and_nonce(base, nonce, gas_price, gas_payment);

    // Dispatch the transaction with a network timeout
    let pending = timed(NETWORK_TIMEOUT, call.send())
        .await
        .ok_or_else(|| DataError::Broadcast("refund send: timed out".into()))?
        .map_err(|e| DataError::Broadcast(format!("refund send: {e}")))?;

    // Wait for the receipt also on a timeout
    let receipt = timed(RECEIPT_TIMEOUT, pending.get_receipt())
        .await
        .ok_or_else(|| DataError::Broadcast("refund receipt: timed out".into()))?
        .map_err(|e| DataError::Broadcast(format!("refund receipt: {e}")))?;

    // Revert if receipt is non ok
    if !receipt.status() {
        return Err(DataError::Broadcast(format!(
            "refund reverted: tx {:?}",
            receipt.transaction_hash
        )));
    }
    tracing::info!(
        "EVM refund mined in block {:?}, tx {:?}",
        receipt.block_number,
        receipt.transaction_hash
    );
    Ok(())
}

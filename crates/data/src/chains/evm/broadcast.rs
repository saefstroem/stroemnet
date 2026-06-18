use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::Provider;
use stroemnet_protocol::v1::{CommitmentV1, RevealV1};

use super::GasPayment;
use super::contracts::StroemHTLCV1;
use crate::{DataError, Result};

/// Parses a hex string into an Ethereum address, returning a DataError if parsing fails.
fn parse_address(label: &str, value: &str) -> Result<Address> {
    value
        .parse()
        .map_err(|e| DataError::Broadcast(format!("{label} address {value}: {e}")))
}

/// Parses a decimal string into a U256, returning a DataError if parsing fails.
fn parse_value(value: &str) -> Result<U256> {
    U256::from_str_radix(value, 10)
        .map_err(|e| DataError::Broadcast(format!("amount {value}: {e}")))
}

async fn resolve_gas_price<P: Provider>(
    provider: &P,
    gas_payment: GasPayment,
) -> Result<Option<u128>> {
    match gas_payment {
        GasPayment::Eip1559 => Ok(None),
        GasPayment::Legacy => provider
            .get_gas_price()
            .await
            .map(|gp| Some(gp.saturating_mul(6) / 5))
            .map_err(|e| DataError::Broadcast(format!("gas price: {e}"))),
    }
}

/// Submits a new commitment to the EVM chain by calling the `newSwap` function on the HTLC contract.
/// Used by lp providers to commit to a swap after a user submits a commitment
pub(super) async fn submit_commitment<P: Provider>(
    provider: &P,
    htlc_address: Address,
    commitment: &CommitmentV1,
    gas_payment: GasPayment,
) -> Result<()> {
    // Parse the addresses
    let sender_addr = parse_address("sender", &commitment.addresses.sender)?;
    let receiver_addr = parse_address("receiver", &commitment.addresses.receiver)?;

    // Instantiate a new contract instance
    let stroem_htlc = StroemHTLCV1::new(htlc_address, provider);

    // Call the newSwap function with the appropriate parameters from the commitment
    let mut call = stroem_htlc
        .newSwap(
            sender_addr,   // The sender is on the behalf of
            receiver_addr, // Receiver of the swap on the EVM side
            commitment // Our destination address on the other chain
                .addresses
                .sender_destination
                .as_bytes()
                .to_vec()
                .into(),
            commitment.secret_hash.into(), // The secret hash of this swap
            U256::from(commitment.unlock_ts), // unlock timestamp for this swap i.e. when it can be refunded
            commitment.destination,           // the destination channel id
            commitment.swap_id.into(),        // unique swap id for this swap
        )
        .value(parse_value(&commitment.amount.value)?);
    if let Some(gp) = resolve_gas_price(provider, gas_payment).await? {
        call = call.gas_price(gp);
    }

    // Send the transaction and wait for it to be mined,
    // returning a DataError if any step fails
    let pending = call
        .send()
        .await
        .map_err(|e| DataError::Broadcast(format!("newSwap send: {e}")))?;

    // Wait for the transaction to be mined and get the receipt
    let receipt = pending
        .get_receipt()
        .await
        .map_err(|e| DataError::Broadcast(format!("newSwap receipt: {e}")))?;
    tracing::info!(
        "EVM commitment mined in block {:?}, tx {:?}",
        receipt.block_number,
        receipt.transaction_hash
    );
    Ok(())
}

/// Submits a claim transaction to the EVM chain by calling the `claim` function on the HTLC contract.
/// Used by lp providers to claim a swap after a user reveals the secret on the other chain
/// Or also as CCR participant to claim a swap after the counterparty reveals the secret via p2p
pub(super) async fn submit_claim<P: Provider>(
    provider: &P,
    htlc_address: Address,
    reveal: &RevealV1,
    gas_payment: GasPayment,
) -> Result<()> {
    // Instantiate a new contract instance
    let stroem_htlc = StroemHTLCV1::new(htlc_address, provider);
    let mut call = stroem_htlc.claim(
        FixedBytes::from(reveal.swap_id),
        FixedBytes::from(reveal.secret),
    );
    if let Some(gp) = resolve_gas_price(provider, gas_payment).await? {
        call = call.gas_price(gp);
    }
    // Send the transaction and wait for it to be mined, returning a DataError if any step fails
    let pending = call
        .send()
        .await
        .map_err(|e| DataError::Broadcast(format!("claim send: {e}")))?;
    // Wait for the transaction to be mined and get the receipt
    let receipt = pending
        .get_receipt()
        .await
        .map_err(|e| DataError::Broadcast(format!("claim receipt: {e}")))?;
    tracing::info!(
        "EVM claim mined in block {:?}, tx {:?}",
        receipt.block_number,
        receipt.transaction_hash
    );
    Ok(())
}

/// Submits a refund transaction to the EVM chain by calling the `refund` function on the HTLC contract.
/// Used by lp providers to refund a swap after the unlock timestamp has passed without a reveal
pub(super) async fn submit_refund<P: Provider>(
    provider: &P,
    htlc_address: Address,
    swap_id: [u8; 32],
    gas_payment: GasPayment,
) -> Result<()> {
    // Instantiate a new contract instance
    let stroem_htlc = StroemHTLCV1::new(htlc_address, provider);
    let mut call = stroem_htlc.refund(FixedBytes::from(swap_id));
    if let Some(gp) = resolve_gas_price(provider, gas_payment).await? {
        call = call.gas_price(gp);
    }

    // Send the transaction and wait for it to be mined, returning a DataError if any step fails
    let pending = call
        .send()
        .await
        .map_err(|e| DataError::Broadcast(format!("refund send: {e}")))?;
    // Wait for the transaction to be mined and get the receipt
    let receipt = pending
        .get_receipt()
        .await
        .map_err(|e| DataError::Broadcast(format!("refund receipt: {e}")))?;
    tracing::info!(
        "EVM refund mined in block {:?}, tx {:?}",
        receipt.block_number,
        receipt.transaction_hash
    );
    Ok(())
}

/// Gets the timestamp of the latest block on the EVM chain, returning None if the block cannot be retrieved
pub(super) async fn current_block_timestamp<P: Provider>(provider: &P) -> Option<u64> {
    match provider
        .get_block_by_number(alloy::eips::BlockNumberOrTag::Latest)
        .await
    {
        Ok(Some(block)) => Some(block.header.timestamp),
        _ => None,
    }
}

use alloy::primitives::U256;
use alloy::providers::Provider;
use stroemnet_protocol::v1::CommitmentV1;

use super::super::GasPayment;
use super::super::contracts::StroemHTLCV1;
use crate::chains::evm::parse_address;
use crate::chains::net::{NETWORK_TIMEOUT, RECEIPT_TIMEOUT, retry_timed, timed};
use crate::{DataError, Result};

/// A function to submit a commitment to an HTLC swap over the EVM network.
pub(crate) async fn submit_commitment<P: Provider>(
    provider: &P,
    htlc_address: alloy::primitives::Address,
    commitment: &CommitmentV1,
    gas_payment: GasPayment,
) -> Result<()> {
    // Parse the sender address
    let sender_addr = parse_address("sender", &commitment.addresses.sender)?;

    // Parse the receiver address
    let receiver_addr = parse_address("receiver", &commitment.addresses.receiver)?;

    // Create a new stroem htlc v1 instance
    let stroem_htlc = StroemHTLCV1::new(htlc_address, provider);

    // Build the call to create a new swap
    // We are doing it on behalf of the sender and receiver address
    let mut call = stroem_htlc
        .newSwap(
            sender_addr,   // the sender of this swap
            receiver_addr, // the receiver of the funds
            commitment // the senders destination address on the destination chain
                .addresses
                .sender_destination
                .as_bytes()
                .to_vec()
                .into(),
            commitment.secret_hash.into(), // secret hash that allows for the unlock
            U256::from(commitment.unlock_ts), // when this leg of the swap is refundable
            commitment.destination,        // the destination chain
            commitment.swap_id.into(),     // a unique swap id
        )
        .value(parse_value(&commitment.amount.value)?); // parse

    // Try to resolve a gas price depending on the payment type
    if let Some(gp) = resolve_gas_price(provider, gas_payment).await? {
        call = call.gas_price(gp);
    }

    // Transmit onchain with a timeout
    let pending = timed(NETWORK_TIMEOUT, call.send())
        .await
        .ok_or_else(|| DataError::Broadcast("newSwap send: timed out".into()))?
        .map_err(|e| DataError::Broadcast(format!("newSwap send: {e}")))?;

    // Try to receive a receipt but also on a timed basis,
    // returning an error if we dont receive it fast enough
    let receipt = timed(RECEIPT_TIMEOUT, pending.get_receipt())
        .await
        .ok_or_else(|| DataError::Broadcast("newSwap receipt: timed out".into()))?
        .map_err(|e| DataError::Broadcast(format!("newSwap receipt: {e}")))?;

    // Revert if its non-ok
    if !receipt.status() {
        return Err(DataError::Broadcast(format!(
            "newSwap reverted: tx {:?}",
            receipt.transaction_hash
        )));
    }
    tracing::info!(
        "EVM commitment mined in block {:?}, tx {:?}",
        receipt.block_number,
        receipt.transaction_hash
    );
    Ok(())
}

/// Parse a string to U256
fn parse_value(value: &str) -> Result<U256> {
    U256::from_str_radix(value, 10)
        .map_err(|e| DataError::Broadcast(format!("amount {value}: {e}")))
}

/// Resolve the gas price depending on a specific payment type variant
async fn resolve_gas_price<P: Provider>(
    provider: &P,
    gas_payment: GasPayment,
) -> Result<Option<u128>> {
    match gas_payment {
        GasPayment::Eip1559 => Ok(None), // for eip1559 we use the alloy built-in handling
        GasPayment::Legacy => {
            // for legacy networks we get the gas price and bump it slightly
            let gp = retry_timed("gas_price", || provider.get_gas_price())
                .await
                .ok_or_else(|| DataError::Broadcast("gas price: timed out".into()))?;
            Ok(Some(gp.saturating_mul(6) / 5))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_address_rejects_garbage() {
        assert!(parse_address("sender", "not-an-address").is_err());
    }

    #[test]
    fn parse_value_parses_decimal() {
        assert_eq!(parse_value("1000").ok(), Some(U256::from(1000u64)));
        assert!(parse_value("0xff").is_err());
    }
}

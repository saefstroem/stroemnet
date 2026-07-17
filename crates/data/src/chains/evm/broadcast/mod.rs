#[cfg(not(target_arch = "wasm32"))]
mod claim;
mod commit;
#[cfg(not(target_arch = "wasm32"))]
mod refund;

#[cfg(not(target_arch = "wasm32"))]
pub(super) use claim::submit_claim;
pub(super) use commit::submit_commitment;
#[cfg(not(target_arch = "wasm32"))]
pub(super) use refund::submit_refund;

#[cfg(not(target_arch = "wasm32"))]
use super::GasPayment;

#[cfg(not(target_arch = "wasm32"))]
/// Applies gas and nonce to an alloy call depending on the `GasPayment` variant.
pub(super) fn apply_gas_and_nonce<P, D, N>(
    base: alloy::contract::CallBuilder<P, D, N>,
    nonce: u64,
    gas_price: u128,
    gas_payment: GasPayment,
) -> alloy::contract::CallBuilder<P, D, N>
where
    P: alloy::providers::Provider<N>,
    D: alloy::contract::CallDecoder,
    N: alloy::network::Network,
{
    // Add the nonce to the base call
    let base = base.nonce(nonce);

    // For simplicity we set the gas price to be the same for eip1559
    // as well but for the future we should probably make this a bit more efficient
    match gas_payment {
        GasPayment::Eip1559 => base
            .max_fee_per_gas(gas_price)
            .max_priority_fee_per_gas(gas_price),
        GasPayment::Legacy => base.gas_price(gas_price),
    }
}

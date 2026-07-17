use std::sync::Arc;

use kaspa_consensus_core::constants::{STORAGE_MASS_PARAMETER, TRANSIENT_BYTE_TO_MASS_FACTOR};
use kaspa_consensus_core::mass::MassCalculator;
use kaspa_consensus_core::tx::Transaction;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::KaspaRpcClient;

use super::super::error::{KaspaError, Result};
use crate::chains::net::retry_timed;

/// Mass per tx byte
const MASS_PER_TX_BYTE: u64 = 1;
/// How much mass for every spk byte
const MASS_PER_SCRIPT_PUB_KEY_BYTE: u64 = 10;

/// How much mass per signature operation
const MASS_PER_SIG_OP: u64 = 1000;

/// The signature script size for schnorr sig
const SCHNORR_SIG_SCRIPT_SIZE: u64 = 66;

/// Maximum fee rate
const MAX_FEERATE: f64 = 100000.0;

/// Compute the fee from specified mass
fn fee_from_mass(mass: u64, feerate: f64) -> u64 {
    ((mass as f64 * feerate).ceil() as u64).max(1)
}

/// Calculate the priority fee based on the transaction
pub(super) async fn calculate_priority_fee(
    client: &Arc<KaspaRpcClient>,
    tx: &Transaction,
    extra_sig_script_bytes: u64,
) -> Result<u64> {
    // Retrieve the fee estimate from the rpc
    let fee_estimate = retry_timed("get_fee_estimate", || client.get_fee_estimate())
        .await
        .ok_or_else(|| KaspaError::Other("get_fee_estimate: timed out".into()))?;
    // We only work with priority buckets
    let feerate = fee_estimate.priority_bucket.feerate;
    if !feerate.is_finite() || feerate < 0.0 {
        return Err(KaspaError::Other(format!("invalid rpc feerate {feerate}")));
    }

    // Take the smallest of fee rate or maximum
    let feerate = feerate.min(MAX_FEERATE);

    // Instantiate a mass calculator with our configured constants
    let mass_calc = MassCalculator::new(
        MASS_PER_TX_BYTE,
        MASS_PER_SCRIPT_PUB_KEY_BYTE,
        MASS_PER_SIG_OP,
        STORAGE_MASS_PARAMETER,
    );

    // Compute the non contextual masses on the tx
    let non_contextual = mass_calc.calc_non_contextual_masses(tx);

    // Compute the signature bytes based on how many inputs
    let schnorr_sig_bytes: u64 = tx
        .inputs
        .iter()
        .filter(|input| input.sig_op_count > 0)
        .count() as u64
        * SCHNORR_SIG_SCRIPT_SIZE;

    // Compute the total signature bytes that we have
    let total_sig_bytes = schnorr_sig_bytes + extra_sig_script_bytes;

    // Compute the compute mass
    let compute_mass = non_contextual.compute_mass + total_sig_bytes * MASS_PER_TX_BYTE;

    // Compute transient bytes to mass factor
    let transient_mass =
        non_contextual.transient_mass + total_sig_bytes * TRANSIENT_BYTE_TO_MASS_FACTOR;

    // The mass is whatever is larger between compute and transient mass
    let mass = compute_mass.max(transient_mass);

    // Compute the fee from the mass and return
    Ok(fee_from_mass(mass, feerate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_is_at_least_one() {
        assert_eq!(fee_from_mass(0, 0.0), 1);
        assert_eq!(fee_from_mass(100, 0.0), 1);
    }

    #[test]
    fn fee_rounds_up() {
        assert_eq!(fee_from_mass(10, 1.5), 15);
        assert_eq!(fee_from_mass(3, 1.4), 5);
    }
}

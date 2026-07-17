use alloy::primitives::Address;
use alloy::providers::Provider;

use super::Evm;
use super::signing;
use crate::chains::net::retry_timed;
use crate::chains::settlement::ActionKey;

/// How many times we try to resubmit a transaction with a higher gas price
const MAX_GAS_BUMPS: u32 = 12;

/// Maximum gas price across the entire application
const MAX_GAS_PRICE_WEI: u128 = 5_000_000_000_000;

/// Compute the next gas bump based on current base and number of attempts
fn escalated_gas(base: u128, attempt_count: u32) -> u128 {
    let mut gas = base;

    // Bump the gas based on the number of attempts
    for _ in 0..attempt_count.min(MAX_GAS_BUMPS) {
        gas = gas.saturating_mul(115) / 100;
    }

    // We never want to exceed the maximum gas price in wei units
    gas.min(MAX_GAS_PRICE_WEI)
}

impl Evm {
    /// Computes the next nonce and the gas price for how much to increase the transaction by
    pub(super) async fn replacement(&self, key: ActionKey) -> Option<(u64, u128)> {
        // Retrieve the state of the attempt
        let attempt = self.queue.get(key)?;

        // Read the gas price
        let base = retry_timed("settle gas_price", || self.read_provider.get_gas_price()).await?;

        // Compute an increased amount based on the attempt to prioritize our inclusion
        let mut gas = escalated_gas(base, attempt.attempt_count);

        // Set the updated gas price value
        if let Some(prev) = attempt.last_gas {
            gas = gas
                .max(prev.saturating_mul(115) / 100)
                .min(MAX_GAS_PRICE_WEI);
        }

        // Compute the address from the private key
        let pk = self.private_key.as_deref()?;
        let addr: Address = signing::address_from_private_key(pk).ok()?.parse().ok()?;

        // Retrieve the nonce from onchain
        let confirmed = retry_timed("settle account nonce", || {
            self.read_provider.get_transaction_count(addr)
        })
        .await?;
        let nonce = match attempt.nonce {
            Some(n) if n >= confirmed => n, // if the attempts nonce is greater than the latest onchain nonce we can just use it
            _ => {
                let pending = retry_timed("settle pending nonce", || {
                    // otherwise we need to retrieve the latest pending nonce
                    self.read_provider.get_transaction_count(addr).pending()
                })
                .await?;
                // then we take the maximum nonce, either the pending or the confirmed
                let n = pending.max(confirmed);

                // then update the nonce that we now have used
                self.queue.set_nonce(key, n);
                n
            }
        };

        // Then also update the gas price that we have used for this attempt
        self.queue.set_last_gas(key, gas);
        // sync to disk
        self.persist_swap(key.swap_id);

        // Return the nonce to use and the gas price
        Some((nonce, gas))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gas_bumps_at_least_one_eighth_per_attempt_then_caps() {
        let base = 1_000u128;
        assert_eq!(escalated_gas(base, 0), 1_000);
        assert!(escalated_gas(base, 1) >= base * 1125 / 1000);
        assert!(escalated_gas(base, 2) > escalated_gas(base, 1));
        assert_eq!(escalated_gas(base, 100), escalated_gas(base, MAX_GAS_BUMPS));
    }
}

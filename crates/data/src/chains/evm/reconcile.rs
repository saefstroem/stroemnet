use alloy::eips::BlockId;
use alloy::primitives::FixedBytes;

use super::Evm;
use super::contracts::StroemHTLCV1;
use crate::chains::net::{NETWORK_TIMEOUT, timed};
use crate::chains::settlement::{ActionKey, Observation};

/// Map the EVM contract booleans into a concrete observation status for a swap
/// that describes its state.
fn observation_from(finalized: bool, initialized: bool) -> Observation {
    if finalized {
        Observation::Settled
    } else if initialized {
        Observation::NotSettled
    } else {
        Observation::Unknown
    }
}

impl Evm {
    /// Observe this swaps state onchain, i.e. whether it is settled or not
    pub(super) async fn observe_onchain(&self, key: ActionKey) -> Observation {
        let stroem = StroemHTLCV1::new(self.htlc_address, &self.read_provider);
        match timed(
            NETWORK_TIMEOUT,
            stroem
                .swaps(FixedBytes::from(key.swap_id))
                .block(BlockId::finalized())
                .call(),
        )
        .await
        {
            Some(Ok(swap)) => observation_from(swap.finalized, swap.initialized),
            _ => Observation::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_finalized_is_settled_uninitialized_is_unknown() {
        assert!(matches!(observation_from(true, true), Observation::Settled));
        assert!(matches!(
            observation_from(false, false),
            Observation::Unknown
        ));
        assert!(matches!(
            observation_from(false, true),
            Observation::NotSettled
        ));
    }
}

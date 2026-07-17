use std::future::Future;
use std::pin::Pin;

use super::action::ActionKey;
use crate::MaybeSend;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type SettleFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
pub(crate) type SettleFut<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A settleoutcome is either a retriable error or a fatal one
pub(crate) enum SettleOutcome {
    Retry(&'static str),
    Fatal(String),
}

/// Either settled not settled or unknown. Not all chains can guarantee settled from onchain observation.
/// For example for kaspa we actually need to observe the spending of the htlc utxo as we go over blocks
pub(crate) enum Observation {
    Settled,
    NotSettled,
    Unknown,
}

pub(crate) trait Settler: MaybeSend {
    fn due_now(&self, now: u64) -> Vec<ActionKey>;
    fn settle(&self, key: ActionKey) -> SettleFut<'_, SettleOutcome>;
    fn observe(&self, key: ActionKey) -> SettleFut<'_, Observation>;
    fn record_success(&self, key: ActionKey);
    fn record_failure(&self, key: ActionKey, now: u64);
    fn is_stuck(&self, key: ActionKey, now: u64) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_and_observation_variants_construct() {
        let outcomes = [SettleOutcome::Retry("x"), SettleOutcome::Fatal("y".into())];
        let observations = [
            Observation::Settled,
            Observation::NotSettled,
            Observation::Unknown,
        ];
        assert_eq!(outcomes.len(), 2);
        assert_eq!(observations.len(), 3);
    }
}

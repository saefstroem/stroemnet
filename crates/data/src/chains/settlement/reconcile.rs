use stroemnet_protocol::now_unix_secs;

use super::action::ActionKey;
use super::metrics::{Metric, SettlementMetrics};
use super::settler::{Observation, Settler};

/// Accepts a trait backed settled which will check if any of the swaps
/// are ready to be resolved and then check it against the onchain observation
pub(crate) async fn reconcile_on_boot<S: Settler>(settler: &S, metrics: &dyn SettlementMetrics) {
    let keys = settler.due_now(now_unix_secs());
    reconcile(settler, &keys, metrics).await;
}

pub(crate) async fn reconcile<S: Settler>(
    settler: &S,
    keys: &[ActionKey],
    metrics: &dyn SettlementMetrics,
) {
    for &key in keys {
        // If the key is observed to be settled we can instantly remove it now.
        if let Observation::Settled = settler.observe(key).await {
            settler.record_success(key);
            metrics.incr(Metric::Reconciled);
            tracing::info!(
                target: "settlement",
                swap = %hex::encode(key.swap_id),
                action = ?key.action,
                "reconciled already-settled on boot"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::metrics::NoopMetrics;
    use super::super::settler::{SettleFut, SettleOutcome};
    use super::*;
    use parking_lot::Mutex;

    struct MockSettler {
        observation: &'static str,
        cleared: Mutex<Vec<ActionKey>>,
    }

    impl Settler for MockSettler {
        fn due_now(&self, _now: u64) -> Vec<ActionKey> {
            Vec::new()
        }
        fn settle(&self, _key: ActionKey) -> SettleFut<'_, SettleOutcome> {
            Box::pin(async { SettleOutcome::Retry("mock") })
        }
        fn observe(&self, _key: ActionKey) -> SettleFut<'_, Observation> {
            let obs = match self.observation {
                "settled" => Observation::Settled,
                "not" => Observation::NotSettled,
                _ => Observation::Unknown,
            };
            Box::pin(async move { obs })
        }
        fn record_success(&self, key: ActionKey) {
            self.cleared.lock().push(key);
        }
        fn record_failure(&self, _key: ActionKey, _now: u64) {}
        fn is_stuck(&self, _key: ActionKey, _now: u64) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn settled_is_recorded_others_kept() {
        let keys = [ActionKey::refund([2u8; 32])];
        let settled = MockSettler {
            observation: "settled",
            cleared: Mutex::new(Vec::new()),
        };
        reconcile(&settled, &keys, &NoopMetrics).await;
        assert_eq!(settled.cleared.lock().len(), 1);

        let not = MockSettler {
            observation: "not",
            cleared: Mutex::new(Vec::new()),
        };
        reconcile(&not, &keys, &NoopMetrics).await;
        assert!(not.cleared.lock().is_empty());
    }
}

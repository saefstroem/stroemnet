use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use stroemnet_protocol::{now_unix_secs, sleep_secs};

use super::super::net::{RECEIPT_TIMEOUT, timed};
use super::metrics::{Gauge, Metric, SettlementMetrics};
use super::settler::{Observation, SettleOutcome, Settler};
use crate::TaskFut;

/// How often we should try to drive the settler
const SETTLE_TICK_SECS: u64 = 5;

pub(crate) fn settler_loop<S: Settler + 'static>(
    settler: Arc<S>,
    metrics: Arc<dyn SettlementMetrics>,
) -> TaskFut {
    Box::pin(async move {
        loop {
            // We want to ensure that the settler driver can never panic
            // this is not a means for error handling rather it is a way for us
            // to ensure that underlying dependencies if incorrectly done wouldnt cause a
            // panic to the whole settlement system
            let tick = AssertUnwindSafe(drive(
                &*settler,
                now_unix_secs(),
                RECEIPT_TIMEOUT,
                metrics.as_ref(),
            ))
            .catch_unwind()
            .await;
            if tick.is_err() {
                tracing::error!(
                    target: "settlement",
                    kind = "panic",
                    "settlement drive panicked; continuing loop"
                );
            }
            // Sleep for x amount of seconds then retry
            sleep_secs(SETTLE_TICK_SECS).await;
        }
    })
}

pub(crate) async fn drive<S: Settler>(
    settler: &S,
    now: u64,
    timeout: Duration,
    metrics: &dyn SettlementMetrics,
) {
    // Go over all action keys that are due now
    for key in settler.due_now(now) {
        if settler.is_stuck(key, now) {
            // check if the current key is stuck i.e. its past deadline
            metrics.incr(Metric::DeadlineExceeded);
            // If its settled then we can exit
            if let Observation::Settled = settler.observe(key).await {
                settler.record_success(key);
                metrics.incr(Metric::Reconciled);
                continue;
            }
            tracing::error!(
                target: "settlement",
                kind = "needs_intervention",
                swap = %hex::encode(key.swap_id),
                action = ?key.action,
                "settlement past deadline but funds still recoverable on-chain; retrying"
            );
        }
        // Attempt to settle it.
        let outcome = match timed(timeout, settler.settle(key)).await {
            Some(o) => o,
            None => SettleOutcome::Retry("timeout"),
        };

        // The outcome can eeither be a retry or a fatal failure
        match outcome {
            SettleOutcome::Retry(reason) => {
                tracing::info!(
                    target: "settlement",
                    reason,
                    swap = %hex::encode(key.swap_id),
                    "settlement retry"
                );
                settler.record_failure(key, now);
                metrics.incr(Metric::Retried);
            }
            SettleOutcome::Fatal(e) => {
                tracing::error!(
                    target: "settlement",
                    error = %e,
                    swap = %hex::encode(key.swap_id),
                    "settlement fatal"
                );
                settler.record_failure(key, now);
                metrics.incr(Metric::Fatal);
            }
        }
    }
    metrics.gauge(Gauge::QueueDepth, settler.due_now(now).len() as u64);
}

#[cfg(test)]
mod tests {
    use super::super::action::ActionKey;
    use super::super::metrics::NoopMetrics;
    use super::super::settler::{Observation, SettleFut, Settler};
    use super::*;
    use parking_lot::Mutex;

    #[derive(Default)]
    struct MockSettler {
        outcome: Option<&'static str>,
        observed: Option<&'static str>,
        stuck: bool,
        succeeded: Mutex<Vec<ActionKey>>,
        failed: Mutex<Vec<ActionKey>>,
    }

    impl Settler for MockSettler {
        fn due_now(&self, _now: u64) -> Vec<ActionKey> {
            vec![ActionKey::claim([1u8; 32])]
        }
        fn settle(&self, _key: ActionKey) -> SettleFut<'_, SettleOutcome> {
            let outcome = match self.outcome {
                Some("fatal") => SettleOutcome::Fatal("mock".into()),
                _ => SettleOutcome::Retry("mock"),
            };
            Box::pin(async move { outcome })
        }
        fn observe(&self, _key: ActionKey) -> SettleFut<'_, Observation> {
            let obs = match self.observed {
                Some("settled") => Observation::Settled,
                _ => Observation::Unknown,
            };
            Box::pin(async move { obs })
        }
        fn record_success(&self, key: ActionKey) {
            self.succeeded.lock().push(key);
        }
        fn record_failure(&self, key: ActionKey, _now: u64) {
            self.failed.lock().push(key);
        }
        fn is_stuck(&self, _key: ActionKey, _now: u64) -> bool {
            self.stuck
        }
    }

    #[tokio::test]
    async fn retry_and_fatal_record_failure() {
        let m = MockSettler {
            outcome: Some("retry"),
            ..Default::default()
        };
        drive(&m, 0, Duration::from_secs(1), &NoopMetrics).await;
        assert_eq!(m.failed.lock().len(), 1);
        let f = MockSettler {
            outcome: Some("fatal"),
            ..Default::default()
        };
        drive(&f, 0, Duration::from_secs(1), &NoopMetrics).await;
        assert_eq!(f.failed.lock().len(), 1);
    }

    #[tokio::test]
    async fn stuck_but_settled_onchain_is_cleaned_up() {
        let m = MockSettler {
            stuck: true,
            observed: Some("settled"),
            ..Default::default()
        };
        drive(&m, 0, Duration::from_secs(1), &NoopMetrics).await;
        assert_eq!(m.succeeded.lock().len(), 1);
        assert!(m.failed.lock().is_empty());
    }

    #[tokio::test]
    async fn stuck_but_recoverable_retries_not_abandons() {
        let m = MockSettler {
            stuck: true,
            outcome: Some("retry"),
            ..Default::default()
        };
        drive(&m, 0, Duration::from_secs(1), &NoopMetrics).await;
        assert!(m.succeeded.lock().is_empty());
        assert_eq!(m.failed.lock().len(), 1);
    }
}

use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
/// Different type of metrics that can happen to swaps during the settlement loop
pub enum Metric {
    Retried,
    Fatal,
    DeadlineExceeded,
    Reconciled,
}

#[derive(Clone, Copy, Debug)]
/// Different gauges in order to inspect internal system state
pub enum Gauge {
    QueueDepth, // Inspects the queue depth
}

pub trait SettlementMetrics: Send + Sync {
    fn incr(&self, metric: Metric);
    fn gauge(&self, gauge: Gauge, value: u64);
}

pub struct NoopMetrics;

impl SettlementMetrics for NoopMetrics {
    fn incr(&self, _metric: Metric) {}
    fn gauge(&self, _gauge: Gauge, _value: u64) {}
}

pub(crate) fn or_noop(metrics: Option<Arc<dyn SettlementMetrics>>) -> Arc<dyn SettlementMetrics> {
    metrics.unwrap_or_else(|| Arc::new(NoopMetrics))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_metrics_are_inert() {
        let m = or_noop(None);
        m.incr(Metric::Retried);
        m.incr(Metric::Fatal);
        m.incr(Metric::DeadlineExceeded);
        m.incr(Metric::Reconciled);
        m.gauge(Gauge::QueueDepth, 5);
    }
}

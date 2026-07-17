use std::fmt::Display;
use std::future::IntoFuture;
use std::time::Duration;

pub(crate) const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const RECEIPT_TIMEOUT: Duration = Duration::from_secs(120);
#[cfg(not(target_arch = "wasm32"))]
const RETRY_ATTEMPTS: u32 = 3;
#[cfg(not(target_arch = "wasm32"))]
const RETRY_BASE_MS: u64 = 400;

#[cfg(not(target_arch = "wasm32"))]
/// Executes a future and races it against a timeout
pub(crate) async fn timed<F: IntoFuture>(dur: Duration, fut: F) -> Option<F::Output> {
    tokio::time::timeout(dur, fut.into_future()).await.ok()
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn timed<F: IntoFuture>(_dur: Duration, fut: F) -> Option<F::Output> {
    Some(fut.into_future().await)
}

#[cfg(not(target_arch = "wasm32"))]
/// Executes a future with a number of retry attempts with also a network timeout
pub(crate) async fn retry_timed<T, E, Fut, Op>(label: &str, mut op: Op) -> Option<T>
where
    Op: FnMut() -> Fut,
    Fut: IntoFuture<Output = core::result::Result<T, E>>,
    E: Display,
{
    // Go over each attempt
    for attempt in 0..RETRY_ATTEMPTS {
        // race against timeout
        match timed(NETWORK_TIMEOUT, op()).await {
            Some(Ok(value)) => return Some(value),
            Some(Err(e)) => {
                tracing::warn!(target: "net", "{label} attempt {} error: {e}", attempt + 1)
            }
            None => tracing::warn!(target: "net", "{label} attempt {} timed out", attempt + 1),
        }
        if attempt + 1 < RETRY_ATTEMPTS {
            tokio::time::sleep(Duration::from_millis(RETRY_BASE_MS << attempt)).await;
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn retry_timed<T, E, Fut, Op>(_label: &str, mut op: Op) -> Option<T>
where
    Op: FnMut() -> Fut,
    Fut: IntoFuture<Output = core::result::Result<T, E>>,
    E: Display,
{
    op().into_future().await.ok()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn timed_returns_none_on_hang() {
        let hang = std::future::pending::<u8>();
        assert!(timed(Duration::from_millis(10), hang).await.is_none());
        assert_eq!(timed(NETWORK_TIMEOUT, async { 7u8 }).await, Some(7));
    }

    #[tokio::test]
    async fn retry_timed_recovers_after_transient_error() {
        let calls = AtomicU32::new(0);
        let got = retry_timed("op", || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move { if n == 0 { Err("transient") } else { Ok(99u8) } }
        })
        .await;
        assert_eq!(got, Some(99));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

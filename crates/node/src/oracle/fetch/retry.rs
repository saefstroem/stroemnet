use super::PriceFeed;
use crate::oracle::OracleError;
use crate::oracle::result::Result;

const MAX_RETRIES: usize = 3;
#[cfg(not(target_arch = "wasm32"))]
const ATTEMPT_TIMEOUT_SECS: u64 = 30;
const RETRY_BACKOFF_SECS: u64 = 1;

impl PriceFeed {
    pub(super) async fn with_retry<F, Fut, T>(&self, source: &'static str, f: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut last_error = None;
        for attempt in 1..=MAX_RETRIES {
            #[cfg(not(target_arch = "wasm32"))]
            let outcome: Result<T> = match tokio::time::timeout(
                std::time::Duration::from_secs(ATTEMPT_TIMEOUT_SECS),
                f(),
            )
            .await
            {
                Ok(r) => r,
                Err(_) => Err(OracleError::Timeout(ATTEMPT_TIMEOUT_SECS)),
            };
            #[cfg(target_arch = "wasm32")]
            let outcome: Result<T> = f().await;

            match outcome {
                Ok(v) => return Ok(v),
                Err(e) => {
                    tracing::warn!("{source} fetch attempt {attempt}/{MAX_RETRIES} failed: {e}");
                    last_error = Some(e);
                }
            }
            if attempt < MAX_RETRIES {
                stroemnet_protocol::sleep_secs(RETRY_BACKOFF_SECS).await;
            }
        }
        Err(OracleError::RetryExhausted(
            MAX_RETRIES,
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Unknown error".to_string()),
        ))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::cell::Cell;

    #[tokio::test]
    async fn returns_first_success_without_retrying() {
        let feed = PriceFeed::new(reqwest::Client::new());
        let calls = Cell::new(0u32);
        let result: Result<u8> = feed
            .with_retry("x", || {
                calls.set(calls.get() + 1);
                async { Ok(7u8) }
            })
            .await;
        assert_eq!(result.unwrap(), 7);
        assert_eq!(calls.get(), 1);
    }
}

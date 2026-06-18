use crate::oracle::OracleError;
use crate::oracle::result::Result;
use ahash::AHashMap;
use stroemnet_protocol::ChannelId;

use super::PriceFeed;
use super::PriceSample;

// Max retries for each source before giving up, and the backoff strategy (simple fixed backoff here)
const MAX_RETRIES: usize = 3;
#[cfg(not(target_arch = "wasm32"))]
const ATTEMPT_TIMEOUT_SECS: u64 = 30;
const RETRY_BACKOFF_SECS: u64 = 1;

impl PriceFeed {
    /// Fetches price data from multiple sources and aggregates it into a single price per channel.
    /// The aggregation is done by taking a weighted average of the prices from different sources, where
    /// where the weights are based on the reported trading volume in USD for that channel on each source.
    pub async fn aggregate(&self, channels: &[ChannelId]) -> Result<Vec<(ChannelId, f64)>> {
        // Retrieve price samples from all available sources in parallel, with retries.
        // Browsers (wasm) can only reach Bybit — Gate.io and MEXC send no CORS headers —
        // so they are omitted there to avoid guaranteed failures. Add more CORS-enabled
        // sources to the wasm list as they become available.
        #[cfg(not(target_arch = "wasm32"))]
        let source_results = {
            let (mexc_res, bybit_res, gateio_res) = tokio::join!(
                self.with_retry("MEXC", || self.mexc(channels)),
                self.with_retry("Bybit", || self.bybit(channels)),
                self.with_retry("Gate.io", || self.gateio(channels)),
            );
            [
                ("MEXC", mexc_res),
                ("Bybit", bybit_res),
                ("Gate.io", gateio_res),
            ]
        };
        #[cfg(target_arch = "wasm32")]
        let source_results = [(
            "Bybit",
            self.with_retry("Bybit", || self.bybit(channels)).await,
        )];

        let mut by_channel: AHashMap<ChannelId, Vec<PriceSample>> = AHashMap::new();

        // Map the results by channel,
        // and log any errors without failing the entire aggregation,
        // since we want to be resilient to partial failures of sources.
        for (source_name, source_res) in source_results {
            match source_res {
                Ok(samples) => {
                    for (ch, sample) in samples {
                        by_channel.entry(ch).or_default().push(sample);
                    }
                }
                Err(e) => tracing::warn!("{} source exhausted: {}", source_name, e),
            }
        }

        let mut out = Vec::new();
        // For each requested channel, compute the weighted average price
        //across all sources that provided data for that channel.
        for ch in channels {
            let Some(samples) = by_channel.get(ch) else {
                continue;
            };
            if samples.is_empty() {
                continue;
            }
            // Compute the total volume across all sources for this channel, and then the weighted average price.
            let total_volume: f64 = samples.iter().map(|s| s.volume_usd).sum();
            let weighted_price = if total_volume > 0.0 {
                (samples.iter().map(|s| s.price * s.volume_usd).sum::<f64>()) / total_volume
            } else {
                // If there is no volume we simply fall back to average price
                samples.iter().map(|s| s.price).sum::<f64>() / samples.len() as f64
            };
            tracing::debug!(
                "Aggregated {} price across {} source(s): ${:.6}",
                ch.to_string(),
                samples.len(),
                weighted_price
            );
            // Push the aggregated price for this channel to the output list
            out.push((*ch, weighted_price));
        }

        if out.is_empty() {
            return Err(OracleError::AllSourcesFailed(
                "All exchanges failed for every channel".to_string(),
            ));
        }

        Ok(out)
    }

    /// Helper function to perform retries with timeouts for a given async operation, used for fetching from each source.
    async fn with_retry<F, Fut, T>(&self, source: &'static str, f: F) -> Result<T>
    where
        F: Fn() -> Fut,                               // Accepts a fn that returns future
        Fut: std::future::Future<Output = Result<T>>, // standard future that returns result
    {
        let mut last_error = None;
        for attempt in 1..=MAX_RETRIES {
            // Native: race the fetch against a timeout. Wasm: the browser fetch has its
            // own timeout/abort semantics and tokio timers are unavailable.
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
                    tracing::warn!(
                        "{} fetch attempt {}/{} failed: {}",
                        source,
                        attempt,
                        MAX_RETRIES,
                        e
                    );
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
    use super::*;

    #[tokio::test]
    #[ignore = "requires network"]
    async fn aggregate_kas_across_three_sources() {
        let feed = PriceFeed::new(reqwest::Client::new());
        let prices = feed
            .aggregate(&[ChannelId::KaspaTn10])
            .await
            .expect("aggregate");
        assert_eq!(prices.len(), 1);
        assert!(prices[0].1 > 0.0);
        println!("Weighted KAS price: ${:.6}", prices[0].1);
    }

    #[tokio::test]
    #[ignore = "requires network"]
    async fn aggregate_eth_across_three_sources() {
        let feed = PriceFeed::new(reqwest::Client::new());
        let prices = feed
            .aggregate(&[ChannelId::EthereumSepolia])
            .await
            .expect("aggregate");
        assert_eq!(prices.len(), 1);
        assert!(prices[0].1 > 0.0);
        println!("Weighted ETH price: ${:.2}", prices[0].1);
    }

    #[tokio::test]
    #[ignore = "requires network"]
    async fn aggregate_both_channels() {
        let feed = PriceFeed::new(reqwest::Client::new());
        let prices = feed
            .aggregate(&[ChannelId::KaspaTn10, ChannelId::EthereumSepolia])
            .await
            .expect("aggregate");
        assert_eq!(prices.len(), 2);
        for (ch, p) in &prices {
            println!("{}: ${:.6}", ch.to_string(), p);
            assert!(*p > 0.0);
        }
    }
}

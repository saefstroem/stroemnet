use ahash::AHashMap;
use stroemnet_protocol::ChannelId;

use super::PriceFeed;
use super::PriceSample;
use super::robust::robust_price;
use crate::oracle::OracleError;
use crate::oracle::result::Result;

impl PriceFeed {
    /// Computes an aggregation of multiple price sources and their prices
    /// for each channels native token
    pub async fn aggregate(&self, channels: &[ChannelId]) -> Result<Vec<(ChannelId, f64)>> {
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
        for (source_name, source_res) in source_results {
            match source_res {
                Ok(samples) => {
                    for (ch, sample) in samples {
                        by_channel.entry(ch).or_default().push(sample);
                    }
                }
                Err(e) => tracing::warn!("{source_name} source exhausted: {e}"),
            }
        }

        let mut out = Vec::new();
        for ch in channels {
            if let Some(samples) = by_channel.get(ch)
                && let Some(price) = robust_price(samples)
            {
                out.push((*ch, price));
            }
        }

        if out.is_empty() {
            return Err(OracleError::AllSourcesFailed(
                "All exchanges failed or produced insufficient agreeing sources".to_string(),
            ));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    #[ignore = "requires network"]
    async fn aggregate_both_channels() {
        let feed = PriceFeed::new(reqwest::Client::new());
        let prices = feed
            .aggregate(&[ChannelId::KaspaTn10, ChannelId::EthereumSepolia])
            .await
            .unwrap();
        assert_eq!(prices.len(), 2);
        assert!(prices.iter().all(|(_, p)| *p > 0.0));
    }
}

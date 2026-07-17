use crate::oracle::OracleError;
use crate::oracle::result::Result;
use ahash::AHashMap;
use serde::Deserialize;
use stroemnet_protocol::ChannelId;

use super::PriceFeed;
use super::PriceSample;

const MEXC_BASE_URL: &str = "https://api.mexc.com";

#[derive(Debug, Deserialize)]
struct MexcTicker {
    #[serde(rename = "lastPrice")]
    last_price: String,
    #[serde(rename = "quoteVolume")]
    quote_volume: String,
}

impl PriceFeed {
    pub(super) async fn mexc(
        &self,
        channels: &[ChannelId],
    ) -> Result<Vec<(ChannelId, PriceSample)>> {
        let mut symbol_to_channels: AHashMap<&'static str, Vec<ChannelId>> = AHashMap::new();
        for ch in channels {
            if let Some(sym) = Self::mexc_symbol(ch) {
                symbol_to_channels.entry(sym).or_default().push(*ch);
            }
        }

        if symbol_to_channels.is_empty() {
            return Err(OracleError::AllSourcesFailed(
                "No supported MEXC symbols for the given channels".to_string(),
            ));
        }

        let mut out = Vec::new();
        for (symbol, chs) in symbol_to_channels {
            match self.mexc_single(symbol).await {
                Ok(sample) => {
                    for ch in chs {
                        out.push((ch, sample));
                    }
                }
                Err(e) => tracing::warn!("MEXC fetch for {} failed: {}", symbol, e),
            }
        }

        if out.is_empty() {
            return Err(OracleError::AllSourcesFailed(
                "All MEXC tickers returned invalid data".to_string(),
            ));
        }

        Ok(out)
    }

    async fn mexc_single(&self, symbol: &str) -> Result<PriceSample> {
        let resp = self
            .client
            .get(format!("{}/api/v3/ticker/24hr", MEXC_BASE_URL))
            .query(&[("symbol", symbol)])
            .header("Accept", "application/json")
            .header("User-Agent", "stroemnet/0.1")
            .send()
            .await?;
        let ticker: MexcTicker = resp.json().await?;
        let price = ticker.last_price.parse::<f64>()?;
        if price <= 0.0 {
            return Err(OracleError::NoPriceData(format!(
                "MEXC returned non-positive price for {}: {}",
                symbol, price
            )));
        }
        let volume_usd = ticker.quote_volume.parse::<f64>().unwrap_or(0.0);
        Ok(PriceSample { price, volume_usd })
    }

    fn mexc_symbol(channel: &ChannelId) -> Option<&'static str> {
        match channel {
            ChannelId::KaspaTn10 => Some("KASUSDT"),
            ChannelId::EthereumSepolia => Some("ETHUSDT"),
            ChannelId::IgraGalleon => Some("KASUSDT"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn mexc_symbol_mapping() {
        assert_eq!(
            PriceFeed::mexc_symbol(&ChannelId::KaspaTn10),
            Some("KASUSDT")
        );
        assert_eq!(
            PriceFeed::mexc_symbol(&ChannelId::EthereumSepolia),
            Some("ETHUSDT")
        );
        assert_eq!(
            PriceFeed::mexc_symbol(&ChannelId::IgraGalleon),
            Some("KASUSDT")
        );
    }

    #[tokio::test]
    #[ignore = "requires network"]
    async fn mexc_fetches_kas() {
        let feed = PriceFeed::new(reqwest::Client::new());
        let samples = feed.mexc(&[ChannelId::KaspaTn10]).await.expect("fetch");
        assert_eq!(samples.len(), 1);
        assert!(samples[0].1.price > 0.0);
    }
}

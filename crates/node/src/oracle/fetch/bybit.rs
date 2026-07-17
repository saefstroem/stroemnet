use crate::oracle::OracleError;
use crate::oracle::result::Result;
use ahash::AHashMap;
use serde::Deserialize;
use stroemnet_protocol::ChannelId;

use super::PriceFeed;
use super::PriceSample;

const BYBIT_BASE_URL: &str = "https://api.bybit.com";

#[derive(Debug, Deserialize)]
struct BybitResponse {
    #[serde(rename = "retCode")]
    ret_code: i32,
    #[serde(rename = "retMsg", default)]
    ret_msg: String,
    result: BybitResult,
}

#[derive(Debug, Deserialize)]
struct BybitResult {
    list: Vec<BybitTicker>,
}

#[derive(Debug, Deserialize)]
struct BybitTicker {
    #[serde(rename = "lastPrice")]
    last_price: String,
    #[serde(rename = "turnover24h")]
    turnover_24h: String,
}

impl PriceFeed {
    pub(super) async fn bybit(
        &self,
        channels: &[ChannelId],
    ) -> Result<Vec<(ChannelId, PriceSample)>> {
        let mut symbol_to_channels: AHashMap<&'static str, Vec<ChannelId>> = AHashMap::new();

        for ch in channels {
            if let Some(sym) = Self::bybit_symbol(ch) {
                symbol_to_channels.entry(sym).or_default().push(*ch);
            }
        }

        if symbol_to_channels.is_empty() {
            return Err(OracleError::AllSourcesFailed(
                "No supported Bybit symbols for the given channels".to_string(),
            ));
        }

        let mut out = Vec::new();
        for (symbol, chs) in symbol_to_channels {
            match self.bybit_single(symbol).await {
                Ok(sample) => {
                    for ch in chs {
                        out.push((ch, sample));
                    }
                }
                Err(e) => tracing::warn!("Bybit fetch for {} failed: {}", symbol, e),
            }
        }

        if out.is_empty() {
            return Err(OracleError::AllSourcesFailed(
                "All Bybit tickers returned invalid data".to_string(),
            ));
        }

        Ok(out)
    }

    async fn bybit_single(&self, symbol: &str) -> Result<PriceSample> {
        let resp = self
            .client
            .get(format!("{}/v5/market/tickers", BYBIT_BASE_URL))
            .query(&[("category", "spot"), ("symbol", symbol)])
            .header("Accept", "application/json")
            .header("User-Agent", "stroemnet/0.1")
            .send()
            .await?;
        let body: BybitResponse = resp.json().await?;
        if body.ret_code != 0 {
            return Err(OracleError::NoPriceData(format!(
                "Bybit retCode {} for {}: {}",
                body.ret_code, symbol, body.ret_msg
            )));
        }
        let ticker = body.result.list.first().ok_or_else(|| {
            OracleError::NoPriceData(format!("Bybit returned empty list for {}", symbol))
        })?;
        let price = ticker.last_price.parse::<f64>()?;
        if price <= 0.0 {
            return Err(OracleError::NoPriceData(format!(
                "Bybit returned non-positive price for {}: {}",
                symbol, price
            )));
        }
        let volume_usd = ticker.turnover_24h.parse::<f64>().unwrap_or(0.0);
        Ok(PriceSample { price, volume_usd })
    }

    fn bybit_symbol(channel: &ChannelId) -> Option<&'static str> {
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
    fn bybit_symbol_mapping() {
        assert_eq!(
            PriceFeed::bybit_symbol(&ChannelId::KaspaTn10),
            Some("KASUSDT")
        );
        assert_eq!(
            PriceFeed::bybit_symbol(&ChannelId::EthereumSepolia),
            Some("ETHUSDT")
        );
        assert_eq!(
            PriceFeed::bybit_symbol(&ChannelId::IgraGalleon),
            Some("KASUSDT")
        );
    }

    #[tokio::test]
    #[ignore = "requires network"]
    async fn bybit_fetches_kas() {
        let feed = PriceFeed::new(reqwest::Client::new());
        let samples = feed.bybit(&[ChannelId::KaspaTn10]).await.expect("fetch");
        assert_eq!(samples.len(), 1);
        assert!(samples[0].1.price > 0.0);
    }
}

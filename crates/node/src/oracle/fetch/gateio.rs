use crate::oracle::OracleError;
use crate::oracle::result::Result;
use ahash::AHashMap;
use serde::Deserialize;
use stroemnet_protocol::ChannelId;

use super::PriceFeed;
use super::PriceSample;

const GATEIO_BASE_URL: &str = "https://api.gateio.ws";

#[derive(Debug, Deserialize)]
struct GateioTicker {
    last: String,
    quote_volume: String,
}

impl PriceFeed {
    /// Fetches price data for the given channels from Gate.io's API,
    /// mapping our internal channel IDs to Gate.io symbols.
    pub(super) async fn gateio(
        &self,
        channels: &[ChannelId],
    ) -> Result<Vec<(ChannelId, PriceSample)>> {
        let mut symbol_to_channels: AHashMap<&'static str, Vec<ChannelId>> = AHashMap::new();
        // Goes over all channels  and maps them to gate io symbols
        for ch in channels {
            if let Some(sym) = Self::gateio_symbol(ch) {
                symbol_to_channels.entry(sym).or_default().push(*ch);
            }
        }

        if symbol_to_channels.is_empty() {
            return Err(OracleError::AllSourcesFailed(
                "No supported Gate.io symbols for the given channels".to_string(),
            ));
        }

        let mut out = Vec::new();
        // Go over all the symbols that we need to fetch and fetch them
        for (symbol, chs) in symbol_to_channels {
            // Fetch the price for this symbol
            match self.gateio_single(symbol).await {
                Ok(sample) => {
                    // For all channels that map to this symbol, add the sample to the output
                    for ch in chs {
                        out.push((ch, sample));
                    }
                }
                Err(e) => tracing::warn!("Gate.io fetch for {} failed: {}", symbol, e),
            }
        }

        if out.is_empty() {
            return Err(OracleError::AllSourcesFailed(
                "All Gate.io tickers returned invalid data".to_string(),
            ));
        }

        Ok(out)
    }

    /// Fetches price data for a single symbol from Gate.io's API, and extracts the price and volume.
    async fn gateio_single(&self, symbol: &str) -> Result<PriceSample> {
        let resp = self
            .client
            .get(format!("{}/api/v4/spot/tickers", GATEIO_BASE_URL))
            .query(&[("currency_pair", symbol)])
            .header("Accept", "application/json")
            .header("User-Agent", "stroemnet/0.1")
            .send()
            .await?;
        let tickers: Vec<GateioTicker> = resp.json().await?;
        let ticker = tickers.first().ok_or_else(|| {
            OracleError::NoPriceData(format!("Gate.io returned empty list for {}", symbol))
        })?;
        let price = ticker.last.parse::<f64>()?;
        if price <= 0.0 {
            return Err(OracleError::NoPriceData(format!(
                "Gate.io returned non-positive price for {}: {}",
                symbol, price
            )));
        }
        let volume_usd = ticker.quote_volume.parse::<f64>().unwrap_or(0.0);
        Ok(PriceSample { price, volume_usd })
    }

    fn gateio_symbol(channel: &ChannelId) -> Option<&'static str> {
        match channel {
            ChannelId::KaspaTn10 => Some("KAS_USDT"),
            ChannelId::EthereumSepolia => Some("ETH_USDT"),
            ChannelId::IgraGalleon => Some("KAS_USDT"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateio_symbol_mapping() {
        assert_eq!(
            PriceFeed::gateio_symbol(&ChannelId::KaspaTn10),
            Some("KAS_USDT")
        );
        assert_eq!(
            PriceFeed::gateio_symbol(&ChannelId::EthereumSepolia),
            Some("ETH_USDT")
        );
        assert_eq!(
            PriceFeed::gateio_symbol(&ChannelId::IgraGalleon),
            Some("KAS_USDT")
        );
    }

    #[tokio::test]
    #[ignore = "requires network"]
    async fn gateio_fetches_kas() {
        let feed = PriceFeed::new(reqwest::Client::new());
        let samples = feed.gateio(&[ChannelId::KaspaTn10]).await.expect("fetch");
        assert_eq!(samples.len(), 1);
        assert!(samples[0].1.price > 0.0);
    }
}

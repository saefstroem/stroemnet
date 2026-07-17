mod error;
mod fetch;
mod result;

pub use error::OracleError;
pub use fetch::PriceFeed;
pub use result::Result;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use stroemnet_amounts::PriceStorage;
#[cfg(not(target_arch = "wasm32"))]
use tokio::time::sleep;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
/// The oracle responsible for aggregating prices
/// from online sources and storing them
pub struct Oracle {
    price_storage: PriceStorage,
    feed: PriceFeed,
    update_interval_secs: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Oracle {
    pub fn new(price_storage: PriceStorage, update_interval_secs: u64) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            price_storage,
            feed: PriceFeed::new(client),
            update_interval_secs,
        })
    }

    pub fn run_loop(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                if let Err(e) = self.update_all_prices().await {
                    tracing::warn!(
                        "price update failed; retaining last baseline (stale prices fail closed): {e}"
                    );
                }

                sleep(Duration::from_secs(self.update_interval_secs.max(1))).await;
            }
        })
    }

    async fn update_all_prices(&self) -> Result<()> {
        let channels = self.price_storage.channels();
        let prices = self.feed.aggregate(&channels).await?;

        for (channel, price) in prices {
            self.price_storage.set(channel, price);
        }

        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use stroemnet_protocol::ChannelId;

    use super::*;
    use tokio::time::{Duration, sleep};

    fn init_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .try_init();
    }

    #[test]
    fn test_oracle_creation() {
        let channels = vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia];
        let price_storage = PriceStorage::new(channels);
        assert_eq!(price_storage.get(&ChannelId::KaspaTn10), None);
        assert_eq!(price_storage.get(&ChannelId::EthereumSepolia), None);
    }

    #[test]
    fn test_channel_id_to_string() {
        assert_eq!(ChannelId::KaspaTn10.to_string(), "Kaspa TN10");
        assert_eq!(ChannelId::EthereumSepolia.to_string(), "Sepolia");
    }

    #[test]
    fn test_channel_id_ticker_symbol() {
        assert_eq!(ChannelId::KaspaTn10.ticker_symbol(), "KAS");
        assert_eq!(ChannelId::EthereumSepolia.ticker_symbol(), "ETH");
    }

    #[tokio::test]
    #[ignore = "requires network"]
    async fn test_fetch_real_prices() {
        init_tracing();
        let channels = vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia];
        let price_storage = PriceStorage::new(channels);
        let oracle = Oracle::new(price_storage.clone(), 60).unwrap();

        oracle
            .update_all_prices()
            .await
            .expect("Failed to update prices");

        let kas_price = price_storage.get(&ChannelId::KaspaTn10).unwrap();
        assert!(
            kas_price > 0.0,
            "Expected positive KAS price, got {}",
            kas_price
        );
        println!("Kaspa price: ${:.6}", kas_price);

        let eth_price = price_storage.get(&ChannelId::EthereumSepolia).unwrap();
        assert!(
            eth_price > 0.0,
            "Expected positive ETH price, got {}",
            eth_price
        );
        println!("Ethereum price: ${:.2}", eth_price);
    }

    #[tokio::test]
    async fn test_price_update_loop() {
        let channels = vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia];
        let price_storage = PriceStorage::new(channels);
        let oracle = Oracle::new(price_storage.clone(), 60).unwrap();

        let handle = oracle.run_loop();
        sleep(Duration::from_millis(100)).await;
        assert!(!handle.is_finished());
    }
}

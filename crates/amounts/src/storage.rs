use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use parking_lot::RwLock;
use stroemnet_protocol::ChannelId;

#[derive(Debug, Clone)]
/// Thread-safe storage for channel price information, allowing concurrent reads and writes.
/// Used to store and retrieve the latest price for each channel.
///
/// This is used by LPs to track the latest price for each channel,
/// which is needed to compute output amount for a swap.
pub struct PriceStorage {
    channels: Arc<RwLock<AHashSet<ChannelId>>>,
    prices: Arc<RwLock<AHashMap<ChannelId, f64>>>,
}

impl PriceStorage {
    /// Creates a new PriceStorage with the given channels initialized to a price of 0.0.
    pub fn new(channels: Vec<ChannelId>) -> Self {
        let channel_set = channels.iter().copied().collect::<AHashSet<ChannelId>>();
        let prices = channels
            .into_iter()
            .map(|channel| (channel, 0.0))
            .collect::<AHashMap<ChannelId, f64>>();

        Self {
            channels: Arc::new(RwLock::new(channel_set)),
            prices: Arc::new(RwLock::new(prices)),
        }
    }

    /// Retrieves the price for the given channel, if it exists.
    pub fn get(&self, channel: &ChannelId) -> Option<f64> {
        self.prices.read().get(channel).cloned()
    }

    /// Returns a list of all channels currently stored.
    pub fn channels(&self) -> Vec<ChannelId> {
        self.channels.read().iter().copied().collect()
    }

    /// Sets the price for the given channel.
    pub fn set(&self, channel: ChannelId, price: f64) {
        self.channels.write().insert(channel);
        self.prices.write().insert(channel, price);
    }

    /// Clears all stored prices.
    pub fn clear(&self) {
        self.prices.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_new_storage_initializes_with_zero_prices() {
        let channels = vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia];
        let storage = PriceStorage::new(channels.clone());
        for channel in channels {
            assert_eq!(storage.get(&channel), Some(0.0));
        }
    }

    #[test]
    fn test_new_storage_with_empty_channels() {
        let storage = PriceStorage::new(vec![]);
        assert_eq!(storage.channels().len(), 0);
    }

    #[test]
    fn test_new_storage_with_single_channel() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(0.0));
        let stored = storage.channels();
        assert_eq!(stored.len(), 1);
        assert!(stored.contains(&ChannelId::KaspaTn10));
    }

    #[test]
    fn test_get_existing_channel() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia]);
        storage.set(ChannelId::KaspaTn10, 100.5);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(100.5));
    }

    #[test]
    fn test_get_non_existing_channel() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        assert_eq!(storage.get(&ChannelId::EthereumSepolia), None);
    }

    #[test]
    fn test_get_returns_most_recent_value() {
        let storage = PriceStorage::new(vec![ChannelId::EthereumSepolia]);
        storage.set(ChannelId::EthereumSepolia, 1000.0);
        storage.set(ChannelId::EthereumSepolia, 2000.0);
        storage.set(ChannelId::EthereumSepolia, 3000.0);
        assert_eq!(storage.get(&ChannelId::EthereumSepolia), Some(3000.0));
    }

    #[test]
    fn test_set_updates_existing_channel() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::KaspaTn10, 250.75);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(250.75));
    }

    #[test]
    fn test_set_creates_new_channel_entry() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::EthereumSepolia, 3500.0);
        assert_eq!(storage.get(&ChannelId::EthereumSepolia), Some(3500.0));
    }

    #[test]
    fn test_set_with_zero_price() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::KaspaTn10, 0.0);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(0.0));
    }

    #[test]
    fn test_set_with_negative_price() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::KaspaTn10, -100.0);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(-100.0));
    }

    #[test]
    fn test_set_with_very_large_price() {
        let storage = PriceStorage::new(vec![ChannelId::EthereumSepolia]);
        let large = f64::MAX / 2.0;
        storage.set(ChannelId::EthereumSepolia, large);
        assert_eq!(storage.get(&ChannelId::EthereumSepolia), Some(large));
    }

    #[test]
    fn test_set_with_very_small_price() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        let small = 0.000000001;
        storage.set(ChannelId::KaspaTn10, small);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(small));
    }

    #[test]
    fn test_channels_returns_all_initialized_channels() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia]);
        let stored = storage.channels();
        assert_eq!(stored.len(), 2);
        assert!(stored.contains(&ChannelId::KaspaTn10));
        assert!(stored.contains(&ChannelId::EthereumSepolia));
    }

    #[test]
    fn test_channels_includes_dynamically_added_channels() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::EthereumSepolia, 3000.0);
        let stored = storage.channels();
        assert_eq!(stored.len(), 2);
        assert!(stored.contains(&ChannelId::KaspaTn10));
        assert!(stored.contains(&ChannelId::EthereumSepolia));
    }

    #[test]
    fn test_channels_returns_unique_entries() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia]);
        storage.set(ChannelId::KaspaTn10, 100.0);
        storage.set(ChannelId::KaspaTn10, 200.0);
        storage.set(ChannelId::KaspaTn10, 300.0);
        assert_eq!(storage.channels().len(), 2);
    }

    #[test]
    fn test_concurrent_reads_from_same_channel() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::KaspaTn10, 100.0);

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let s = storage.clone();
                thread::spawn(move || s.get(&ChannelId::KaspaTn10))
            })
            .collect();

        for h in handles {
            assert_eq!(h.join().unwrap(), Some(100.0));
        }
    }

    #[test]
    fn test_concurrent_writes_to_same_channel() {
        let storage = PriceStorage::new(vec![ChannelId::EthereumSepolia]);

        let handles: Vec<_> = (0..100)
            .map(|i| {
                let s = storage.clone();
                thread::spawn(move || s.set(ChannelId::EthereumSepolia, i as f64))
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let final_price = storage.get(&ChannelId::EthereumSepolia).unwrap();
        assert!((0.0..100.0).contains(&final_price));
    }

    #[test]
    fn test_concurrent_operations_on_different_channels() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia]);

        let kaspa_handle = {
            let s = storage.clone();
            thread::spawn(move || {
                for i in 0..100 {
                    s.set(ChannelId::KaspaTn10, i as f64);
                }
            })
        };
        let ethereum_handle = {
            let s = storage.clone();
            thread::spawn(move || {
                for i in 0..100 {
                    s.set(ChannelId::EthereumSepolia, (i * 10) as f64);
                }
            })
        };

        kaspa_handle.join().unwrap();
        ethereum_handle.join().unwrap();

        let kaspa_price = storage.get(&ChannelId::KaspaTn10);
        let ethereum_price = storage.get(&ChannelId::EthereumSepolia);
        assert!(kaspa_price.is_some());
        assert!(ethereum_price.is_some());
        assert_ne!(kaspa_price, ethereum_price);
    }

    #[test]
    fn test_clone_shares_underlying_data() {
        let storage1 = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        let storage2 = storage1.clone();
        storage1.set(ChannelId::KaspaTn10, 100.0);
        assert_eq!(storage2.get(&ChannelId::KaspaTn10), Some(100.0));
    }

    #[test]
    fn test_clone_bidirectional_updates() {
        let storage1 = PriceStorage::new(vec![ChannelId::EthereumSepolia]);
        let storage2 = storage1.clone();
        storage2.set(ChannelId::EthereumSepolia, 2500.0);
        assert_eq!(storage1.get(&ChannelId::EthereumSepolia), Some(2500.0));
    }

    #[test]
    fn test_special_float_values() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia]);
        storage.set(ChannelId::KaspaTn10, f64::INFINITY);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(f64::INFINITY));
        storage.set(ChannelId::EthereumSepolia, f64::NEG_INFINITY);
        assert_eq!(
            storage.get(&ChannelId::EthereumSepolia),
            Some(f64::NEG_INFINITY)
        );
    }

    #[test]
    fn test_nan_handling() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::KaspaTn10, f64::NAN);
        let p = storage.get(&ChannelId::KaspaTn10);
        assert!(p.is_some());
        assert!(p.unwrap().is_nan());
    }

    #[test]
    fn test_rapid_sequential_updates() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        for i in 0..1000 {
            storage.set(ChannelId::KaspaTn10, i as f64);
        }
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(999.0));
    }

    #[test]
    fn test_realistic_price_update_scenario() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia]);
        storage.set(ChannelId::KaspaTn10, 0.15);
        storage.set(ChannelId::EthereumSepolia, 3000.0);
        storage.set(ChannelId::KaspaTn10, 0.155);
        storage.set(ChannelId::EthereumSepolia, 3050.0);
        storage.set(ChannelId::KaspaTn10, 0.152);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(0.152));
        assert_eq!(storage.get(&ChannelId::EthereumSepolia), Some(3050.0));
        assert_eq!(storage.channels().len(), 2);
    }

    #[test]
    fn test_storage_survives_multiple_operations() {
        let channels = vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia];
        let storage = PriceStorage::new(channels.clone());
        for c in &channels {
            storage.set(*c, 100.0);
        }
        let _ = storage.channels();
        for c in &channels {
            let _ = storage.get(c);
        }
        for c in &channels {
            storage.set(*c, 200.0);
        }
        for c in &channels {
            assert_eq!(storage.get(c), Some(200.0));
        }
    }

    #[test]
    fn test_storage_with_all_channel_types() {
        let all = vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia];
        let storage = PriceStorage::new(all.clone());
        storage.set(ChannelId::KaspaTn10, 0.15);
        storage.set(ChannelId::EthereumSepolia, 3000.0);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(0.15));
        assert_eq!(storage.get(&ChannelId::EthereumSepolia), Some(3000.0));
        assert_eq!(storage.channels().len(), all.len());
    }

    #[test]
    fn test_clear_removes_prices_but_keeps_channels() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia]);
        storage.set(ChannelId::KaspaTn10, 0.15);
        storage.set(ChannelId::EthereumSepolia, 3000.0);
        storage.clear();
        assert_eq!(storage.get(&ChannelId::KaspaTn10), None);
        assert_eq!(storage.get(&ChannelId::EthereumSepolia), None);
        assert_eq!(storage.channels().len(), 2);
        assert!(storage.channels().contains(&ChannelId::KaspaTn10));
        assert!(storage.channels().contains(&ChannelId::EthereumSepolia));
    }

    #[test]
    fn test_clear_empty_storage_is_noop() {
        let storage = PriceStorage::new(vec![]);
        storage.clear();
        assert!(storage.channels().is_empty());
    }
}

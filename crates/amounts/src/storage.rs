use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use parking_lot::RwLock;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::now_unix_secs;

/// Maximum age of the price
const DEFAULT_MAX_AGE_SECS: u64 = 300;

/// Maximum price deviation from last measurement.
const MAX_JUMP_RATIO: f64 = 0.5;

#[derive(Debug, Clone)]
/// A price storage storing USD valued prices for any channels
/// native token
pub struct PriceStorage {
    /// all the channels that we are calculating prices for
    channels: Arc<RwLock<AHashSet<ChannelId>>>,
    /// The prices themselves, we want to separate this
    /// from the data so that we can zero out the data
    /// if the prices are stale
    prices: Arc<RwLock<AHashMap<ChannelId, (f64, u64)>>>,
    max_age_secs: u64,
}

impl PriceStorage {
    /// Creates a new price storage with designated channels
    pub fn new(channels: Vec<ChannelId>) -> Self {
        Self::with_max_age(channels, DEFAULT_MAX_AGE_SECS)
    }

    /// Create a new price storage with a specified time for which prices are valid for
    pub fn with_max_age(channels: Vec<ChannelId>, max_age_secs: u64) -> Self {
        let channel_set = channels.into_iter().collect::<AHashSet<ChannelId>>();
        Self {
            channels: Arc::new(RwLock::new(channel_set)),
            prices: Arc::new(RwLock::new(AHashMap::new())),
            max_age_secs,
        }
    }

    /// Retrieve a price based on a given channel id
    pub fn get(&self, channel: &ChannelId) -> Option<f64> {
        let (price, ts) = *self.prices.read().get(channel)?;

        // Ensure the price itself is valid and that its not too old
        if price.is_finite()
            && price > 0.0
            && now_unix_secs().saturating_sub(ts) <= self.max_age_secs
        {
            Some(price)
        } else {
            // If its too old we return none
            None
        }
    }

    /// Get all the channels for which we are tracking prices for
    pub fn channels(&self) -> Vec<ChannelId> {
        self.channels.read().iter().copied().collect()
    }

    /// Set the price of a channel
    pub fn set(&self, channel: ChannelId, price: f64) {
        // Ensure the price that we are setting is valid and not lt 0
        if !price.is_finite() || price <= 0.0 {
            tracing::warn!("rejecting invalid price {price} for {channel:?}");
            return;
        }

        // We need to ensure that prices have not deviated too far from the last
        // price. Before we can do that we need to ensure the last price itself
        // was valid, and then we ensure that it does not exceed the max jump ratio
        // to reduce the risk of oracle failures.
        if let Some(last) = self.get(&channel)
            && (price - last).abs() / last > MAX_JUMP_RATIO
        {
            tracing::warn!(
                "rejecting price {price} for {channel:?}: exceeds {MAX_JUMP_RATIO} jump from {last}"
            );
            return;
        }
        // Now add the channel to the channels if it is the case that we actually
        // did not track this before, this allows us to simply add new channels to the system
        // if needed.
        self.channels.write().insert(channel);

        // Finally, update the price for this channel.
        self.prices
            .write()
            .insert(channel, (price, now_unix_secs()));
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;
    use std::thread;

    #[test]
    fn test_new_storage_has_no_price_until_set() {
        let channels = vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia];
        let storage = PriceStorage::new(channels.clone());
        for channel in channels {
            assert_eq!(storage.get(&channel), None);
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
        assert_eq!(storage.get(&ChannelId::KaspaTn10), None);
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
        storage.set(ChannelId::EthereumSepolia, 1100.0);
        storage.set(ChannelId::EthereumSepolia, 1200.0);
        assert_eq!(storage.get(&ChannelId::EthereumSepolia), Some(1200.0));
    }

    #[test]
    fn test_circuit_breaker_rejects_large_jump() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::KaspaTn10, 0.15);
        storage.set(ChannelId::KaspaTn10, 0.30);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(0.15));
        storage.set(ChannelId::KaspaTn10, 0.16);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(0.16));
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
    fn test_set_with_zero_price_is_rejected() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::KaspaTn10, 0.0);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), None);
    }

    #[test]
    fn test_set_with_negative_price_is_rejected() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::KaspaTn10, -100.0);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), None);
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
    fn test_special_float_values_are_rejected() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia]);
        storage.set(ChannelId::KaspaTn10, f64::INFINITY);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), None);
        storage.set(ChannelId::EthereumSepolia, f64::NEG_INFINITY);
        assert_eq!(storage.get(&ChannelId::EthereumSepolia), None);
    }

    #[test]
    fn test_nan_is_rejected() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        storage.set(ChannelId::KaspaTn10, f64::NAN);
        assert_eq!(storage.get(&ChannelId::KaspaTn10), None);
    }

    #[test]
    fn test_stale_price_reads_as_missing() {
        let storage = PriceStorage::with_max_age(vec![ChannelId::KaspaTn10], 0);
        storage.set(ChannelId::KaspaTn10, 0.15);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(storage.get(&ChannelId::KaspaTn10), None);
    }

    #[test]
    fn test_rapid_sequential_updates_within_band() {
        let storage = PriceStorage::new(vec![ChannelId::KaspaTn10]);
        let mut p = 1.0;
        storage.set(ChannelId::KaspaTn10, p);
        for _ in 0..1000 {
            p *= 1.1;
            storage.set(ChannelId::KaspaTn10, p);
        }
        assert_eq!(storage.get(&ChannelId::KaspaTn10), Some(p));
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
            storage.set(*c, 120.0);
        }
        for c in &channels {
            assert_eq!(storage.get(c), Some(120.0));
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
}

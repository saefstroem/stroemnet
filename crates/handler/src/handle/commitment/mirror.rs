use alloy::primitives::U256;

use crate::result::Result;
use crate::{Handler, HandlerError, required_init_lock_secs};
use stroemnet_amounts::Amounts;
use stroemnet_protocol::v1::{AddressesV1, AmountV1, CommitmentV1};
use stroemnet_protocol::{ChainClock, ChannelId};

impl Handler {
    /// Build a counter commitment based on the users initial commitment
    pub(super) fn build_counter_commitment(
        &self,
        swap_id: [u8; 32],
        init_commitment: &CommitmentV1,
        source_channel: ChannelId,
        destination_channel: ChannelId,
        clock: &ChainClock,
    ) -> Result<CommitmentV1> {
        // Compute the amount in from the commitment
        let amount_in = U256::from_str_radix(&init_commitment.amount.value, 10)?;

        // Retrieve the usd source price
        let source_usd_price = self
            .price_storage
            .get(&source_channel)
            .ok_or(HandlerError::MissingPriceData(source_channel))?;

        // Compute the amount in in f64
        let amount_in_f = amount_in.to_string().parse::<f64>()?;

        // Compute the scale by scaling
        let scale = 10u128.pow(init_commitment.amount.decimals as u32) as f64;

        // Scale down the amount in and multiply it by the source usd price to get usd value
        let amount_in_usd = (amount_in_f / scale) * source_usd_price;

        // If its below the threshold
        if amount_in_usd < self.config.min_trade_usd || amount_in_usd > self.config.max_trade_usd {
            return Err(HandlerError::InvalidAmount(amount_in));
        }

        // Get the destination usd price
        let destination_usd_price = self
            .price_storage
            .get(&destination_channel)
            .ok_or(HandlerError::MissingPriceData(destination_channel))?;

        // Compute the amount out for the given swap
        let amount_out = Amounts::amount_out(
            amount_in,
            source_usd_price,
            source_channel.decimals(),
            destination_usd_price,
            destination_channel.decimals(),
            self.config.spread_percent,
        )?;

        // Get the source timestamp right now
        let source_now = clock
            .now_checked(source_channel)
            .ok_or(HandlerError::ChainTimeUnavailable(source_channel))?;

        // Compute how much time is remaining
        let remaining = init_commitment.unlock_ts.saturating_sub(source_now);

        // Compute the required threshold for the destination chain
        let threshold = required_init_lock_secs(
            destination_channel,
            self.config.commit_buffer_secs,
            false
        );

        // If its below the threshold we cannot accept this swap, the only
        // way forward is a refund
        if remaining < threshold {
            return Err(HandlerError::InvalidLockTimeDuration(remaining));
        }

        // Compute the destination time stamp right now
        let dest_now = clock
            .now_checked(destination_channel)
            .ok_or(HandlerError::ChainTimeUnavailable(destination_channel))?;

        // Retrieve the desintation and source addresses for us as MM
        let mm_destination = self
            .address_lookup_table
            .get(&destination_channel)
            .ok_or(HandlerError::MissingAddress(destination_channel))?;
        let mm_source = self
            .address_lookup_table
            .get(&source_channel)
            .ok_or(HandlerError::MissingAddress(source_channel))?;

        // Create the counter commitment
        Ok(CommitmentV1 {
            swap_id,
            addresses: AddressesV1::new(
                mm_destination.clone(),
                init_commitment.addresses.sender_destination.clone(),
                mm_source.clone(),
            ),
            amount: AmountV1::new(amount_out.to_string(), destination_channel.decimals()),
            secret_hash: init_commitment.secret_hash,
            unlock_ts: dest_now + destination_channel.lock_time_secs(),
            source: destination_channel as u8,
            destination: source_channel as u8,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::test_fixtures::{create_test_handler_with, default_test_config, lp_addresses};
    use crate::{HandlerConfig, HandlerError};
    use stroemnet_protocol::v1::{AddressesV1, AmountV1, CommitmentV1};
    use stroemnet_protocol::{ChainClock, ChannelId};

    fn init(swap_id: [u8; 32], unlock_ts: u64) -> CommitmentV1 {
        CommitmentV1::new(
            swap_id,
            AddressesV1::new(
                "0xUserEthSender".into(),
                "0xMmEthereumAddress".into(),
                "kaspa:user_dest".into(),
            ),
            AmountV1::new("1000000000000000000".into(), 18),
            [0xEE; 32],
            unlock_ts,
            ChannelId::EthereumSepolia as u8,
            ChannelId::KaspaTn10 as u8,
        )
    }

    #[tokio::test]
    async fn valid_returns_mirrored_with_positive_amount() {
        let (h, tracker) = create_test_handler_with(
            default_test_config(),
            &[
                (ChannelId::KaspaTn10, 0.15),
                (ChannelId::EthereumSepolia, 3000.0),
            ],
            lp_addresses(),
        );
        let swap_id = [7u8; 32];
        let c = init(swap_id, u64::MAX);
        tracker
            .write()
            .await
            .set_init_commitment(swap_id, c.clone())
            .unwrap();
        let out = h
            .handle_internal_commitment(&c, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap();
        assert_eq!(out.source, ChannelId::KaspaTn10 as u8);
        assert_eq!(out.destination, ChannelId::EthereumSepolia as u8);
        assert_eq!(out.secret_hash, c.secret_hash);
        assert!(out.unlock_ts > 0);
    }

    #[tokio::test]
    async fn lock_time_too_close_is_rejected() {
        let (h, tracker) = create_test_handler_with(
            default_test_config(),
            &[
                (ChannelId::KaspaTn10, 0.15),
                (ChannelId::EthereumSepolia, 3000.0),
            ],
            lp_addresses(),
        );
        let swap_id = [6u8; 32];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let c = init(swap_id, now + 60);
        tracker
            .write()
            .await
            .set_init_commitment(swap_id, c.clone())
            .unwrap();
        let err = h
            .handle_internal_commitment(&c, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidLockTimeDuration(_)));
    }

    #[tokio::test]
    async fn amount_out_of_bounds_is_rejected() {
        let (h, tracker) = create_test_handler_with(
            HandlerConfig {
                min_trade_usd: 5000.0,
                max_trade_usd: 10_000.0,
                ..default_test_config()
            },
            &[
                (ChannelId::KaspaTn10, 0.15),
                (ChannelId::EthereumSepolia, 3000.0),
            ],
            lp_addresses(),
        );
        let swap_id = [8u8; 32];
        let c = init(swap_id, u64::MAX);
        tracker
            .write()
            .await
            .set_init_commitment(swap_id, c.clone())
            .unwrap();
        let err = h
            .handle_internal_commitment(&c, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidAmount(_)));
    }
}

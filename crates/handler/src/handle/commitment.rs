use alloy::primitives::U256;

use crate::result::Result;
use crate::{Handler, HandlerError, normalised_address_eq, required_init_lock_secs};
use stroemnet_amounts::Amounts;
use stroemnet_protocol::v1::{AddressesV1, AmountV1, CommitmentV1};
use stroemnet_protocol::{ChainClock, ChannelId};

impl Handler {
    /// Handles an internal commitment coming from another channel
    /// but from within our own system.
    pub async fn handle_internal_commitment(
        &self,
        commitment: &CommitmentV1,
        channel_id: ChannelId,
        clock: &ChainClock,
    ) -> Result<CommitmentV1> {
        tracing::info!("Handling internal commitment: {:?}", commitment);

        // Try and check if this is an existing swap or not.
        let tracker_read = self.swap_tracker.read().await;
        let record = tracker_read
            .get_swap(&commitment.swap_id)
            .ok_or(HandlerError::SwapNotFound(commitment.swap_id))?;

        // If the swap exists, we expect it to be in the InitLock stage,
        // meaning we have seen the init commitment but not the counter commitment yet.
        // If there is a counter commitment already it means that the swap's two parties
        // have already locked the swap and we should not be receiving any more commitments for this swap.
        if record.counter_commitment.is_some() || record.resolution.is_some() {
            tracing::warn!(
                "Received commitment for swap that is not in InitLock state. Swap ID: {:?}",
                commitment.swap_id
            );
            return Err(HandlerError::InvalidState(commitment.swap_id));
        }

        // Since we dont have any counter commitment yet, this means that
        // most likely we need to counter this commitment by creating a mirrored commitment

        // Lets read the init commitment
        let init_commitment = record.init_commitment.clone();

        let source_channel = ChannelId::try_from(init_commitment.source)?;
        let destination_channel = ChannelId::try_from(init_commitment.destination)?;

        // If the destination channel doesnt match the channel id of this invocation
        // it means that this swap is not intended for our channel.
        if destination_channel != channel_id {
            tracing::error!(
                "Received commitment for swap that is not intended for our channel. Swap ID: {:?}, Destination: {:?}, Our Channel: {:?}",
                commitment.swap_id,
                destination_channel,
                channel_id
            );
            return Err(HandlerError::InvalidChannelId(destination_channel));
        }

        // Retrieve our address for the source channel of this swap
        let our_source_address = self
            .address_lookup_table
            .get(&source_channel)
            .ok_or(HandlerError::MissingAddress(source_channel))?;

        // If we are to receive the proceeds, it must mean that our source address
        // matches the direct recipient on the source chain, if it does not
        // we should simply skip this commitment as it is not addressed to us
        if !normalised_address_eq(
            source_channel,
            &init_commitment.addresses.receiver,
            our_source_address,
        ) {
            tracing::debug!(
                "Skipping commitment not addressed to us. Swap ID: {:?}, \
                 receiver: {}, our address on {}: {}",
                commitment.swap_id,
                init_commitment.addresses.receiver,
                source_channel,
                our_source_address
            );
            return Err(HandlerError::NotAddressedToUs(commitment.swap_id));
        }

        // Parse the amount in as u256
        let amount_in = U256::from_str_radix(&init_commitment.amount.value, 10)?;
        let source_usd_price = self
            .price_storage
            .get(&source_channel)
            .ok_or(HandlerError::MissingPriceData(source_channel))?;

        // Compute it in float, scale it to the decimals of the init commitment
        let amount_in_f = amount_in.to_string().parse::<f64>()?;
        let scale = 10u128.pow(record.init_commitment.amount.decimals as u32) as f64;

        // Compute the amountin in USD value
        let amount_in_usd = (amount_in_f / scale) * source_usd_price;

        // If the amount in USD is below a minimum trade value or above a minimum trade value
        // we skip the swap
        if amount_in_usd < self.config.min_trade_usd || amount_in_usd > self.config.max_trade_usd {
            tracing::error!(
                "Received commitment with USD value out of bounds. USD Value: {}, Min: {}, Max: {}. Swap ID: {:?}",
                amount_in_usd,
                self.config.min_trade_usd,
                self.config.max_trade_usd,
                commitment.swap_id
            );
            return Err(HandlerError::InvalidAmount(amount_in));
        }
        drop(tracker_read);

        // Retrieve the usd price for the destination token
        let destination_usd_price = self
            .price_storage
            .get(&destination_channel)
            .ok_or(HandlerError::MissingPriceData(destination_channel))?;

        // Now compute an instant amount out for the specified price
        let amount_out = Amounts::amount_out(
            amount_in,
            source_usd_price,
            source_channel.decimals(),
            destination_usd_price,
            destination_channel.decimals(),
            self.config.spread_percent,
        )?;

        // Retrieve the current timestamp
        let source_now = clock
            .now_checked(source_channel)
            .ok_or(HandlerError::ChainTimeUnavailable(source_channel))?;

        // Compute how much time left there is for the initiating party
        // until they are able to refund.
        let lock_time_duration = init_commitment.unlock_ts.saturating_sub(source_now);

        // Compute the required init lock duration that we have configured
        let threshold_duration =
            required_init_lock_secs(destination_channel, self.config.commit_buffer_secs);

        // If the remaining duration of the initiating party's swap
        // we throw an error, because the user did not lock for long enough
        // their swap will be refunded via CCR.
        if lock_time_duration < threshold_duration {
            tracing::error!(
                "Init-lock duration too short for {} finality. \
                 Duration: {}s, Required: {}s. Swap ID: {:?}",
                destination_channel.to_string(),
                lock_time_duration,
                threshold_duration,
                commitment.swap_id
            );
            return Err(HandlerError::InvalidLockTimeDuration(lock_time_duration));
        }

        // Now compute our lock time which is basically the current timestamp
        // and the finality seconds for this chain.
        let dest_now = clock
            .now_checked(destination_channel)
            .ok_or(HandlerError::ChainTimeUnavailable(destination_channel))?;
        let returning_unlock_ts = dest_now + destination_channel.finality_secs();

        // Retrieve destination address for us
        let mm_destination_address = self
            .address_lookup_table
            .get(&destination_channel)
            .ok_or(HandlerError::MissingAddress(destination_channel))?;

        // Retrieve our source address
        let mm_source_address = self
            .address_lookup_table
            .get(&source_channel)
            .ok_or(HandlerError::MissingAddress(source_channel))?;

        // Now create a counter commitment with the same swap
        let returning_commitment = CommitmentV1 {
            swap_id: commitment.swap_id,
            addresses: AddressesV1::new(
                mm_destination_address.clone(),
                init_commitment.addresses.sender_destination.clone(),
                mm_source_address.clone(),
            ),
            amount: AmountV1::new(amount_out.to_string(), destination_channel.decimals()),
            secret_hash: init_commitment.secret_hash,
            unlock_ts: returning_unlock_ts,
            source: destination_channel as u8,
            destination: source_channel as u8,
        };
        Ok(returning_commitment)
    }

    /// Function for internally mapping external commitments to swap states
    /// within the system.
    pub async fn handle_external_commitment(&self, commitment: CommitmentV1) -> Result<()> {
        tracing::info!("Handling external commitment: {:?}", commitment);

        let mut tracker_write = self.swap_tracker.write().await;
        // Try and retrieve a swap with this id.
        if let Some(record) = tracker_write.get_swap(&commitment.swap_id) {
            // If it exists it is a duplicate, and therefore we should discard it
            if record.init_commitment == commitment {
                tracing::debug!(
                    "duplicate init commitment for swap {} — ignoring",
                    hex::encode(commitment.swap_id)
                );
                return Ok(());
            }
            if record.counter_commitment.as_ref() == Some(&commitment) {
                tracing::debug!(
                    "duplicate counter commitment for swap {} — ignoring",
                    hex::encode(commitment.swap_id)
                );
                return Ok(());
            }

            // We shouldnt allow the initial source to submit a counter commitment
            // regardless if they made a mistake or malicious reason
            if record.init_commitment.source == commitment.source {
                tracing::debug!(
                    "same-source re-arrival for swap {} (source={}) — ignoring",
                    hex::encode(commitment.swap_id),
                    commitment.source
                );
                return Ok(());
            }

            // All verifications passed, we can set the counter commitment
            tracker_write.set_counter_commitment(commitment.swap_id, commitment)?;
        } else {
            // If it does not exist, it means it is the first commitment
            tracker_write.set_init_commitment(commitment.swap_id, commitment)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::RwLock;

    use crate::HandlerConfig;
    use crate::test_fixtures::{create_test_handler_with, default_test_config, lp_addresses};
    use crate::{Handler, HandlerError};
    use stroemnet_protocol::{ChainClock, ChannelId};
    use stroemnet_protocol::swap_tracker::{SwapStage, SwapTracker};
    use stroemnet_protocol::v1::{AddressesV1, AmountV1, CommitmentV1};

    async fn create_test_handler(
        kaspa_price: f64,
        eth_price: f64,
    ) -> (Handler, Arc<RwLock<SwapTracker>>) {
        create_test_handler_with(
            default_test_config(),
            &[
                (ChannelId::KaspaTn10, kaspa_price),
                (ChannelId::EthereumSepolia, eth_price),
            ],
            lp_addresses(),
        )
    }

    fn mock_init_commitment(swap_id: [u8; 32]) -> CommitmentV1 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        CommitmentV1 {
            swap_id,
            addresses: AddressesV1::new(
                "0xUserEthSender".to_string(),
                "0xMmEthereumAddress".to_string(),
                "kaspa:user_dest_address".to_string(),
            ),
            amount: AmountV1::new("1000000000000000000".to_string(), 18),

            secret_hash: [0xEE; 32],
            unlock_ts: now + 3600,
            source: ChannelId::EthereumSepolia as u8,
            destination: ChannelId::KaspaTn10 as u8,
        }
    }

    fn mock_counter_commitment(swap_id: [u8; 32]) -> CommitmentV1 {
        CommitmentV1 {
            swap_id,
            addresses: AddressesV1::new(
                "kaspa:mm_kaspa_address".to_string(),
                "kaspa:user_dest_address".to_string(),
                "0xMmEthereumAddress".to_string(),
            ),
            amount: AmountV1::new("50000000".to_string(), 18),
            secret_hash: [0xEE; 32],
            unlock_ts: u64::MAX,
            source: ChannelId::KaspaTn10 as u8,
            destination: ChannelId::EthereumSepolia as u8,
        }
    }

    #[tokio::test]
    async fn external_commitment_new_swap_creates_init_lock() {
        let (handler, tracker) = create_test_handler(0.15, 3000.0).await;
        let swap_id = [1u8; 32];
        let commitment = mock_init_commitment(swap_id);

        handler
            .handle_external_commitment(commitment.clone())
            .await
            .unwrap();

        let t = tracker.read().await;
        let record = t.get_swap(&swap_id).unwrap();
        assert_eq!(SwapTracker::stage(record), SwapStage::Initialized);
        assert_eq!(record.init_commitment.swap_id, swap_id);
        assert_eq!(
            record.init_commitment.addresses.sender,
            commitment.addresses.sender
        );
    }

    #[tokio::test]
    async fn external_commitment_existing_init_lock_transitions_to_lock() {
        let (handler, tracker) = create_test_handler(0.15, 3000.0).await;
        let swap_id = [2u8; 32];

        let init_commitment = mock_init_commitment(swap_id);
        handler
            .handle_external_commitment(init_commitment.clone())
            .await
            .unwrap();

        let counter = mock_counter_commitment(swap_id);
        handler
            .handle_external_commitment(counter.clone())
            .await
            .unwrap();

        let t = tracker.read().await;
        let record = t.get_swap(&swap_id).unwrap();
        assert_eq!(SwapTracker::stage(record), SwapStage::Locked);
        assert_eq!(record.init_commitment.swap_id, swap_id);
        assert_eq!(
            record.counter_commitment.as_ref().unwrap().addresses.sender,
            counter.addresses.sender
        );
    }

    #[tokio::test]
    async fn internal_commitment_swap_not_found() {
        let (handler, _tracker) = create_test_handler(0.15, 3000.0).await;
        let swap_id = [3u8; 32];
        let commitment = mock_init_commitment(swap_id);

        let err = handler
            .handle_internal_commitment(&commitment, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::SwapNotFound(_)));
    }

    #[tokio::test]
    async fn internal_commitment_wrong_state() {
        let (handler, tracker) = create_test_handler(0.15, 3000.0).await;
        let swap_id = [4u8; 32];

        let init = mock_init_commitment(swap_id);
        let counter = mock_counter_commitment(swap_id);
        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, init.clone()).unwrap();
            t.set_counter_commitment(swap_id, counter).unwrap();
        }

        let err = handler
            .handle_internal_commitment(&init, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidState(_)));
    }

    #[tokio::test]
    async fn internal_commitment_wrong_destination_channel() {
        let (handler, tracker) = create_test_handler(0.15, 3000.0).await;
        let swap_id = [5u8; 32];

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let init = CommitmentV1 {
            swap_id,
            addresses: AddressesV1::new(
                "kaspa:sender".to_string(),
                "kaspa:receiver".to_string(),
                "0xSenderDest".to_string(),
            ),
            amount: AmountV1::new("100000000".to_string(), 8),
            secret_hash: [0xEE; 32],
            unlock_ts: now + 3600,
            source: ChannelId::KaspaTn10 as u8,
            destination: ChannelId::EthereumSepolia as u8,
        };
        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, init.clone()).unwrap();
        }

        let err = handler
            .handle_internal_commitment(&init, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidChannelId(_)));
    }

    #[tokio::test]
    async fn internal_commitment_lock_time_too_close() {
        let (handler, tracker) = create_test_handler(0.15, 3000.0).await;
        let swap_id = [6u8; 32];

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let init = CommitmentV1 {
            swap_id,
            addresses: AddressesV1::new(
                "0xUserEthSender".to_string(),
                "0xMmEthereumAddress".to_string(),
                "kaspa:user_dest_address".to_string(),
            ),
            amount: AmountV1::new("1000000000000000000".to_string(), 18),
            secret_hash: [0xEE; 32],
            unlock_ts: now + 60,
            source: ChannelId::EthereumSepolia as u8,
            destination: ChannelId::KaspaTn10 as u8,
        };
        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, init.clone()).unwrap();
        }

        let err = handler
            .handle_internal_commitment(&init, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidLockTimeDuration(_)));
    }

    #[tokio::test]
    async fn internal_commitment_valid_returns_mirrored() {
        let (handler, tracker) = create_test_handler(0.15, 3000.0).await;
        let swap_id = [7u8; 32];
        let commitment = mock_init_commitment(swap_id);
        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, commitment.clone()).unwrap();
        }

        let result = handler
            .handle_internal_commitment(&commitment, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap();

        assert_eq!(result.swap_id, swap_id);

        assert_eq!(result.addresses.sender, "kaspa:mm_kaspa_address");
        assert_eq!(
            result.addresses.receiver,
            commitment.addresses.sender_destination
        );

        assert_eq!(result.addresses.sender_destination, "0xMmEthereumAddress");

        assert_eq!(result.source, ChannelId::KaspaTn10 as u8);
        assert_eq!(result.destination, ChannelId::EthereumSepolia as u8);
        assert_eq!(result.secret_hash, commitment.secret_hash);
        assert!(result.unlock_ts > 0);
        let amount: alloy::primitives::U256 =
            alloy::primitives::U256::from_str_radix(&result.amount.value, 10).unwrap();
        assert!(amount > alloy::primitives::U256::ZERO);
    }

    #[tokio::test]
    async fn internal_commitment_amount_out_of_bounds() {
        let (handler, swap_tracker) = create_test_handler_with(
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
        let commitment = mock_init_commitment(swap_id);
        {
            let mut t = swap_tracker.write().await;
            t.set_init_commitment(swap_id, commitment.clone()).unwrap();
        }

        let err = handler
            .handle_internal_commitment(&commitment, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidAmount(_)));
    }

    #[tokio::test]
    async fn internal_commitment_rejected_when_not_addressed_to_us() {
        let (handler, swap_tracker) = create_test_handler(0.0001, 1000.0).await;
        let swap_id = [42u8; 32];
        let mut commitment = mock_init_commitment(swap_id);

        commitment.addresses.receiver = "0xSomeOtherLpEthAddress".to_string();

        {
            let mut t = swap_tracker.write().await;
            t.set_init_commitment(swap_id, commitment.clone()).unwrap();
        }

        let err = handler
            .handle_internal_commitment(&commitment, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap_err();
        assert!(
            matches!(err, HandlerError::NotAddressedToUs(id) if id == swap_id),
            "expected NotAddressedToUs, got {err:?}"
        );

        let our_swap_id = [43u8; 32];
        let our_commit = mock_init_commitment(our_swap_id);
        {
            let mut t = swap_tracker.write().await;
            t.set_init_commitment(our_swap_id, our_commit.clone())
                .unwrap();
        }
        let counter = handler
            .handle_internal_commitment(&our_commit, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .expect("commit addressed to us should pass the filter");
        assert_eq!(counter.swap_id, our_swap_id);
    }
}

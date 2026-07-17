use crate::error::HandlerError;
use crate::result::Result;
use crate::{Handler, required_init_lock_secs};
use alloy::primitives::U256;
use stroemnet_amounts::Amounts;
use stroemnet_protocol::ChannelId;

#[derive(Clone, Debug)]
/// A swap request it contains the origin destination
/// and amount, this is what LPs quote against
pub struct SwapRequest {
    pub origin: u8,
    pub destination: u8,
    pub amount: String,
}

#[derive(Clone, Debug)]
/// A proposal from an LP containing origin, destination
/// how much amount in yields amount out, the LPs destination address
/// and the offset seconds that the user needs to lock for in order for the
/// LP to consider it valid, too short lock times give in general quite substantial risk to
/// the LP.
pub struct TradeProposal {
    /// Where the swap originates from
    pub origin: ChannelId,
    /// Where the swap is going
    pub destination: ChannelId,
    /// Amount in to swap
    pub amount_in: String,
    /// Amount out in the destination token
    pub amount_out: String,
    /// The senders destination address for which the receiver should lock funds against
    pub sender_destination_address: String,
    /// Required offset i.e. lock time for the user
    pub commit_unlock_offset_secs: u64,
}

impl Handler {
    /// For a user's swap request we need to create a countering trade proposal
    /// essentially a quote.
    pub async fn create_proposal(&self, request: &SwapRequest) -> Result<TradeProposal> {
        let origin = ChannelId::try_from(request.origin)?;
        let destination = ChannelId::try_from(request.destination)?;

        let source_usd_price = self
            .price_storage
            .get(&origin)
            .ok_or(HandlerError::MissingPriceData(origin))?;

        let destination_usd_price = self
            .price_storage
            .get(&destination)
            .ok_or(HandlerError::MissingPriceData(destination))?;

        // Parse amount in
        let amount_in = U256::from_str_radix(&request.amount, 10)?;

        // Conver tthe amount in to f64
        let amount_in_f = amount_in.to_string().parse::<f64>()?;

        // Compute the scale for which we need to divide by to get it to 'ether' units
        let scale = 10u128.pow(origin.decimals() as u32) as f64;

        // Scale down to ether units and multiply by source usd price to get the amount in usd
        let amount_in_usd = (amount_in_f / scale) * source_usd_price;

        // If the amount in usd is below the minimum threshold we reject
        if amount_in_usd < self.config.min_trade_usd {
            return Err(HandlerError::TradeTooSmall {
                amount_in: request.amount.clone(),
                amount_in_usd,
                min_usd: self.config.min_trade_usd,
            });
        }

        // If the amount in is above the max threshold we reject too
        if amount_in_usd > self.config.max_trade_usd {
            return Err(HandlerError::TradeTooLarge {
                amount_in: request.amount.clone(),
                amount_in_usd,
                max_usd: self.config.max_trade_usd,
            });
        }

        // Compute amount out,
        let output = Amounts::amount_out(
            amount_in,
            source_usd_price,
            origin.decimals(),
            destination_usd_price,
            destination.decimals(),
            self.config.spread_percent,
        )?;

        tracing::info!(
            "Computed proposal output amount: {output} (amount_in: {amount_in}, source_price: {source_usd_price}, destination_price: {destination_usd_price}, spread_percent: {}, usd_value: {amount_in_usd})",
            self.config.spread_percent
        );

        // Retrieve the sender destination address, from the origin
        // (our address)
        let sender_destination_address = self
            .address_lookup_table
            .get(&origin)
            .ok_or(HandlerError::UnknownChannel(origin))?
            .clone();

        // Compute the commit unlock offset
        // This time we enforce a buffer in order to account for the time it takes for users
        // commitment to propagate across all networks and reach us again
        let commit_unlock_offset_secs =
            required_init_lock_secs(destination, self.config.commit_buffer_secs,true);

        Ok(TradeProposal {
            destination: origin,
            origin: destination,
            amount_out: request.amount.clone(),
            amount_in: output.to_string(),
            sender_destination_address,
            commit_unlock_offset_secs,
        })
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
    use ahash::AHashMap;
    use alloy::primitives::U256;

    use crate::test_fixtures::{create_test_handler_with, default_test_config, lp_addresses};
    use crate::{Handler, HandlerError};
    use stroemnet_protocol::ChannelId;

    use super::SwapRequest;

    async fn setup_with_prices(kaspa_price: f64, eth_price: f64) -> Handler {
        let (handler, _) = create_test_handler_with(
            default_test_config(),
            &[
                (ChannelId::KaspaTn10, kaspa_price),
                (ChannelId::EthereumSepolia, eth_price),
            ],
            lp_addresses(),
        );
        handler
    }

    #[tokio::test]
    async fn create_proposal_valid_flow() {
        let handler = setup_with_prices(0.15, 3000.0).await;

        let request = SwapRequest {
            origin: ChannelId::EthereumSepolia as u8,
            destination: ChannelId::KaspaTn10 as u8,
            amount: "1000000000000000000".to_string(),
        };

        let proposal = handler.create_proposal(&request).await.unwrap();

        assert_eq!(proposal.origin, ChannelId::KaspaTn10);
        assert_eq!(proposal.destination, ChannelId::EthereumSepolia);
        let amount: U256 = U256::from_str_radix(&proposal.amount_out, 10).unwrap();
        assert!(amount > U256::ZERO, "Proposal amount should be positive");
    }

    #[tokio::test]
    async fn create_proposal_missing_source_price() {
        let (handler, _) = create_test_handler_with(
            default_test_config(),
            &[(ChannelId::KaspaTn10, 0.15)],
            AHashMap::new(),
        );

        let request = SwapRequest {
            origin: ChannelId::EthereumSepolia as u8,
            destination: ChannelId::KaspaTn10 as u8,
            amount: "1000".to_string(),
        };

        let err = handler.create_proposal(&request).await.unwrap_err();
        assert!(matches!(err, HandlerError::MissingPriceData(_)));
    }

    #[tokio::test]
    async fn create_proposal_invalid_amount() {
        let handler = setup_with_prices(0.15, 3000.0).await;

        let request = SwapRequest {
            origin: ChannelId::EthereumSepolia as u8,
            destination: ChannelId::KaspaTn10 as u8,
            amount: "not_a_number".to_string(),
        };

        let err = handler.create_proposal(&request).await;
        assert!(err.is_err());
    }
}

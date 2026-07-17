use alloy_primitives::U256;

use crate::error::AmountError;
use crate::result::Result;

pub struct Amounts;

impl Amounts {
    /// The number of decimal places to use for price calculations.
    /// This is a canonical decimal precision for any kind of reasoning with amounts.
    pub const PRICE_DECIMALS: u8 = 8;

    /// A canonical function for computing the output amount for a swap
    /// given input amount, source and destination prices, and a spread percentage.
    /// The spread percentage is a value between 0 and 100, representing the fee taken from the output amount.
    pub fn amount_out(
        amount_in: U256,
        source_usd_price: f64,
        source_decimals: u8,
        destination_usd_price: f64,
        destination_decimals: u8,
        spread_percent: f64,
    ) -> Result<U256> {
        tracing::info!(
            "Calculating amount_out with amount_in: {amount_in}, source_usd_price: {source_usd_price}, source_decimals: {source_decimals}, destination_usd_price: {destination_usd_price}, destination_decimals: {destination_decimals}, spread_percent: {spread_percent}"
        );

        // Validate that the source price is finite and non-negative,
        if !source_usd_price.is_finite() || source_usd_price < 0.0 {
            return Err(AmountError::InvalidPriceData(source_usd_price));
        }

        // Validate that the destination price is finite and positive,
        if !destination_usd_price.is_finite() || destination_usd_price <= 0.0 {
            return Err(AmountError::InvalidPriceData(destination_usd_price));
        }

        // Ensure that spread is within 0, 100% this is the spread set by the LP node
        if !(0.0..100.0).contains(&spread_percent) {
            return Err(AmountError::InvalidPriceData(spread_percent));
        }

        // Compute the price scale
        let price_scale = 10f64.powi(Self::PRICE_DECIMALS as i32);

        // We increase the source and destination price by the price scale in accordance
        // with the price decimals. Essentially 1e8
        // We need ot scale these prices in order to turn them into U256 which allows for easy multiplication
        // without too much loss at the precision level.
        let source_price_fixed = U256::from((source_usd_price * price_scale).round() as u128);
        let dest_price_fixed = U256::from((destination_usd_price * price_scale).round() as u128);

        // The output calculation  works in the way that we multiply the source input (amount in)
        // by the source price in order to compute the usd value of the input. Then we simply divide
        // by the destination price in order to get the output amount.
        let output_in_source_decimals = amount_in
            .checked_mul(source_price_fixed)
            .ok_or(AmountError::ArithmeticOverflow(amount_in))?
            .checked_div(dest_price_fixed)
            .ok_or(AmountError::DivisionByZero(dest_price_fixed))?;

        // Compute the bps instead of spread.
        let spread_bps = (spread_percent * 100.0) as u32;

        // Compute the spread multiplier which will be how much we need to reduce
        // the output by in order to account for the spread.
        let spread_multiplier = U256::from(10_000u32.saturating_sub(spread_bps));
        let spread_divisor = U256::from(10_000u32);

        // Multiply the output (which is in source decimals) by the spread multiplier
        // but then divide it by the full divisor effectively reducing it by spread_bps.
        let output_after_spread = output_in_source_decimals
            .checked_mul(spread_multiplier)
            .ok_or(AmountError::ArithmeticOverflow(output_in_source_decimals))?
            .checked_div(spread_divisor)
            .ok_or(AmountError::DivisionByZero(spread_divisor))?;

        // Now the output is fully ready, we just need to rescale it from the source decimals
        // to the destination decimals
        Self::rescale(output_after_spread, source_decimals, destination_decimals)
    }

    /// Rescales any value either up or down depending on the size of from and to decimals
    /// The final value is in to_decimals.
    pub fn rescale(amount: U256, from_decimals: u8, to_decimals: u8) -> Result<U256> {
        // Compute the diff
        let diff = to_decimals as i32 - from_decimals as i32;

        // If the diff is greater than 0
        // we need to multiply it by the diff to rescale it up to the larger
        // decimals
        if diff > 0 {
            amount
                .checked_mul(U256::from(10u64).pow(U256::from(diff as u64)))
                .ok_or(AmountError::AmountOverflow(amount))
        } else if diff < 0 {
            // If its less we do the opposite, we divide by the 10^-diff in order to scale it down.
            amount
                .checked_div(U256::from(10u64).pow(U256::from((-diff) as u64)))
                .ok_or(AmountError::AmountUnderflow(amount))
        } else {
            Ok(amount)
        }
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

    const KAS_DEC: u8 = 8;
    const ETH_DEC: u8 = 18;

    fn kas(whole: u64) -> U256 {
        U256::from(whole) * U256::from(10u64).pow(U256::from(KAS_DEC as u64))
    }
    fn eth(whole: u64) -> U256 {
        U256::from(whole) * U256::from(10u64).pow(U256::from(ETH_DEC as u64))
    }
    fn to_kas_f64(v: U256) -> f64 {
        v.to::<u128>() as f64 / 10f64.powi(KAS_DEC as i32)
    }
    fn to_eth_f64(v: U256) -> f64 {
        v.to::<u128>() as f64 / 10f64.powi(ETH_DEC as i32)
    }

    #[test]
    fn rescale_identity() {
        let v = U256::from(42u64);
        assert_eq!(Amounts::rescale(v, 8, 8).unwrap(), v);
    }

    #[test]
    fn rescale_up_8_to_18() {
        assert_eq!(
            Amounts::rescale(U256::from(100_000_000u64), 8, 18).unwrap(),
            U256::from(1_000_000_000_000_000_000u128),
        );
    }

    #[test]
    fn rescale_down_18_to_8() {
        assert_eq!(
            Amounts::rescale(U256::from(1_000_000_000_000_000_000u128), 18, 8).unwrap(),
            U256::from(100_000_000u64),
        );
    }

    #[test]
    fn rescale_down_truncates() {
        assert_eq!(
            Amounts::rescale(U256::from(1_999_999_999_999_999_999u128), 18, 0).unwrap(),
            U256::from(1u64),
        );
    }

    #[test]
    fn rescale_zero() {
        assert_eq!(Amounts::rescale(U256::ZERO, 0, 18).unwrap(), U256::ZERO);
    }

    #[test]
    fn kas_to_eth_basic() {
        let result = Amounts::amount_out(kas(1000), 0.15, KAS_DEC, 3000.0, ETH_DEC, 0.3).unwrap();
        let got = to_eth_f64(result);
        let expected = 1000.0 * 0.15 / 3000.0 * 0.997;
        assert!(
            (got - expected).abs() < 1e-8,
            "expected ~{expected}, got {got}"
        );
    }

    #[test]
    fn eth_to_kas_basic() {
        let result = Amounts::amount_out(eth(1), 3000.0, ETH_DEC, 0.15, KAS_DEC, 0.3).unwrap();
        let got = to_kas_f64(result);
        let expected = 3000.0 / 0.15 * 0.997;
        assert!(
            (got - expected).abs() < 1.0,
            "expected ~{expected}, got {got}"
        );
    }

    #[test]
    fn sub_dollar_both_sides() {
        let result = Amounts::amount_out(kas(500), 0.15, KAS_DEC, 0.50, ETH_DEC, 0.0).unwrap();
        let got = to_eth_f64(result);
        let expected = 500.0 * 0.15 / 0.50;
        assert!(
            (got - expected).abs() < 1e-6,
            "expected ~{expected}, got {got}"
        );
    }

    #[test]
    fn fractional_price_precision() {
        let result =
            Amounts::amount_out(kas(10_000), 0.153, KAS_DEC, 2000.50, ETH_DEC, 0.0).unwrap();
        let got = to_eth_f64(result);
        let expected = 10_000.0 * 0.153 / 2000.50;
        assert!(
            (got - expected).abs() < 1e-6,
            "expected ~{expected}, got {got}"
        );
    }

    #[test]
    fn zero_fee_exact_conversion() {
        let result = Amounts::amount_out(kas(20_000), 0.15, KAS_DEC, 3000.0, ETH_DEC, 0.0).unwrap();
        let got = to_eth_f64(result);
        assert!((got - 1.0).abs() < 1e-8, "expected ~1.0, got {got}");
    }

    #[test]
    fn higher_fee_lower_output() {
        let low = Amounts::amount_out(kas(1000), 0.15, KAS_DEC, 3000.0, ETH_DEC, 0.1).unwrap();
        let high = Amounts::amount_out(kas(1000), 0.15, KAS_DEC, 3000.0, ETH_DEC, 1.0).unwrap();
        assert!(low > high, "lower fee should give more: {low} vs {high}");
    }

    #[test]
    fn same_price_output_is_input_minus_fee() {
        let result = Amounts::amount_out(eth(1), 3000.0, ETH_DEC, 3000.0, ETH_DEC, 0.3).unwrap();
        let got = to_eth_f64(result);
        assert!((got - 0.997).abs() < 1e-8, "expected ~0.997, got {got}");
    }

    #[test]
    fn one_percent_fee() {
        let result = Amounts::amount_out(eth(10), 2000.0, ETH_DEC, 2000.0, ETH_DEC, 1.0).unwrap();
        let got = to_eth_f64(result);
        assert!((got - 9.9).abs() < 1e-6, "expected ~9.9, got {got}");
    }

    #[test]
    fn zero_input_returns_zero() {
        let result = Amounts::amount_out(U256::ZERO, 0.15, KAS_DEC, 3000.0, ETH_DEC, 0.3).unwrap();
        assert_eq!(result, U256::ZERO);
    }

    #[test]
    fn large_swap_no_overflow() {
        let result = Amounts::amount_out(kas(1_000_000_000), 0.15, KAS_DEC, 3000.0, ETH_DEC, 0.3);
        assert!(result.is_ok());
        assert!(result.unwrap() > U256::ZERO);
    }

    #[test]
    fn very_small_amount_does_not_error() {
        let result = Amounts::amount_out(U256::from(1u64), 0.15, KAS_DEC, 3000.0, ETH_DEC, 0.0);
        assert!(result.is_ok());
    }

    #[test]
    fn very_small_price() {
        let result =
            Amounts::amount_out(kas(1_000_000), 0.00001, KAS_DEC, 3000.0, ETH_DEC, 0.0).unwrap();
        let got = to_eth_f64(result);
        let expected = 1_000_000.0 * 0.00001 / 3000.0;
        assert!(
            (got - expected).abs() < 1e-6,
            "expected ~{expected}, got {got}"
        );
    }

    #[test]
    fn round_trip_loses_only_fees() {
        let original = kas(10_000);
        let eth_out = Amounts::amount_out(original, 0.15, KAS_DEC, 3000.0, ETH_DEC, 0.3).unwrap();
        let kas_back = Amounts::amount_out(eth_out, 3000.0, ETH_DEC, 0.15, KAS_DEC, 0.3).unwrap();

        let original_f = to_kas_f64(original);
        let returned_f = to_kas_f64(kas_back);
        let loss_pct = (1.0 - returned_f / original_f) * 100.0;

        assert!(
            loss_pct > 0.55 && loss_pct < 0.65,
            "round trip should lose ~0.6%, lost {loss_pct:.4}%"
        );
    }

    #[test]
    fn zero_destination_price_errors() {
        assert!(Amounts::amount_out(kas(100), 0.15, KAS_DEC, 0.0, ETH_DEC, 0.3).is_err());
    }

    #[test]
    fn negative_destination_price_errors() {
        assert!(Amounts::amount_out(kas(100), 0.15, KAS_DEC, -1.0, ETH_DEC, 0.3).is_err());
    }

    #[test]
    fn negative_source_price_errors() {
        assert!(Amounts::amount_out(kas(100), -0.15, KAS_DEC, 3000.0, ETH_DEC, 0.3).is_err());
    }

    #[test]
    fn same_decimals_18() {
        let result = Amounts::amount_out(eth(2), 1500.0, ETH_DEC, 3000.0, ETH_DEC, 0.0).unwrap();
        let got = to_eth_f64(result);
        assert!((got - 1.0).abs() < 1e-8, "expected ~1.0, got {got}");
    }

    #[test]
    fn same_decimals_8() {
        let result = Amounts::amount_out(kas(1000), 0.15, KAS_DEC, 0.30, KAS_DEC, 0.0).unwrap();
        let got = to_kas_f64(result);
        assert!((got - 500.0).abs() < 1e-4, "expected ~500, got {got}");
    }
}

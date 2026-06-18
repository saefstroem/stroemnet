use alloy_primitives::U256;

use crate::error::AmountError;
use crate::result::Result;

/// Utility struct for amount calculations and conversions.
pub struct Amounts;

impl Amounts {
    /// Stroemnets internal decimals that is used for all amounts and representations.
    pub const PRICE_DECIMALS: u8 = 8;

    /// Computes amount out given a number of parameters.
    /// This is used across all chains to compute the expected output amount for a swap,
    /// given the input amount, source and destination prices and decimals, and the spread percentage.
    ///
    /// This was grouped to ensure that all chains and all callers compute the exact same amount in order
    /// to avoid discrepancies that could lead to failed swaps or user confusion.
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

        // If the destination price is 0 there is no need to compute anything
        if destination_usd_price <= 0.0 {
            return Err(AmountError::InvalidPriceData(destination_usd_price));
        }

        // If the source price is negative, it's also invalid data
        if source_usd_price < 0.0 {
            return Err(AmountError::InvalidPriceData(source_usd_price));
        }

        // We only validate valid spread percentages.
        if !(0.0..100.0).contains(&spread_percent) {
            return Err(AmountError::InvalidPriceData(spread_percent));
        }

        // Compute the price-scaled amount in source decimals,
        let price_scale = 10f64.powi(Self::PRICE_DECIMALS as i32);

        // Convert both prices to the fixedpoint representation with
        // our internal number of decimals.
        let source_price_fixed = U256::from((source_usd_price * price_scale).round() as u128);
        let dest_price_fixed = U256::from((destination_usd_price * price_scale).round() as u128);

        // It is in source decimals because we inherit from amount_in which is in source decimals
        let output_in_source_decimals = amount_in
            .checked_mul(source_price_fixed)
            .ok_or(AmountError::ArithmeticOverflow(amount_in))?
            .checked_div(dest_price_fixed)
            .ok_or(AmountError::DivisionByZero(dest_price_fixed))?;

        // Compute the spread basis points and compute the multiplier that
        // we apply to the output amount. We use basis points here
        // since we are dealing with non-float u256.
        let spread_bps = (spread_percent * 100.0) as u32;
        let spread_multiplier = U256::from(10_000u32.saturating_sub(spread_bps));
        let spread_divisor = U256::from(10_000u32);

        // Apply the spread to the output amount.
        // Then divide by 100 bps to get the final amount after spread.
        let output_after_spread = output_in_source_decimals
            .checked_mul(spread_multiplier)
            .ok_or(AmountError::ArithmeticOverflow(output_in_source_decimals))?
            .checked_div(spread_divisor)
            .ok_or(AmountError::DivisionByZero(spread_divisor))?;

        // Finally, rescale the output amount from source decimals to destination decimals.
        Self::rescale(output_after_spread, source_decimals, destination_decimals)
    }

    /// A universal rescaling function that can be used to convert amounts between different decimal representations.
    pub fn rescale(amount: U256, from_decimals: u8, to_decimals: u8) -> Result<U256> {
        // Compute the difference in decimals.
        let diff = to_decimals as i32 - from_decimals as i32;

        // If the difference is larger than 0 we need to multiply by 10^(diff) to get to the new decimals.
        if diff > 0 {
            amount
                .checked_mul(U256::from(10u64).pow(U256::from(diff as u64)))
                .ok_or(AmountError::AmountOverflow(amount))
        } else if diff < 0 {
            // If the difference is smaller than 0 we need to divide by 10^(-diff) to get to the new decimals.
            amount
                .checked_div(U256::from(10u64).pow(U256::from((-diff) as u64)))
                .ok_or(AmountError::AmountUnderflow(amount))
        } else {
            // If the difference is 0, we can return the same amount since it's already in the correct decimals.
            Ok(amount)
        }
    }

    /// Rescales the amount and formats it as a string with the correct number of decimals,
    /// applying a ceiling-like behavior to ensure that we don't under-represent small amounts when displaying to users.
    /// I.e. if we display only 8 decimals but the actual amount has 18 decimals, we want to ensure
    /// we over-represent the amount rather than under-representing it, to avoid insufficient deposits or failed swaps
    pub fn rescale_display_like_ceil(
        amount: U256,
        from_decimals: u8,
        to_decimals: u8,
    ) -> Result<String> {
        let rescaled = Self::rescale_ceil(amount, from_decimals, to_decimals)?;
        Self::format_fixed_point(rescaled, to_decimals)
    }

    /// Similar to rescale but applies a ceiling-like behavior when scaling down, to avoid under-representing small amounts.
    fn rescale_ceil(amount: U256, from_decimals: u8, to_decimals: u8) -> Result<U256> {
        // Compute the difference in decimals.
        let diff = to_decimals as i32 - from_decimals as i32;
        if diff >= 0 {
            // If we are scaling up or keeping the same decimals,
            // we can just rescale normally since there is no risk of under-representation.
            return Self::rescale(amount, from_decimals, to_decimals);
        }

        // If we are scaling down, we need to apply the ceiling behavior.
        let divisor = U256::from(10u64).pow(U256::from((-diff) as u64));

        // Compute the quotient and remainder to determine if we need to apply the ceiling.
        let q = amount
            .checked_div(divisor)
            .ok_or(AmountError::DivisionByZero(divisor))?;

        // If there is a remainder, we need to add 1 to the quotient to apply the ceiling.
        let r = amount % divisor;
        if r > U256::ZERO {
            // Bump up the quotient by 1 to apply the ceiling, but check for overflow first.
            q.checked_add(U256::from(1u64))
                .ok_or(AmountError::AmountOverflow(q))
        } else {
            // If there is no remainder, we can return the quotient as is.
            Ok(q)
        }
    }

    /// Formats a U256 amount as a fixed-point decimal string with the given number of decimals.
    fn format_fixed_point(amount: U256, decimals: u8) -> Result<String> {
        if decimals == 0 {
            // If there are no decimals, we can just return the amount as a string.
            return Ok(amount.to_string());
        }

        // Compute the divisor for the decimals to split the whole and fractional parts.
        let divisor = U256::from(10u64).pow(U256::from(decimals as u64));

        // Compute the whole amounts
        let whole = amount / divisor;

        // Compute fracntional amount.
        let frac = amount % divisor;

        // Format the fractional part as a string.
        let frac_str = frac.to_string();

        // Left-pad the fractional string with zeros to ensure it has the correct number of decimal places.
        // and replace some of it with the frac_str.
        let padded = format!("{frac_str:0>width$}", width = decimals as usize);

        // Return the formatted string in the form "whole.fractional".
        Ok(format!("{whole}.{padded}"))
    }
}

#[cfg(test)]
mod tests {
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
    fn rescale_display_like_ceil_bumps_subunit_to_one() {
        assert_eq!(
            Amounts::rescale_display_like_ceil(U256::from(1u64), 18, 8).unwrap(),
            "0.00000001",
        );
    }

    #[test]
    fn rescale_display_like_ceil_exact_value_no_bump() {
        assert_eq!(
            Amounts::rescale_display_like_ceil(U256::from(10_000_000_000u64), 18, 8).unwrap(),
            "0.00000001",
        );
    }
    #[test]
    fn rescale_display_like_ceil_zero_target_decimals() {
        assert_eq!(
            Amounts::rescale_display_like_ceil(U256::from(1_500_000_000_000_000_000u128), 18, 0)
                .unwrap(),
            "2",
        );
    }

    #[test]
    fn rescale_display_like_ceil_zero_input() {
        assert_eq!(
            Amounts::rescale_display_like_ceil(U256::ZERO, 18, 8).unwrap(),
            "0.00000000",
        );
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

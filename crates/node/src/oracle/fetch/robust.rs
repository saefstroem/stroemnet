use super::PriceSample;

const MAX_DEVIATION: f64 = 0.10;
#[cfg(not(target_arch = "wasm32"))]
const MIN_SOURCES: usize = 2;
#[cfg(target_arch = "wasm32")]
const MIN_SOURCES: usize = 1;

fn weighted_price(samples: &[PriceSample]) -> f64 {
    let total_volume: f64 = samples.iter().map(|s| s.volume_usd).sum();
    if total_volume > 0.0 {
        samples.iter().map(|s| s.price * s.volume_usd).sum::<f64>() / total_volume
    } else {
        samples.iter().map(|s| s.price).sum::<f64>() / samples.len() as f64
    }
}

fn median(prices: &mut [f64]) -> Option<f64> {
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = prices.len();
    match n % 2 {
        1 => prices.get(n / 2).copied(),
        _ => match (prices.get(n / 2 - 1), prices.get(n / 2)) {
            (Some(&lo), Some(&hi)) => Some((lo + hi) / 2.0),
            _ => None,
        },
    }
}

pub(super) fn robust_price(samples: &[PriceSample]) -> Option<f64> {
    let valid: Vec<PriceSample> = samples
        .iter()
        .filter(|s| {
            s.price.is_finite() && s.price > 0.0 && s.volume_usd.is_finite() && s.volume_usd >= 0.0
        })
        .copied()
        .collect();
    if valid.len() < MIN_SOURCES {
        return None;
    }
    let mut prices: Vec<f64> = valid.iter().map(|s| s.price).collect();
    let med = median(&mut prices)?;
    if med <= 0.0 {
        return None;
    }
    let kept: Vec<PriceSample> = valid
        .into_iter()
        .filter(|s| ((s.price - med) / med).abs() <= MAX_DEVIATION)
        .collect();
    if kept.len() < MIN_SOURCES {
        return None;
    }
    let p = weighted_price(&kept);
    if p.is_finite() && p > 0.0 {
        Some(p)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn sample(price: f64, volume_usd: f64) -> PriceSample {
        PriceSample { price, volume_usd }
    }

    #[test]
    fn weighted_price_is_volume_weighted() {
        let samples = [sample(100.0, 30.0), sample(200.0, 10.0)];
        assert!((weighted_price(&samples) - 125.0).abs() < 1e-9);
    }

    #[test]
    fn weighted_price_falls_back_to_mean_when_no_volume() {
        let samples = [sample(100.0, 0.0), sample(200.0, 0.0)];
        assert!((weighted_price(&samples) - 150.0).abs() < 1e-9);
    }

    #[test]
    fn robust_price_requires_two_agreeing_sources() {
        assert!(robust_price(&[sample(100.0, 10.0)]).is_none());
        let p = robust_price(&[sample(100.0, 10.0), sample(102.0, 10.0)]).unwrap();
        assert!((p - 101.0).abs() < 1e-9);
    }

    #[test]
    fn robust_price_drops_outliers_and_nonfinite() {
        let samples = [
            sample(100.0, 10.0),
            sample(101.0, 10.0),
            sample(500.0, 10.0),
            sample(f64::INFINITY, 10.0),
        ];
        let p = robust_price(&samples).unwrap();
        assert!((p - 100.5).abs() < 1e-9);
    }
}

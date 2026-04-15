use anyhow::Result;
use polars::prelude::*;

/// Calculates weights for fractional differentiation
/// w_k = -w_{k-1} * (d - k + 1) / k
fn get_weights(d: f64, size: usize, threshold: f64) -> Vec<f64> {
    let mut weights = vec![1.0];
    let mut k = 1;

    loop {
        let w_prev = *weights.last().unwrap();
        let w_new = -w_prev * (d - k as f64 + 1.0) / k as f64;

        if k >= size || w_new.abs() < threshold {
            break;
        }

        weights.push(w_new);
        k += 1;
    }
    weights.into_iter().rev().collect()
}

/// Applies Fixed-Window Fractional Differentiation to a Series
pub fn frac_diff_ffd(series: &Series, d: f64, window_size: usize) -> Result<Series> {
    let ca = series.f64()?;
    let data: Vec<f64> = (0..ca.len())
        .map(|i| ca.get(i).unwrap_or(f64::NAN))
        .collect();
    let weights = get_weights(d, window_size, 1e-5);
    let w_len = weights.len();

    let mut output = vec![f64::NAN; data.len()];

    for i in (w_len - 1)..data.len() {
        let mut val = 0.0;
        for (j, &w) in weights.iter().enumerate() {
            val += w * data[i - (w_len - 1) + j];
        }
        output[i] = val;
    }

    Ok(Series::new("frac_diff", output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_weights_d_zero() {
        // d=0 should give identity (weight = 1.0)
        let weights = get_weights(0.0, 10, 1e-5);
        assert_eq!(weights.len(), 1);
        assert!((weights[0] - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_get_weights_d_one() {
        // d=1 should give first difference weights
        let weights = get_weights(1.0, 10, 1e-5);
        // For d=1: w=[1, -1]
        assert!(weights.contains(&1.0));
        assert!(weights.contains(&-1.0));
    }

    #[test]
    fn test_get_weights_size_limit() {
        // Should respect size limit
        let weights = get_weights(0.5, 5, 1e-10);
        assert!(weights.len() <= 5);
    }

    #[test]
    fn test_get_weights_threshold() {
        // Small weights should be dropped below threshold
        let weights = get_weights(0.1, 1000, 0.1);
        // With d=0.1 and threshold 0.1, should stop early
        assert!(weights.len() < 100);
    }

    #[test]
    fn test_frac_diff_constant_series() {
        // For d=1.0 (full differencing), constant series gives zero (first difference = 0).
        // For fractional d (e.g. 0.5), constant series produces non-zero because weights
        // don't sum to zero — output = value * sum(weights). We test d=1.0 here.
        let data = Series::new("data", vec![100.0; 50]);
        let result = frac_diff_ffd(&data, 1.0, 20).unwrap();
        let values = result.f64().unwrap();

        // After warmup (window_size=20), output should be near zero
        for i in 30..50 {
            let val = values.get(i).unwrap();
            assert!(
                val.abs() < 1e-6,
                "Expected near zero at index {}, got {}",
                i,
                val
            );
        }
    }

    #[test]
    fn test_frac_diff_linear_trend() {
        // Linear trend: [1, 2, 3, 4, ...]
        let data: Vec<f64> = (1..=50).map(|x| x as f64).collect();
        let series = Series::new("data", data);
        let result = frac_diff_ffd(&series, 1.0, 10).unwrap();
        let values = result.f64().unwrap();

        // d=1 should give first difference, which is constant (=1)
        for i in 15..45 {
            let val = values.get(i).unwrap();
            assert!(
                (val - 1.0).abs() < 0.01,
                "Expected ~1.0 at index {}, got {}",
                i,
                val
            );
        }
    }

    #[test]
    fn test_frac_diff_preserves_length() {
        let data = Series::new("data", (0..100).map(|x| x as f64).collect::<Vec<_>>());
        let result = frac_diff_ffd(&data, 0.4, 20).unwrap();
        assert_eq!(result.len(), 100);
    }

    #[test]
    fn test_frac_diff_warmup_nans() {
        // Early values should be NaN (warmup period)
        let data = Series::new("data", (0..50).map(|x| x as f64).collect::<Vec<_>>());
        let result = frac_diff_ffd(&data, 0.5, 20).unwrap();
        let values = result.f64().unwrap();

        // First few values should be NaN
        assert!(values.get(0).unwrap().is_nan());
        assert!(values.get(5).unwrap().is_nan());
    }

    #[test]
    fn test_frac_diff_d_value_impact() {
        // Different d values should give different results
        let data = Series::new(
            "data",
            (0..100).map(|x| (x as f64).sin()).collect::<Vec<_>>(),
        );

        let result_03 = frac_diff_ffd(&data, 0.3, 20).unwrap();
        let result_07 = frac_diff_ffd(&data, 0.7, 20).unwrap();

        let vals_03 = result_03.f64().unwrap();
        let vals_07 = result_07.f64().unwrap();

        // Results should differ at some point
        let mut different = false;
        for i in 50..80 {
            let v1 = vals_03.get(i).unwrap();
            let v2 = vals_07.get(i).unwrap();
            if (v1 - v2).abs() > 0.01 {
                different = true;
                break;
            }
        }
        assert!(
            different,
            "Different d values should produce different results"
        );
    }
}

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

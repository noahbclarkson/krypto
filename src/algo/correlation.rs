//! Correlation filtering for multi-asset portfolios.
//!
//! Prevents simultaneous entries on highly correlated assets (e.g., BTC + ETH).
//! When multiple assets have the same signal, only the highest-conviction one is kept.

use anyhow::Result;
use polars::prelude::*;

/// Simple correlation filter for portfolio position management.
///
/// Given a set of current signals and a correlation matrix, filters out
/// positions that would create excessive correlation risk.
pub struct CorrelationFilter {
    /// Minimum correlation threshold to consider assets "correlated"
    threshold: f64,
    /// Maximum number of correlated positions allowed (reserved for future use)
    #[allow(dead_code)]
    max_correlated: usize,
}

impl CorrelationFilter {
    pub fn new(threshold: f64, max_correlated: usize) -> Self {
        Self {
            threshold,
            max_correlated,
        }
    }

    /// Filter signals to reduce correlation risk.
    ///
    /// # Arguments
    /// * `signals` - Map of symbol → signal value (-1.0 to 1.0)
    /// * `correlations` - Correlation matrix as HashMap<(symbol, symbol), f64>
    ///
    /// # Returns
    /// Filtered signals with reduced correlation
    pub fn filter(
        &self,
        signals: &std::collections::HashMap<String, f64>,
        correlations: &std::collections::HashMap<(String, String), f64>,
    ) -> std::collections::HashMap<String, f64> {
        let mut filtered = signals.clone();
        let symbols: Vec<&String> = signals.keys().collect();

        // For each pair of correlated assets with same-direction signals,
        // keep only the one with stronger conviction
        for i in 0..symbols.len() {
            for j in (i + 1)..symbols.len() {
                let sym_i = symbols[i];
                let sym_j = symbols[j];

                let sig_i = *filtered.get(sym_i).unwrap_or(&0.0);
                let sig_j = *filtered.get(sym_j).unwrap_or(&0.0);

                // Skip if either already filtered out
                if sig_i.abs() < 0.01 || sig_j.abs() < 0.01 {
                    continue;
                }

                // Skip if signals are in opposite directions
                if sig_i * sig_j < 0.0 {
                    continue;
                }

                // Check correlation
                let corr = correlations
                    .get(&(sym_i.clone(), sym_j.clone()))
                    .or_else(|| correlations.get(&(sym_j.clone(), sym_i.clone())))
                    .copied()
                    .unwrap_or(0.0);

                if corr.abs() >= self.threshold {
                    // Same-direction signals on correlated assets
                    // Keep only the one with stronger conviction
                    if sig_i.abs() >= sig_j.abs() {
                        filtered.insert(sym_j.clone(), 0.0);
                    } else {
                        filtered.insert(sym_i.clone(), 0.0);
                    }
                }
            }
        }

        filtered
    }

    /// Compute rolling correlation from price data.
    ///
    /// # Arguments
    /// * `df1` - First asset's OHLCV data with "close" column
    /// * `df2` - Second asset's OHLCV data with "close" column
    /// * `window` - Rolling window size
    ///
    /// # Returns
    /// Series of rolling correlation values
    pub fn rolling_correlation(df1: &DataFrame, df2: &DataFrame, window: usize) -> Result<Series> {
        let close1 = df1.column("close")?.f64()?;
        let close2 = df2.column("close")?.f64()?;

        let len = close1.len().min(close2.len());
        let mut correlations = Vec::with_capacity(len);

        for i in 0..len {
            if i < window - 1 {
                correlations.push(None);
                continue;
            }

            let start = i - window + 1;
            let slice1: Vec<f64> = (start..=i).filter_map(|k| close1.get(k)).collect();
            let slice2: Vec<f64> = (start..=i).filter_map(|k| close2.get(k)).collect();

            if slice1.len() == window && slice2.len() == window {
                let corr = Self::pearson(&slice1, &slice2);
                correlations.push(Some(corr));
            } else {
                correlations.push(None);
            }
        }

        Ok(Series::new("correlation", correlations))
    }

    /// Pearson correlation coefficient
    fn pearson(x: &[f64], y: &[f64]) -> f64 {
        let n = x.len() as f64;
        if n == 0.0 {
            return 0.0;
        }

        let sum_x: f64 = x.iter().sum();
        let sum_y: f64 = y.iter().sum();
        let sum_xy: f64 = x.iter().zip(y.iter()).map(|(a, b)| a * b).sum();
        let sum_x2: f64 = x.iter().map(|a| a * a).sum();
        let sum_y2: f64 = y.iter().map(|b| b * b).sum();

        let numerator = n * sum_xy - sum_x * sum_y;
        let denominator = ((n * sum_x2 - sum_x * sum_x) * (n * sum_y2 - sum_y * sum_y)).sqrt();

        if denominator == 0.0 {
            0.0
        } else {
            numerator / denominator
        }
    }
}

impl Default for CorrelationFilter {
    fn default() -> Self {
        Self::new(0.7, 1) // 70% correlation threshold, max 1 correlated position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_filter_removes_correlated_same_direction() {
        let filter = CorrelationFilter::new(0.7, 1);

        let mut signals = HashMap::new();
        signals.insert("BTC".to_string(), 1.0);
        signals.insert("ETH".to_string(), 0.8); // Same direction, lower conviction

        let mut correlations = HashMap::new();
        correlations.insert(("BTC".to_string(), "ETH".to_string()), 0.85);

        let filtered = filter.filter(&signals, &correlations);

        assert_eq!(*filtered.get("BTC").unwrap_or(&0.0), 1.0); // Kept (stronger)
        assert_eq!(*filtered.get("ETH").unwrap_or(&0.0), 0.0); // Filtered out
    }

    #[test]
    fn test_filter_keeps_opposite_direction() {
        let filter = CorrelationFilter::new(0.7, 1);

        let mut signals = HashMap::new();
        signals.insert("BTC".to_string(), 1.0); // Long
        signals.insert("ETH".to_string(), -1.0); // Short (opposite direction)

        let mut correlations = HashMap::new();
        correlations.insert(("BTC".to_string(), "ETH".to_string()), 0.85);

        let filtered = filter.filter(&signals, &correlations);

        // Both kept because they're opposite directions
        assert_eq!(*filtered.get("BTC").unwrap_or(&0.0), 1.0);
        assert_eq!(*filtered.get("ETH").unwrap_or(&0.0), -1.0);
    }

    #[test]
    fn test_filter_keeps_uncorrelated() {
        let filter = CorrelationFilter::new(0.7, 1);

        let mut signals = HashMap::new();
        signals.insert("BTC".to_string(), 1.0);
        signals.insert("DOGE".to_string(), 1.0);

        let mut correlations = HashMap::new();
        correlations.insert(("BTC".to_string(), "DOGE".to_string()), 0.5); // Below threshold

        let filtered = filter.filter(&signals, &correlations);

        // Both kept because correlation is below threshold
        assert_eq!(*filtered.get("BTC").unwrap_or(&0.0), 1.0);
        assert_eq!(*filtered.get("DOGE").unwrap_or(&0.0), 1.0);
    }

    #[test]
    fn test_pearson_correlation() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![2.0, 4.0, 6.0, 8.0, 10.0]; // Perfect positive correlation

        let corr = CorrelationFilter::pearson(&x, &y);
        assert!((corr - 1.0).abs() < 0.0001);

        let z = vec![-1.0, -2.0, -3.0, -4.0, -5.0]; // Perfect negative correlation
        let corr_neg = CorrelationFilter::pearson(&x, &z);
        assert!((corr_neg - (-1.0)).abs() < 0.0001);
    }
}

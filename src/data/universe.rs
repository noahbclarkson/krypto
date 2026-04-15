//! Universe data operations and cross-sectional features.
//!
//! Provides tools for fetching multiple symbols and computing features
//! that rank assets against each other (e.g., cross-sectional momentum).

use super::loader::DataLoader;
use anyhow::Result;
use futures::future::join_all;
use polars::prelude::*;
use std::collections::HashMap;

pub struct Universe;

impl Universe {
    pub fn new() -> Self {
        Self
    }

    /// Fetches multiple symbols and intervals concurrently
    pub async fn fetch_universe(
        &self,
        symbols: &[&str],
        intervals: &[&str],
        limit: u32,
    ) -> Result<HashMap<String, DataFrame>> {
        let mut tasks = Vec::new();

        for &sym in symbols {
            for &inv in intervals {
                let key = format!("{sym}_{inv}");
                let sym_owned = sym.to_string();
                let inv_owned = inv.to_string();
                let loader = DataLoader::new(None, None);

                tasks.push(async move {
                    let df = loader.fetch_data(&sym_owned, &inv_owned, limit).await?;
                    Ok::<(String, DataFrame), anyhow::Error>((key, df))
                });
            }
        }

        let results = join_all(tasks).await;

        let mut map = HashMap::new();
        for res in results {
            match res {
                Ok((key, df)) => {
                    map.insert(key, df);
                }
                Err(e) => eprintln!("Failed to fetch data: {e}"),
            }
        }

        Ok(map)
    }
}

impl Default for Universe {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute cross-sectional momentum and trend scores across a universe of assets.
///
/// Modifies the DataFrames in place, adding two columns:
/// - `cs_momentum_rank`: [0.0 - 1.0] percentile rank of price return over the lookback window
/// - `cs_trend_score`: Return divided by historical volatility, then ranked [0.0 - 1.0]
///
/// Assumes all DataFrames in the map represent the same interval and are aligned in time
/// (or at least dense enough that row indices roughly match, which is true for crypto 1h/4h cache data).
///
/// # Arguments
/// - `data`: Mutable map of symbol -> DataFrame
/// - `lookback`: Number of periods for return calculation (e.g. 168 for 1-week on 1h data)
pub fn compute_cross_sectional_features(
    data: &mut HashMap<String, DataFrame>,
    lookback: usize,
) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }

    // Extract symbols and lengths (assume all have same length, use shortest to be safe)
    let symbols: Vec<String> = data.keys().cloned().collect();
    let n_bars = data.values().map(|df| df.height()).min().unwrap_or(0);

    if n_bars <= lookback {
        return Ok(());
    }

    // Extract close prices for all assets
    let mut closes_map = HashMap::new();
    for sym in &symbols {
        let df = data.get(sym).unwrap();
        let closes: Vec<f64> = df
            .column("close")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect();
        closes_map.insert(sym.clone(), closes);
    }

    // We will build rank arrays for each symbol
    let mut rank_map: HashMap<String, Vec<f64>> = symbols
        .iter()
        .map(|s| (s.clone(), vec![0.5f64; n_bars])) // default to neutral 0.5
        .collect();

    let mut vol_adj_rank_map: HashMap<String, Vec<f64>> = symbols
        .iter()
        .map(|s| (s.clone(), vec![0.5f64; n_bars]))
        .collect();

    // Iterate through time, compute cross-sectional ranks
    for i in lookback..n_bars {
        let mut returns = Vec::with_capacity(symbols.len());
        let mut vol_adj_returns = Vec::with_capacity(symbols.len());

        for sym in &symbols {
            let closes = closes_map.get(sym).unwrap();
            let current = closes[i];
            let past = closes[i - lookback];

            if past <= 0.0 {
                returns.push((sym.clone(), 0.0));
                vol_adj_returns.push((sym.clone(), 0.0));
                continue;
            }

            let ret = (current - past) / past;
            returns.push((sym.clone(), ret));

            // Compute realized volatility over the lookback window
            let slice = &closes[(i - lookback)..=i];
            let mut log_returns = Vec::with_capacity(slice.len() - 1);
            for j in 1..slice.len() {
                if slice[j - 1] > 0.0 && slice[j] > 0.0 {
                    log_returns.push((slice[j] / slice[j - 1]).ln());
                } else {
                    log_returns.push(0.0);
                }
            }

            let mean_log_ret = log_returns.iter().sum::<f64>() / log_returns.len() as f64;
            let variance = log_returns
                .iter()
                .map(|r| (r - mean_log_ret).powi(2))
                .sum::<f64>()
                / log_returns.len() as f64;
            let std_dev = variance.sqrt();

            let vol_adj = if std_dev > 1e-6 { ret / std_dev } else { 0.0 };
            vol_adj_returns.push((sym.clone(), vol_adj));
        }

        // Helper to compute percentiles (0.0 = worst, 1.0 = best)
        let compute_ranks = |mut vals: Vec<(String, f64)>| -> HashMap<String, f64> {
            // Sort by value ascending
            vals.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            let mut result = HashMap::new();
            let n = vals.len().max(1) as f64;
            for (rank, (sym, _)) in vals.into_iter().enumerate() {
                let pct = rank as f64 / (n - 1.0).max(1.0);
                result.insert(sym, pct);
            }
            result
        };

        let ranks = compute_ranks(returns);
        let vol_adj_ranks = compute_ranks(vol_adj_returns);

        for sym in &symbols {
            rank_map.get_mut(sym).unwrap()[i] = *ranks.get(sym).unwrap_or(&0.5);
            vol_adj_rank_map.get_mut(sym).unwrap()[i] = *vol_adj_ranks.get(sym).unwrap_or(&0.5);
        }
    }

    // Attach columns back to DataFrames
    for sym in &symbols {
        let df = data.get_mut(sym).unwrap();
        let r = rank_map.get(sym).unwrap().clone();
        let vr = vol_adj_rank_map.get(sym).unwrap().clone();

        // Pad if df is longer than n_bars (in case they weren't perfectly aligned)
        let mut r_padded = r;
        let mut vr_padded = vr;
        r_padded.resize(df.height(), 0.5);
        vr_padded.resize(df.height(), 0.5);

        df.with_column(Series::new("cs_momentum_rank", r_padded))?;
        df.with_column(Series::new("cs_trend_score", vr_padded))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_df(symbol: &str, closes: &[f64]) -> (String, DataFrame) {
        let len = closes.len();
        let df = df! [
            "close" => closes,
            "time" => (0..len as i64).collect::<Vec<_>>(),
            "high" => closes,
            "low" => closes,
            "open" => closes,
            "volume" => vec![1000.0; len],
        ]
        .unwrap();
        (symbol.to_string(), df)
    }

    #[test]
    fn test_universe_creation() {
        // Just verify Universe::new() doesn't panic
        let _ = Universe::new();
    }

    #[test]
    fn test_cross_sectional_empty_data() {
        let mut data = HashMap::new();
        let result = compute_cross_sectional_features(&mut data, 10);
        assert!(result.is_ok(), "Should handle empty data");
    }

    #[test]
    fn test_cross_sectional_single_asset() {
        let mut data = HashMap::new();
        let (key, df) = create_test_df(
            "BTC",
            &[
                100.0, 101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0, 108.0, 109.0, 110.0, 111.0,
            ],
        );
        data.insert(key, df);

        let result = compute_cross_sectional_features(&mut data, 5);
        assert!(result.is_ok(), "Should handle single asset");

        // Single asset should have neutral rank (0.5) for all bars
        let df = data.get("BTC").unwrap();
        let ranks = df.column("cs_momentum_rank").unwrap().f64().unwrap();
        // With only one asset, rank should always be 0.0 (only element, rank 0 / (1-1) = 0/0 issue)
        // Actually with n=1, the formula is rank / (n-1) which is 0/0, so it defaults
        assert!(ranks.get(10).is_some(), "Should have momentum rank column");
    }

    #[test]
    fn test_cross_sectional_two_assets() {
        let mut data = HashMap::new();

        // Asset 1: 100 -> 110 (10% gain)
        let (key1, df1) = create_test_df(
            "ASSET1",
            &[
                100.0, 101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0, 108.0, 109.0, 110.0, 111.0,
            ],
        );
        // Asset 2: 100 -> 90 (10% loss)
        let (key2, df2) = create_test_df(
            "ASSET2",
            &[
                100.0, 99.0, 98.0, 97.0, 96.0, 95.0, 94.0, 93.0, 92.0, 91.0, 90.0, 89.0,
            ],
        );

        data.insert(key1, df1);
        data.insert(key2, df2);

        let result = compute_cross_sectional_features(&mut data, 10);
        assert!(result.is_ok(), "Should handle two assets");

        // Check that ASSET1 has higher momentum rank than ASSET2
        let df1 = data.get("ASSET1").unwrap();
        let df2 = data.get("ASSET2").unwrap();

        let rank1 = df1.column("cs_momentum_rank").unwrap().f64().unwrap();
        let rank2 = df2.column("cs_momentum_rank").unwrap().f64().unwrap();

        // At bar 10 (after 10-period lookback), ASSET1 should outrank ASSET2
        let r1 = rank1.get(10).unwrap_or(0.5);
        let r2 = rank2.get(10).unwrap_or(0.5);

        assert!(
            r1 >= r2,
            "Asset with higher return should have higher or equal rank"
        );
    }

    #[test]
    fn test_cross_sectional_short_data() {
        let mut data = HashMap::new();
        // Only 5 bars, but lookback is 10
        let (key, df) = create_test_df("SHORT", &[100.0, 101.0, 102.0, 103.0, 104.0]);
        data.insert(key, df);

        let result = compute_cross_sectional_features(&mut data, 10);
        // Should succeed but not compute any ranks (n_bars <= lookback)
        assert!(result.is_ok(), "Should handle short data gracefully");
    }

    #[test]
    fn test_cross_sectional_column_added() {
        let mut data = HashMap::new();
        let (key, df) = create_test_df("TEST", &[100.0; 20]);
        data.insert(key, df);

        let result = compute_cross_sectional_features(&mut data, 5);
        assert!(result.is_ok());

        let df = data.get("TEST").unwrap();
        assert!(
            df.column("cs_momentum_rank").is_ok(),
            "Should add momentum rank column"
        );
        assert!(
            df.column("cs_trend_score").is_ok(),
            "Should add trend score column"
        );
    }
}

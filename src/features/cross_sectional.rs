//! Cross-Sectional Feature Engineering
//!
//! Computes relative rank features across a universe of assets.
//! These features are injected as columns into each asset's DataFrame,
//! allowing the existing single-asset strategy interface to exploit
//! cross-asset information.
//!
//! # How it works
//! 1. For each bar `t`, compute the N-period return for every asset.
//! 2. Rank all assets by their return (0.0 = worst, 1.0 = best).
//! 3. Inject the rank as a `cs_momentum_rank` column into each asset's DataFrame.
//! 4. A `CrossSectionalMomentum` strategy then thresholds on this rank.
//!
//! # Example
//! ```ignore
//! use krypto::features::cross_sectional::compute_cs_features;
//! use std::collections::HashMap;
//!
//! let mut dfs: HashMap<String, DataFrame> = /* load multiple assets */ todo!();
//! let enriched = compute_cs_features(&dfs, 20)?;
//! // Each enriched DataFrame now has cs_momentum_rank, cs_vol_rank, cs_trend_score columns
//! ```

use anyhow::{bail, Result};
use polars::prelude::*;
use std::collections::HashMap;

/// Computes cross-sectional rank features across a universe of assets.
///
/// Injects the following columns into each asset's DataFrame:
/// - `cs_momentum_rank` (f64, 0.0–1.0): rank by N-period return (1.0 = strongest momentum)
/// - `cs_vol_rank` (f64, 0.0–1.0): rank by N-period volatility (1.0 = most volatile)
/// - `cs_trend_score` (f64, ~-0.65–0.65): combined momentum minus volatility signal
///
/// # Parameters
/// - `dfs`: map from symbol name to DataFrame (must all have `close` column)
/// - `momentum_period`: number of bars for return computation
///
/// Returns a new map with enriched DataFrames. Assets without enough history
/// are returned with rank columns set to 0.5 (neutral).
pub fn compute_cs_features(
    dfs: &HashMap<String, DataFrame>,
    momentum_period: usize,
) -> Result<HashMap<String, DataFrame>> {
    if dfs.is_empty() {
        bail!("No DataFrames provided for cross-sectional feature computation");
    }

    let symbols: Vec<&String> = dfs.keys().collect();
    let n_assets = symbols.len();

    // Find the minimum length across all DataFrames
    let min_len = dfs.values().map(|df| df.height()).min().unwrap_or(0);
    if min_len < momentum_period + 1 {
        bail!(
            "DataFrames too short: min length {} < momentum_period {} + 1",
            min_len,
            momentum_period
        );
    }

    // Extract close prices for all assets
    let closes: HashMap<&String, Vec<f64>> = dfs
        .iter()
        .map(|(sym, df)| {
            let c: Vec<f64> = df
                .column("close")
                .map(|s| s.f64().map(|ca| ca.into_iter().map(|v| v.unwrap_or(0.0)).collect()).unwrap_or_default())
                .unwrap_or_default();
            (sym, c)
        })
        .collect();

    // For each asset, determine its length (may differ)
    // Note: lengths not used since we crop all assets to min_len for fair cross-sectional ranking
    let _lengths: HashMap<&String, usize> = dfs.iter().map(|(s, df)| (s, df.height())).collect();

    // Build rank arrays per asset (indexed at the end of each asset's timeline)
    // Strategy: for each bar position in the longest series, compute cross-sectional ranks
    // For assets shorter than the longest, we use their own bar index mapped to the same time.
    // 
    // Simplified: assume all DataFrames are time-aligned (same bars). Crop to min_len.
    let use_len = min_len; // use the minimum common length for ranking

    // Pre-compute N-period returns for all assets at all bar positions
    let mut returns: HashMap<&String, Vec<f64>> = HashMap::new();
    let mut vols: HashMap<&String, Vec<f64>> = HashMap::new();

    for sym in &symbols {
        let c = &closes[sym];
        let n = c.len().min(use_len);
        let mut ret = vec![0.0f64; n];
        let mut vol = vec![0.0f64; n];

        for i in momentum_period..n {
            let past = c[i - momentum_period];
            if past > 0.0 {
                ret[i] = (c[i] - past) / past;
            }

            // Realized volatility: std of log returns over window
            let log_rets: Vec<f64> = (i.saturating_sub(momentum_period)..i)
                .filter(|&j| c[j] > 0.0 && c[j + 1] > 0.0)
                .map(|j| (c[j + 1] / c[j]).ln())
                .collect();
            if log_rets.len() > 1 {
                let mean = log_rets.iter().sum::<f64>() / log_rets.len() as f64;
                let variance = log_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
                    / (log_rets.len() - 1) as f64;
                vol[i] = variance.sqrt();
            }
        }

        returns.insert(sym, ret);
        vols.insert(sym, vol);
    }

    // For each bar, rank assets by return and volatility
    let mut cs_momentum_rank: HashMap<&String, Vec<f64>> = symbols
        .iter()
        .map(|s| (*s, vec![0.5f64; use_len]))
        .collect();
    let mut cs_vol_rank: HashMap<&String, Vec<f64>> = symbols
        .iter()
        .map(|s| (*s, vec![0.5f64; use_len]))
        .collect();

    for i in momentum_period..use_len {
        let ret_values: Vec<(&String, f64)> = symbols
            .iter()
            .map(|s| (*s, returns[s][i]))
            .collect();
        let vol_values: Vec<(&String, f64)> = symbols
            .iter()
            .map(|s| (*s, vols[s][i]))
            .collect();

        // Compute ranks (fraction of assets this one beats)
        for sym in &symbols {
            let r = returns[sym][i];
            let v = vols[sym][i];

            let ret_rank = ret_values.iter().filter(|(_, rv)| *rv < r).count() as f64
                / (n_assets - 1).max(1) as f64;
            let vol_rank = vol_values.iter().filter(|(_, vv)| *vv < v).count() as f64
                / (n_assets - 1).max(1) as f64;

            cs_momentum_rank.get_mut(sym).unwrap()[i] = ret_rank;
            cs_vol_rank.get_mut(sym).unwrap()[i] = vol_rank;
        }
    }

    // Build result: inject rank columns into each DataFrame
    let mut result = HashMap::new();
    for sym in &symbols {
        let df = &dfs[*sym];
        let df_len = df.height();
        let rank_len = cs_momentum_rank[sym].len();

        // If this asset is longer than the ranking window, pad the front with 0.5
        let pad = df_len.saturating_sub(rank_len);
        let mut mom_padded = vec![0.5f64; pad];
        let mut vol_padded = vec![0.5f64; pad];

        mom_padded.extend_from_slice(&cs_momentum_rank[sym][..rank_len.min(df_len)]);
        vol_padded.extend_from_slice(&cs_vol_rank[sym][..rank_len.min(df_len)]);

        // Truncate or pad to exactly df_len
        mom_padded.resize(df_len, 0.5);
        vol_padded.resize(df_len, 0.5);

        // Trend score: momentum rank - vol rank (reward momentum, penalize volatility)
        let trend_score: Vec<f64> = mom_padded
            .iter()
            .zip(vol_padded.iter())
            .map(|(m, v)| (m - 0.5) - 0.3 * (v - 0.5))
            .collect();

        let mut enriched = df.clone();
        enriched.with_column(Series::new("cs_momentum_rank", mom_padded))?;
        enriched.with_column(Series::new("cs_vol_rank", vol_padded))?;
        enriched.with_column(Series::new("cs_trend_score", trend_score))?;

        result.insert((*sym).clone(), enriched);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    

    fn make_df(closes: Vec<f64>) -> DataFrame {
        let n = closes.len();
        DataFrame::new(vec![
            Series::new("close", closes),
            Series::new("volume", vec![1000.0f64; n]),
        ])
        .unwrap()
    }

    #[test]
    fn test_cs_features_basic() {
        let mut dfs = HashMap::new();
        // Asset A: strongly trending up
        let a: Vec<f64> = (0..100).map(|i| 100.0 + i as f64).collect();
        // Asset B: flat
        let b: Vec<f64> = vec![100.0; 100];
        // Asset C: trending down
        let c: Vec<f64> = (0..100).map(|i| 100.0 - i as f64 * 0.5).collect();

        dfs.insert("A".to_string(), make_df(a));
        dfs.insert("B".to_string(), make_df(b));
        dfs.insert("C".to_string(), make_df(c));

        let result = compute_cs_features(&dfs, 20).unwrap();

        assert_eq!(result.len(), 3);
        assert!(result["A"].get_column_names().contains(&"cs_momentum_rank"));
        assert!(result["A"].get_column_names().contains(&"cs_vol_rank"));
        assert!(result["A"].get_column_names().contains(&"cs_trend_score"));
    }

    #[test]
    fn test_cs_momentum_rank_ordering() {
        let mut dfs = HashMap::new();
        let a: Vec<f64> = (0..50).map(|i| 100.0 + i as f64 * 2.0).collect(); // strong up
        let b: Vec<f64> = vec![100.0f64; 50]; // flat
        let c: Vec<f64> = (0..50).map(|i| 100.0 - i as f64).collect(); // strong down

        dfs.insert("A".to_string(), make_df(a));
        dfs.insert("B".to_string(), make_df(b));
        dfs.insert("C".to_string(), make_df(c));

        let result = compute_cs_features(&dfs, 10).unwrap();

        // At bar 49, A should have higher rank than B, B higher than C
        let a_rank = result["A"]
            .column("cs_momentum_rank")
            .unwrap()
            .f64()
            .unwrap()
            .get(49)
            .unwrap();
        let b_rank = result["B"]
            .column("cs_momentum_rank")
            .unwrap()
            .f64()
            .unwrap()
            .get(49)
            .unwrap();
        let c_rank = result["C"]
            .column("cs_momentum_rank")
            .unwrap()
            .f64()
            .unwrap()
            .get(49)
            .unwrap();

        assert!(a_rank > b_rank, "A({a_rank:.2}) should rank higher than B({b_rank:.2})");
        assert!(b_rank > c_rank, "B({b_rank:.2}) should rank higher than C({c_rank:.2})");
    }

    #[test]
    fn test_cs_features_error_on_empty() {
        let dfs: HashMap<String, DataFrame> = HashMap::new();
        assert!(compute_cs_features(&dfs, 20).is_err());
    }

    #[test]
    fn test_cs_trend_score_range() {
        let mut dfs = HashMap::new();
        let a: Vec<f64> = (0..60).map(|i| 100.0 + i as f64).collect();
        let b: Vec<f64> = (0..60).map(|i| 100.0 - i as f64 * 0.5).collect();
        dfs.insert("A".to_string(), make_df(a));
        dfs.insert("B".to_string(), make_df(b));

        let result = compute_cs_features(&dfs, 10).unwrap();
        let scores = result["A"].column("cs_trend_score").unwrap().f64().unwrap();

        for i in 10..60 {
            let s = scores.get(i).unwrap();
            assert!(
                s >= -2.0 && s <= 2.0,
                "Trend score {s:.2} out of expected range at bar {i}"
            );
        }
    }
}

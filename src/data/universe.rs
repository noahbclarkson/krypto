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
        limit: u16,
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
        let closes: Vec<f64> = df.column("close")?.f64()?.into_iter().map(|v| v.unwrap_or(0.0)).collect();
        closes_map.insert(sym.clone(), closes);
    }

    // We will build rank arrays for each symbol
    let mut rank_map: HashMap<String, Vec<f64>> = symbols.iter()
        .map(|s| (s.clone(), vec![0.5f64; n_bars])) // default to neutral 0.5
        .collect();
        
    let mut vol_adj_rank_map: HashMap<String, Vec<f64>> = symbols.iter()
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
                if slice[j-1] > 0.0 && slice[j] > 0.0 {
                    log_returns.push((slice[j] / slice[j-1]).ln());
                } else {
                    log_returns.push(0.0);
                }
            }
            
            let mean_log_ret = log_returns.iter().sum::<f64>() / log_returns.len() as f64;
            let variance = log_returns.iter().map(|r| (r - mean_log_ret).powi(2)).sum::<f64>() / log_returns.len() as f64;
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

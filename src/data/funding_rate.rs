//! Binance Perpetual Funding Rate Fetcher
//!
//! Funding rates are published every 8 hours (00:00, 08:00, 16:00 UTC).
//! Extreme positive rates → market is overheated long → price tends to revert down.
//! Extreme negative rates → market is overheated short → price tends to revert up.
//!
//! This loader fetches the full funding rate history and can align it to a price DataFrame.
//!
//! # Example
//! ```ignore
//! let loader = FundingRateLoader::new();
//! let rates = loader.fetch_all("BTCUSDT").await?;
//! let aligned = loader.align_to_ohlcv(&rates, &price_df, "4h")?;
//! ```

use anyhow::{Context, Result};
use polars::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const BINANCE_FUTURES_BASE: &str = "https://fapi.binance.com";
const BATCH_SIZE: u32 = 1000;
const FUNDING_INTERVAL_MS: i64 = 8 * 60 * 60 * 1000; // 8h in ms

// ─── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingRateRecord {
    pub symbol: String,
    pub funding_time_ms: i64,
    pub funding_rate: f64,
    pub mark_price: f64,
}

impl FundingRateRecord {
    pub fn funding_time_secs(&self) -> i64 {
        self.funding_time_ms / 1000
    }
}

// ─── Raw Binance response ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BinanceFundingRate {
    #[serde(rename = "fundingTime")]
    funding_time: u64,
    #[serde(rename = "fundingRate")]
    funding_rate: String,
    #[serde(rename = "markPrice")]
    mark_price: String,
}

// ─── Cache ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct FundingCache {
    records: Vec<FundingRateRecord>,
    fetched_at_ms: i64,
}

fn cache_path(base_dir: &Path, symbol: &str) -> PathBuf {
    base_dir.join(format!("funding_{}. bincode", symbol.to_lowercase()))
}

fn load_cache(path: &Path) -> Option<FundingCache> {
    let data = std::fs::read(path).ok()?;
    bincode::deserialize(&data).ok()
}

fn save_cache(path: &Path, cache: &FundingCache) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = bincode::serialize(cache) {
        let _ = std::fs::write(path, data);
    }
}

// ─── Loader ────────────────────────────────────────────────────────────────────

pub struct FundingRateLoader {
    client: reqwest::Client,
    cache_dir: Option<PathBuf>,
    /// How old cache can be before a refresh is triggered (ms). Default: 8h.
    cache_ttl_ms: i64,
}

impl FundingRateLoader {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            cache_dir: None,
            cache_ttl_ms: FUNDING_INTERVAL_MS,
        }
    }

    pub fn with_cache(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            client: reqwest::Client::new(),
            cache_dir: Some(cache_dir.into()),
            cache_ttl_ms: FUNDING_INTERVAL_MS,
        }
    }

    /// Fetch the complete funding rate history for a symbol.
    /// Uses pagination to retrieve all records from inception to now.
    pub async fn fetch_all(&self, symbol: &str) -> Result<Vec<FundingRateRecord>> {
        // Check cache first
        if let Some(ref dir) = self.cache_dir {
            let path = cache_path(dir, symbol);
            if let Some(cache) = load_cache(&path) {
                let now_ms = chrono::Utc::now().timestamp_millis();
                if now_ms - cache.fetched_at_ms < self.cache_ttl_ms {
                    return Ok(cache.records);
                }
                // Cache stale — do an incremental update
                if !cache.records.is_empty() {
                    let last_ms = cache.records.last().unwrap().funding_time_ms;
                    let mut records = cache.records;
                    let new = self.fetch_since(symbol, last_ms + 1).await?;
                    records.extend(new);
                    let updated = FundingCache {
                        records: records.clone(),
                        fetched_at_ms: now_ms,
                    };
                    save_cache(&path, &updated);
                    return Ok(records);
                }
            }
        }

        // Full fetch from inception
        let records = self.fetch_paginated(symbol, None, None).await?;

        if let Some(ref dir) = self.cache_dir {
            let path = cache_path(dir, symbol);
            let cache = FundingCache {
                records: records.clone(),
                fetched_at_ms: chrono::Utc::now().timestamp_millis(),
            };
            save_cache(&path, &cache);
        }

        Ok(records)
    }

    /// Fetch funding rates starting from a specific timestamp (ms).
    async fn fetch_since(&self, symbol: &str, start_ms: i64) -> Result<Vec<FundingRateRecord>> {
        self.fetch_paginated(symbol, Some(start_ms as u64), None).await
    }

    /// Paginated fetch: walks forward through time in BATCH_SIZE chunks.
    async fn fetch_paginated(
        &self,
        symbol: &str,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<Vec<FundingRateRecord>> {
        let mut all: Vec<FundingRateRecord> = Vec::new();
        let mut current_start = start_time;

        loop {
            let mut url = format!(
                "{}/fapi/v1/fundingRate?symbol={}&limit={}",
                BINANCE_FUTURES_BASE, symbol, BATCH_SIZE
            );
            if let Some(st) = current_start {
                url.push_str(&format!("&startTime={}", st));
            }
            if let Some(et) = end_time {
                url.push_str(&format!("&endTime={}", et));
            }

            let resp = self
                .client
                .get(&url)
                .send()
                .await
                .context("Failed to fetch funding rate from Binance")?;

            let batch: Vec<BinanceFundingRate> = resp
                .json()
                .await
                .context("Failed to parse funding rate response")?;

            if batch.is_empty() {
                break;
            }

            let last_time = batch.last().unwrap().funding_time;
            let n = batch.len();

            for r in batch {
                all.push(FundingRateRecord {
                    symbol: symbol.to_string(),
                    funding_time_ms: r.funding_time as i64,
                    funding_rate: r.funding_rate.parse().unwrap_or(0.0),
                    mark_price: r.mark_price.parse().unwrap_or(0.0),
                });
            }

            if n < BATCH_SIZE as usize {
                break;
            }

            // Advance past the last record
            current_start = Some(last_time + 1);

            // Rate limit: 1 req/s is conservative; Binance allows more
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        Ok(all)
    }

    /// Align funding rates to an OHLCV DataFrame.
    ///
    /// For each bar in the price DataFrame, finds the most recent funding rate at or before
    /// the bar's timestamp and adds it as a new column `funding_rate`.
    ///
    /// Funding rates are 8h — they apply to bars between publication times.
    pub fn align_to_ohlcv(
        &self,
        rates: &[FundingRateRecord],
        price_df: &DataFrame,
        _interval: &str,
    ) -> Result<DataFrame> {
        if rates.is_empty() {
            anyhow::bail!("No funding rate records to align");
        }

        let time_col = price_df
            .column("time")
            .context("price_df must have 'time' column")?
            .cast(&DataType::Int64)
            .context("Failed to cast time to i64")?;

        let bar_times: Vec<i64> = time_col.i64()?.into_iter().flatten().collect();
        let n = bar_times.len();

        // Build sorted funding time → rate lookup
        let mut sorted_rates: Vec<(i64, f64)> = rates
            .iter()
            .map(|r| (r.funding_time_ms, r.funding_rate))
            .collect();
        sorted_rates.sort_by_key(|(t, _)| *t);

        // For each bar, binary search for the most recent funding rate
        let mut aligned_rates: Vec<f64> = Vec::with_capacity(n);
        let mut aligned_rate_ma8: Vec<f64> = Vec::with_capacity(n);
        let mut aligned_rate_z: Vec<f64> = Vec::with_capacity(n);

        for &bar_time_ms in &bar_times {
            let idx = sorted_rates
                .partition_point(|(t, _)| *t <= bar_time_ms);
            let rate = if idx == 0 {
                0.0
            } else {
                sorted_rates[idx - 1].1
            };
            aligned_rates.push(rate);
        }

        // Compute 8-period (= 64h on 8h data, 8 periods) moving average of funding rate
        let window = 8usize;
        for i in 0..n {
            let start = if i >= window { i - window + 1 } else { 0 };
            let slice = &aligned_rates[start..=i];
            aligned_rate_ma8.push(slice.iter().sum::<f64>() / slice.len() as f64);
        }

        // Compute z-score of funding rate vs its rolling 30-period mean/std
        let zscore_window = 30usize;
        for i in 0..n {
            let start = if i >= zscore_window { i - zscore_window + 1 } else { 0 };
            let slice = &aligned_rates[start..=i];
            let mean = slice.iter().sum::<f64>() / slice.len() as f64;
            let variance = slice.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / slice.len() as f64;
            let std = variance.sqrt();
            let z = if std > f64::EPSILON {
                (aligned_rates[i] - mean) / std
            } else {
                0.0
            };
            aligned_rate_z.push(z);
        }

        // Clone price_df and add new columns
        let mut df = price_df.clone();
        df.with_column(Series::new("funding_rate".into(), aligned_rates))?;
        df.with_column(Series::new("funding_rate_ma8".into(), aligned_rate_ma8))?;
        df.with_column(Series::new("funding_rate_z".into(), aligned_rate_z))?;

        Ok(df)
    }

    /// Compute funding rate statistics for a given slice of records.
    pub fn compute_stats(rates: &[FundingRateRecord]) -> FundingRateStats {
        if rates.is_empty() {
            return FundingRateStats::default();
        }
        let values: Vec<f64> = rates.iter().map(|r| r.funding_rate).collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / values.len() as f64;
        let std = variance.sqrt();
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        let mut sorted = values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p5 = sorted[(sorted.len() as f64 * 0.05) as usize];
        let p95 = sorted[(sorted.len() as f64 * 0.95) as usize];

        FundingRateStats { mean, std, min, max, p5, p95, count: values.len() }
    }
}

impl Default for FundingRateLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default)]
pub struct FundingRateStats {
    pub mean: f64,
    pub std: f64,
    pub min: f64,
    pub max: f64,
    /// 5th percentile (extreme negative)
    pub p5: f64,
    /// 95th percentile (extreme positive)
    pub p95: f64,
    pub count: usize,
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rates(values: &[f64]) -> Vec<FundingRateRecord> {
        values
            .iter()
            .enumerate()
            .map(|(i, &r)| FundingRateRecord {
                symbol: "BTCUSDT".to_string(),
                funding_time_ms: i as i64 * FUNDING_INTERVAL_MS,
                funding_rate: r,
                mark_price: 50_000.0,
            })
            .collect()
    }

    #[test]
    fn test_compute_stats_basic() {
        let rates = make_rates(&[0.0001, -0.0001, 0.0002, -0.0002, 0.0001]);
        let stats = FundingRateLoader::compute_stats(&rates);
        assert_eq!(stats.count, 5);
        assert!((stats.mean - 0.00002).abs() < 1e-10);
        assert!(stats.min < 0.0);
        assert!(stats.max > 0.0);
    }

    #[test]
    fn test_compute_stats_empty() {
        let stats = FundingRateLoader::compute_stats(&[]);
        assert_eq!(stats.count, 0);
    }

    #[test]
    fn test_align_to_ohlcv_basic() {
        use polars::prelude::*;
        use chrono::NaiveDate;

        // Build a minimal price DataFrame with timestamps matching funding rate times
        let base_ms = 0i64;
        let bar_times: Vec<i64> = (0..10)
            .map(|i| base_ms + i * 4 * 60 * 60 * 1000) // 4h bars
            .collect();

        let df = DataFrame::new(vec![
            Series::new("time".into(), bar_times.clone())
                .cast(&DataType::Datetime(TimeUnit::Milliseconds, None))
                .unwrap(),
            Series::new("close".into(), vec![100.0f64; 10]),
        ])
        .unwrap();

        // Funding rates at t=0 and t=8h
        let rates = make_rates(&[0.0001, -0.0002]);

        let loader = FundingRateLoader::new();
        let aligned = loader.align_to_ohlcv(&rates, &df, "4h").unwrap();

        assert!(aligned.column("funding_rate").is_ok());
        assert!(aligned.column("funding_rate_z").is_ok());
        assert!(aligned.column("funding_rate_ma8").is_ok());

        let fr = aligned.column("funding_rate").unwrap().f64().unwrap();
        // First 2 bars (0-4h) should have rate from t=0 (0.0001)
        assert!((fr.get(0).unwrap() - 0.0001).abs() < 1e-10);
        // Bar at 8h should have rate from t=8h (-0.0002)
        assert!((fr.get(2).unwrap() - (-0.0002)).abs() < 1e-10);
    }

    #[test]
    fn test_funding_record_time_conversion() {
        let r = FundingRateRecord {
            symbol: "BTCUSDT".to_string(),
            funding_time_ms: 1_000_000,
            funding_rate: 0.0001,
            mark_price: 50_000.0,
        };
        assert_eq!(r.funding_time_secs(), 1_000);
    }
}

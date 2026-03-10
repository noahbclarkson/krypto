//! Binance Perpetual Funding Rate Data Loader
//!
//! Fetches funding rate history from Binance USDT-M Futures API.
//! Funding rates are published every 8 hours (00:00, 08:00, 16:00 UTC).
//! No API key required — this is public data.
//!
//! # Example
//! ```ignore
//! use krypto::data::funding_rate::FundingRateLoader;
//!
//! let loader = FundingRateLoader::new();
//! let df = loader.fetch("BTCUSDT", None, None).await?;
//! println!("Loaded {} funding rate records", df.height());
//! ```

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use polars::prelude::*;
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;

const FUNDING_BASE_URL: &str = "https://fapi.binance.com/fapi/v1/fundingRate";
const PAGE_LIMIT: u64 = 1000;
/// Funding interval in milliseconds (8 hours).
pub const FUNDING_INTERVAL_MS: i64 = 8 * 3600 * 1_000;

#[derive(Debug, Deserialize)]
struct RawFundingRate {
    #[serde(rename = "fundingTime")]
    funding_time: i64,
    #[serde(rename = "fundingRate")]
    funding_rate: String,
    #[serde(rename = "markPrice")]
    mark_price: Option<String>,
}

/// Configuration for caching funding rate data.
#[derive(Debug, Clone)]
pub struct FundingCacheConfig {
    pub dir: String,
    pub enabled: bool,
}

impl Default for FundingCacheConfig {
    fn default() -> Self {
        Self {
            dir: "data/funding_cache".to_string(),
            enabled: true,
        }
    }
}

/// Loader for Binance perpetual funding rate history.
pub struct FundingRateLoader {
    client: Client,
    cache: FundingCacheConfig,
}

impl FundingRateLoader {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            cache: FundingCacheConfig::default(),
        }
    }

    pub fn with_cache_dir(dir: &str) -> Self {
        Self {
            client: Client::new(),
            cache: FundingCacheConfig {
                dir: dir.to_string(),
                enabled: true,
            },
        }
    }

    fn cache_path(&self, symbol: &str) -> PathBuf {
        PathBuf::from(&self.cache.dir).join(format!("{}_funding.parquet", symbol.to_lowercase()))
    }

    /// Fetch full funding rate history for a symbol.
    ///
    /// Paginates through all available history (Binance allows up to 1000 records per page).
    /// Results are cached to parquet if caching is enabled.
    ///
    /// Returns a DataFrame with columns: `time` (Datetime), `funding_rate` (f64), `mark_price` (f64).
    pub async fn fetch(
        &self,
        symbol: &str,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<DataFrame> {
        let cache_path = self.cache_path(symbol);

        // Try cache first
        if self.cache.enabled && cache_path.exists() && start_time.is_none() && end_time.is_none() {
            if let Ok(df) = self.load_cache(&cache_path) {
                return Ok(df);
            }
        }

        let df = self.fetch_paginated(symbol, start_time, end_time).await?;

        // Save to cache
        if self.cache.enabled && start_time.is_none() && end_time.is_none() {
            if let Err(e) = self.save_cache(&cache_path, &df) {
                eprintln!("Warning: failed to cache funding rates: {e}");
            }
        }

        Ok(df)
    }

    async fn fetch_paginated(
        &self,
        symbol: &str,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<DataFrame> {
        let mut all_records: Vec<RawFundingRate> = Vec::new();

        // Default start: 2020-01-01 (earliest reliable Binance perpetual data)
        let mut current_start = start_time
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(1_577_836_800_000i64); // 2020-01-01 UTC

        let end_ms = end_time
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(i64::MAX);

        loop {
            let url = format!(
                "{}?symbol={}&limit={}&startTime={}",
                FUNDING_BASE_URL, symbol, PAGE_LIMIT, current_start
            );

            let resp: Vec<RawFundingRate> = self
                .client
                .get(&url)
                .send()
                .await
                .context("Failed to fetch funding rates from Binance")?
                .json()
                .await
                .context("Failed to parse funding rate response")?;

            if resp.is_empty() {
                break;
            }

            let last_time = resp.last().unwrap().funding_time;
            let page_count = resp.len() as u64;

            for r in resp {
                if r.funding_time <= end_ms {
                    all_records.push(r);
                }
            }

            // Stop if we've reached the end or didn't get a full page
            if last_time >= end_ms || page_count < PAGE_LIMIT {
                break;
            }

            // Advance to next page (funding rates are every 8h)
            current_start = last_time + FUNDING_INTERVAL_MS;

            // Brief pause to be a good API citizen
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        self.records_to_df(all_records)
    }

    fn records_to_df(&self, records: Vec<RawFundingRate>) -> Result<DataFrame> {
        let mut times: Vec<i64> = Vec::with_capacity(records.len());
        let mut rates: Vec<f64> = Vec::with_capacity(records.len());
        let mut marks: Vec<f64> = Vec::with_capacity(records.len());

        for r in records {
            times.push(r.funding_time);
            rates.push(r.funding_rate.parse::<f64>().unwrap_or(0.0));
            marks.push(
                r.mark_price
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0),
            );
        }

        let df = DataFrame::new(vec![
            Series::new("time", times)
                .cast(&DataType::Datetime(TimeUnit::Milliseconds, None))?,
            Series::new("funding_rate", rates),
            Series::new("mark_price", marks),
        ])?;

        Ok(df)
    }

    fn load_cache(&self, path: &PathBuf) -> Result<DataFrame> {
        use polars::io::parquet::ParquetReader;
        use std::fs::File;
        let file = File::open(path)?;
        let df = ParquetReader::new(file).finish()?;
        Ok(df)
    }

    fn save_cache(&self, path: &PathBuf, df: &DataFrame) -> Result<()> {
        use polars::io::parquet::ParquetWriter;
        use std::fs::{self, File};
        fs::create_dir_all(path.parent().unwrap_or(path))?;
        let file = File::create(path)?;
        ParquetWriter::new(file).finish(&mut df.clone())?;
        Ok(())
    }
}

impl Default for FundingRateLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute rolling statistics on a funding rate series for use as strategy features.
///
/// Returns a DataFrame with the original data plus:
/// - `funding_rate_z`: z-score of funding rate (using rolling window)
/// - `funding_rate_ma`: rolling mean
/// - `funding_rate_std`: rolling std
/// - `funding_extreme`: 1.0 if top quartile, -1.0 if bottom quartile, 0.0 otherwise
pub fn compute_funding_features(df: &DataFrame, window: usize) -> Result<DataFrame> {
    let rates = df.column("funding_rate")?.f64()?;
    let n = rates.len();

    let mut z_scores = vec![0.0f64; n];
    let mut rolling_ma = vec![0.0f64; n];
    let mut rolling_std = vec![0.0f64; n];
    let mut extremes = vec![0.0f64; n];

    for i in 0..n {
        if i < window {
            continue;
        }
        let slice: Vec<f64> = (i.saturating_sub(window)..i)
            .filter_map(|j| rates.get(j))
            .collect();

        if slice.len() < 2 {
            continue;
        }

        let mean = slice.iter().sum::<f64>() / slice.len() as f64;
        let variance =
            slice.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (slice.len() - 1) as f64;
        let std = variance.sqrt();

        rolling_ma[i] = mean;
        rolling_std[i] = std;

        if std > f64::EPSILON {
            let current = rates.get(i).unwrap_or(0.0);
            z_scores[i] = (current - mean) / std;

            // Classify as extreme (using 1-sigma threshold — in crypto, funding is leptokurtic)
            if z_scores[i] > 1.5 {
                extremes[i] = 1.0; // Extreme positive: longs paying, fade the longs
            } else if z_scores[i] < -1.5 {
                extremes[i] = -1.0; // Extreme negative: shorts paying, fade the shorts
            }
        }
    }

    let mut result = df.clone();
    result.with_column(Series::new("funding_rate_z", z_scores))?;
    result.with_column(Series::new("funding_rate_ma", rolling_ma))?;
    result.with_column(Series::new("funding_rate_std", rolling_std))?;
    result.with_column(Series::new("funding_extreme", extremes))?;

    Ok(result)
}

/// Align funding rate data to an OHLCV DataFrame's timestamps.
///
/// Funding rates are published every 8 hours, but OHLCV data may be 1h or 4h.
///
/// **Important:** z-scores are computed on the raw funding data (in funding-period space,
/// not OHLCV bar space) to avoid the repeated-value problem from forward-filling.
/// With z_window=90 funding periods = ~30 days of rolling baseline.
///
/// The resulting DataFrame has all original OHLCV columns plus:
/// - `funding_rate`: most recent funding rate at that bar (forward-filled)
/// - `funding_rate_z`: rolling z-score (computed on raw 8h data, then aligned)
/// - `funding_rate_ma`: rolling mean on raw funding data
/// - `funding_rate_std`: rolling std on raw funding data
/// - `funding_extreme`: -1.0, 0.0, or 1.0 classification
///
/// # Parameters
/// - `ohlcv_df`: the OHLCV DataFrame (must have `time` column as Datetime milliseconds)
/// - `funding_df`: output from `FundingRateLoader::fetch()`
/// - `z_window`: number of **funding periods** (8h each) for rolling z-score. Default: 90 (~30 days)
pub fn align_to_ohlcv(
    ohlcv_df: &DataFrame,
    funding_df: &DataFrame,
    z_window: usize,
) -> Result<DataFrame> {
    // Step 1: compute z-score features on raw funding data (8h intervals)
    let funding_with_features = compute_funding_features(funding_df, z_window)?;

    // Extract all feature columns from funding data
    let fund_times = funding_with_features.column("time")?.cast(&DataType::Int64)?;
    let fund_ts: Vec<i64> = fund_times.i64()?.into_iter().map(|v| v.unwrap_or(0)).collect();

    let fund_rate: Vec<f64> = funding_with_features.column("funding_rate")?.f64()?.into_iter().map(|v| v.unwrap_or(0.0)).collect();
    let fund_z: Vec<f64> = funding_with_features.column("funding_rate_z")?.f64()?.into_iter().map(|v| v.unwrap_or(0.0)).collect();
    let fund_ma: Vec<f64> = funding_with_features.column("funding_rate_ma")?.f64()?.into_iter().map(|v| v.unwrap_or(0.0)).collect();
    let fund_std: Vec<f64> = funding_with_features.column("funding_rate_std")?.f64()?.into_iter().map(|v| v.unwrap_or(0.0)).collect();
    let fund_extreme: Vec<f64> = funding_with_features.column("funding_extreme")?.f64()?.into_iter().map(|v| v.unwrap_or(0.0)).collect();

    // Step 2: forward-fill all features onto OHLCV timestamps
    let ohlcv_times = ohlcv_df.column("time")?.cast(&DataType::Int64)?;
    let ohlcv_ts: Vec<i64> = ohlcv_times.i64()?.into_iter().map(|v| v.unwrap_or(0)).collect();

    let n_ohlcv = ohlcv_ts.len();
    let mut aligned_rate = vec![0.0f64; n_ohlcv];
    let mut aligned_z = vec![0.0f64; n_ohlcv];
    let mut aligned_ma = vec![0.0f64; n_ohlcv];
    let mut aligned_std = vec![0.0f64; n_ohlcv];
    let mut aligned_extreme = vec![0.0f64; n_ohlcv];

    let mut fund_idx = 0usize;
    for (i, &ots) in ohlcv_ts.iter().enumerate() {
        // Advance to most recent funding record at or before this bar
        while fund_idx + 1 < fund_ts.len() && fund_ts[fund_idx + 1] <= ots {
            fund_idx += 1;
        }
        if !fund_ts.is_empty() && fund_ts[fund_idx] <= ots {
            aligned_rate[i] = fund_rate[fund_idx];
            aligned_z[i] = fund_z[fund_idx];
            aligned_ma[i] = fund_ma[fund_idx];
            aligned_std[i] = fund_std[fund_idx];
            aligned_extreme[i] = fund_extreme[fund_idx];
        }
        // else: before first funding record — leave as 0 (neutral)
    }

    // Step 3: attach to OHLCV DataFrame
    let mut result = ohlcv_df.clone();
    result.with_column(Series::new("funding_rate", aligned_rate))?;
    result.with_column(Series::new("funding_rate_z", aligned_z))?;
    result.with_column(Series::new("funding_rate_ma", aligned_ma))?;
    result.with_column(Series::new("funding_rate_std", aligned_std))?;
    result.with_column(Series::new("funding_extreme", aligned_extreme))?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    

    fn make_funding_df(rates: Vec<f64>) -> DataFrame {
        let n = rates.len();
        let times: Vec<i64> = (0..n as i64)
            .map(|i| 1_577_836_800_000 + i * FUNDING_INTERVAL_MS)
            .collect();
        DataFrame::new(vec![
            Series::new("time".into(), times)
                .cast(&DataType::Datetime(TimeUnit::Milliseconds, None))
                .unwrap(),
            Series::new("funding_rate".into(), rates),
            Series::new("mark_price".into(), vec![50_000.0f64; n]),
        ])
        .unwrap()
    }

    #[test]
    fn test_funding_features_basic() {
        let rates = vec![0.0001f64; 30]; // constant rates
        let df = make_funding_df(rates);
        let result = compute_funding_features(&df, 10).unwrap();
        assert!(result.get_column_names().contains(&"funding_rate_z"));
        assert!(result.get_column_names().contains(&"funding_extreme"));
    }

    #[test]
    fn test_funding_features_extreme_positive() {
        // Use varying baseline so std > 0, then spike the last bar well above 1.5 sigma
        let mut rates: Vec<f64> = (0..30).map(|i| 0.0001 + (i as f64 % 5.0) * 0.00005).collect();
        rates[29] = 0.05; // large positive spike
        let df = make_funding_df(rates);
        let result = compute_funding_features(&df, 20).unwrap();
        let extremes = result.column("funding_extreme").unwrap().f64().unwrap();
        assert_eq!(extremes.get(29).unwrap(), 1.0, "Expected extreme positive at last bar");
    }

    #[test]
    fn test_funding_features_extreme_negative() {
        let mut rates: Vec<f64> = (0..30).map(|i| 0.0001 + (i as f64 % 5.0) * 0.00005).collect();
        rates[29] = -0.05; // large negative spike
        let df = make_funding_df(rates);
        let result = compute_funding_features(&df, 20).unwrap();
        let extremes = result.column("funding_extreme").unwrap().f64().unwrap();
        assert_eq!(extremes.get(29).unwrap(), -1.0, "Expected extreme negative at last bar");
    }

    #[test]
    fn test_funding_interval_ms() {
        assert_eq!(FUNDING_INTERVAL_MS, 28_800_000); // 8h in ms
    }
}

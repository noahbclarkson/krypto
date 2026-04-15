//! Binance Futures Open Interest History loader.
//!
//! Fetches public daily open-interest history from Binance USDT-M futures and
//! aligns it to OHLCV bars for state / crowding research.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use polars::prelude::*;
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;

const OI_BASE_URL: &str = "https://fapi.binance.com/futures/data/openInterestHist";
const PAGE_LIMIT: u64 = 500;
pub const OI_INTERVAL_MS_1D: i64 = 24 * 3600 * 1_000;

#[derive(Debug, Clone, Copy)]
pub struct OpenInterestPeriod {
    pub label: &'static str,
    pub interval_ms: i64,
}

impl OpenInterestPeriod {
    pub const M5: Self = Self {
        label: "5m",
        interval_ms: 5 * 60 * 1_000,
    };
    pub const M15: Self = Self {
        label: "15m",
        interval_ms: 15 * 60 * 1_000,
    };
    pub const M30: Self = Self {
        label: "30m",
        interval_ms: 30 * 60 * 1_000,
    };
    pub const H1: Self = Self {
        label: "1h",
        interval_ms: 60 * 60 * 1_000,
    };
    pub const H2: Self = Self {
        label: "2h",
        interval_ms: 2 * 60 * 60 * 1_000,
    };
    pub const H4: Self = Self {
        label: "4h",
        interval_ms: 4 * 60 * 60 * 1_000,
    };
    pub const H6: Self = Self {
        label: "6h",
        interval_ms: 6 * 60 * 60 * 1_000,
    };
    pub const H12: Self = Self {
        label: "12h",
        interval_ms: 12 * 60 * 60 * 1_000,
    };
    pub const D1: Self = Self {
        label: "1d",
        interval_ms: 24 * 60 * 60 * 1_000,
    };
}

#[derive(Debug, Deserialize)]
struct RawOpenInterest {
    #[serde(rename = "sumOpenInterest")]
    sum_open_interest: String,
    #[serde(rename = "sumOpenInterestValue")]
    sum_open_interest_value: String,
    timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct OpenInterestCacheConfig {
    pub dir: String,
    pub enabled: bool,
}

impl Default for OpenInterestCacheConfig {
    fn default() -> Self {
        Self {
            dir: "data/open_interest_cache".to_string(),
            enabled: true,
        }
    }
}

pub struct OpenInterestLoader {
    client: Client,
    cache: OpenInterestCacheConfig,
}

impl OpenInterestLoader {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            cache: OpenInterestCacheConfig::default(),
        }
    }

    pub fn with_cache_dir(dir: &str) -> Self {
        Self {
            client: Client::new(),
            cache: OpenInterestCacheConfig {
                dir: dir.to_string(),
                enabled: true,
            },
        }
    }

    fn cache_path(&self, symbol: &str, period: OpenInterestPeriod) -> PathBuf {
        PathBuf::from(&self.cache.dir).join(format!(
            "{}_oi_{}.parquet",
            symbol.to_lowercase(),
            period.label
        ))
    }

    pub async fn fetch(
        &self,
        symbol: &str,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<DataFrame> {
        self.fetch_with_period(symbol, OpenInterestPeriod::D1, start_time, end_time)
            .await
    }

    pub async fn fetch_with_period(
        &self,
        symbol: &str,
        period: OpenInterestPeriod,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<DataFrame> {
        let cache_path = self.cache_path(symbol, period);
        if self.cache.enabled && cache_path.exists() && start_time.is_none() && end_time.is_none() {
            if let Ok(df) = self.load_cache(&cache_path) {
                return Ok(df);
            }
        }

        let df = self
            .fetch_paginated(symbol, period, start_time, end_time)
            .await?;

        if self.cache.enabled && start_time.is_none() && end_time.is_none() {
            if let Err(e) = self.save_cache(&cache_path, &df) {
                eprintln!("Warning: failed to cache open interest: {e}");
            }
        }

        Ok(df)
    }

    async fn fetch_paginated(
        &self,
        symbol: &str,
        period: OpenInterestPeriod,
        _start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<DataFrame> {
        // Binance's public openInterestHist endpoint is stricter than funding history and
        // may not honor deep pagination consistently. We fetch the latest honest window for
        // the requested period instead of pretending we have more history than the endpoint serves.
        let mut url = format!(
            "{}?symbol={}&period={}&limit={}",
            OI_BASE_URL, symbol, period.label, PAGE_LIMIT
        );
        if let Some(end) = end_time {
            url.push_str(&format!("&endTime={}", end.timestamp_millis()));
        }

        let resp: Vec<RawOpenInterest> = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch open interest history from Binance")?
            .json()
            .await
            .context("Failed to parse open interest response")?;

        self.records_to_df(resp)
    }

    fn records_to_df(&self, records: Vec<RawOpenInterest>) -> Result<DataFrame> {
        let mut times = Vec::with_capacity(records.len());
        let mut oi = Vec::with_capacity(records.len());
        let mut oi_value = Vec::with_capacity(records.len());

        for r in records {
            times.push(r.timestamp);
            oi.push(r.sum_open_interest.parse::<f64>().unwrap_or(0.0));
            oi_value.push(r.sum_open_interest_value.parse::<f64>().unwrap_or(0.0));
        }

        Ok(DataFrame::new(vec![
            Series::new("time", times).cast(&DataType::Datetime(TimeUnit::Milliseconds, None))?,
            Series::new("open_interest", oi),
            Series::new("open_interest_value", oi_value),
        ])?)
    }

    fn load_cache(&self, path: &PathBuf) -> Result<DataFrame> {
        use polars::io::parquet::ParquetReader;
        use std::fs::File;
        let file = File::open(path)?;
        Ok(ParquetReader::new(file).finish()?)
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

impl Default for OpenInterestLoader {
    fn default() -> Self {
        Self::new()
    }
}

pub fn compute_open_interest_features(df: &DataFrame, window: usize) -> Result<DataFrame> {
    let oi = df.column("open_interest")?.f64()?;
    let oi_value = df.column("open_interest_value")?.f64()?;
    let n = oi.len();

    let mut oi_change_1 = vec![0.0; n];
    let mut oi_change_7 = vec![0.0; n];
    let mut oi_value_change_7 = vec![0.0; n];
    let mut oi_z = vec![0.0; n];

    for i in 0..n {
        let cur = oi.get(i).unwrap_or(0.0);
        if i >= 1 {
            let prev = oi.get(i - 1).unwrap_or(cur);
            if prev.abs() > f64::EPSILON {
                oi_change_1[i] = cur / prev - 1.0;
            }
        }
        if i >= 7 {
            let prev7 = oi.get(i - 7).unwrap_or(cur);
            let prev_val7 = oi_value
                .get(i - 7)
                .unwrap_or(oi_value.get(i).unwrap_or(0.0));
            if prev7.abs() > f64::EPSILON {
                oi_change_7[i] = cur / prev7 - 1.0;
            }
            if prev_val7.abs() > f64::EPSILON {
                oi_value_change_7[i] = oi_value.get(i).unwrap_or(0.0) / prev_val7 - 1.0;
            }
        }
        if i >= window {
            let slice: Vec<f64> = (i - window..i).filter_map(|j| oi.get(j)).collect();
            if slice.len() >= 2 {
                let mean = slice.iter().sum::<f64>() / slice.len() as f64;
                let var = slice.iter().map(|x| (x - mean).powi(2)).sum::<f64>()
                    / (slice.len() - 1) as f64;
                let std = var.sqrt();
                if std > f64::EPSILON {
                    oi_z[i] = (cur - mean) / std;
                }
            }
        }
    }

    let mut result = df.clone();
    result.with_column(Series::new("oi_change_1", oi_change_1))?;
    result.with_column(Series::new("oi_change_7", oi_change_7))?;
    result.with_column(Series::new("oi_value_change_7", oi_value_change_7))?;
    result.with_column(Series::new("oi_z", oi_z))?;
    Ok(result)
}

pub fn align_to_ohlcv(ohlcv_df: &DataFrame, oi_df: &DataFrame, window: usize) -> Result<DataFrame> {
    let oi_with_features = compute_open_interest_features(oi_df, window)?;

    let oi_times = oi_with_features.column("time")?.cast(&DataType::Int64)?;
    let oi_ts: Vec<i64> = oi_times
        .i64()?
        .into_iter()
        .map(|v| v.unwrap_or(0))
        .collect();
    let oi_raw: Vec<f64> = oi_with_features
        .column("open_interest")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let oi_val: Vec<f64> = oi_with_features
        .column("open_interest_value")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let oi_c1: Vec<f64> = oi_with_features
        .column("oi_change_1")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let oi_c7: Vec<f64> = oi_with_features
        .column("oi_change_7")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let oi_vc7: Vec<f64> = oi_with_features
        .column("oi_value_change_7")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let oi_z: Vec<f64> = oi_with_features
        .column("oi_z")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();

    let ohlcv_times = ohlcv_df.column("time")?.cast(&DataType::Int64)?;
    let ohlcv_ts: Vec<i64> = ohlcv_times
        .i64()?
        .into_iter()
        .map(|v| v.unwrap_or(0))
        .collect();

    let n = ohlcv_ts.len();
    let mut aligned_raw = vec![0.0; n];
    let mut aligned_val = vec![0.0; n];
    let mut aligned_c1 = vec![0.0; n];
    let mut aligned_c7 = vec![0.0; n];
    let mut aligned_vc7 = vec![0.0; n];
    let mut aligned_z = vec![0.0; n];

    let mut idx = 0usize;
    for (i, &t) in ohlcv_ts.iter().enumerate() {
        while idx + 1 < oi_ts.len() && oi_ts[idx + 1] <= t {
            idx += 1;
        }
        if !oi_ts.is_empty() && oi_ts[idx] <= t {
            aligned_raw[i] = oi_raw[idx];
            aligned_val[i] = oi_val[idx];
            aligned_c1[i] = oi_c1[idx];
            aligned_c7[i] = oi_c7[idx];
            aligned_vc7[i] = oi_vc7[idx];
            aligned_z[i] = oi_z[idx];
        }
    }

    let mut result = ohlcv_df.clone();
    result.with_column(Series::new("open_interest", aligned_raw))?;
    result.with_column(Series::new("open_interest_value", aligned_val))?;
    result.with_column(Series::new("oi_change_1", aligned_c1))?;
    result.with_column(Series::new("oi_change_7", aligned_c7))?;
    result.with_column(Series::new("oi_value_change_7", aligned_vc7))?;
    result.with_column(Series::new("oi_z", aligned_z))?;
    Ok(result)
}

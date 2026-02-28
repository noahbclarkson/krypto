//! Data loading pipeline for fetching and caching market data.
//!
//! The `DataLoader` provides:
//! - Fetching OHLCV data from Binance API with automatic pagination
//! - Local caching to avoid redundant API calls
//! - Save/load functionality for Parquet and CSV formats
//!
//! # Example
//!
//! ```rust,no_run
//! use krypto::data::loader::DataLoader;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let loader = DataLoader::new(None, None);
//!     let df = loader.fetch_data("BTCUSDT", "1h", 1000).await?;
//!     println!("Loaded {} candles", df.height());
//!     Ok(())
//! }
//! ```

use anyhow::{Context, Result};
use binance::api::Binance;
use binance::market::Market;
use binance::rest_model::KlineSummaries;
use chrono::DateTime;
use polars::prelude::*;
use std::path::Path;

/// Configuration for data caching behavior.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Directory to store cached data
    pub cache_dir: String,
    /// Whether to use caching (default: true)
    pub enabled: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_dir: "data/cache".to_string(),
            enabled: true,
        }
    }
}

/// Data loader for fetching OHLCV data from Binance.
///
/// Supports:
/// - Paginated fetching (up to 65535 candles)
/// - Local caching to avoid redundant API calls
/// - Export to Parquet/CSV formats
pub struct DataLoader {
    market: Market,
    cache: CacheConfig,
}

impl DataLoader {
    /// Create a new DataLoader with default cache settings.
    pub fn new(api_key: Option<String>, secret_key: Option<String>) -> Self {
        Self {
            market: Market::new(api_key, secret_key),
            cache: CacheConfig::default(),
        }
    }

    /// Create a DataLoader with custom cache configuration.
    pub fn with_cache(api_key: Option<String>, secret_key: Option<String>, cache: CacheConfig) -> Self {
        Self {
            market: Market::new(api_key, secret_key),
            cache,
        }
    }

    /// Generate a cache key for a symbol/interval combination.
    fn cache_key(&self, symbol: &str, interval: &str) -> String {
        format!("{}/{}_{}.parquet", self.cache.cache_dir, symbol.to_lowercase(), interval)
    }

    /// Try to load data from cache.
    ///
    /// Returns `None` if cache is disabled or file doesn't exist.
    pub fn load_from_cache(&self, symbol: &str, interval: &str) -> Result<Option<DataFrame>> {
        if !self.cache.enabled {
            return Ok(None);
        }

        let path = self.cache_key(symbol, interval);
        let path = Path::new(&path);

        if !path.exists() {
            return Ok(None);
        }

        let df = LazyFrame::scan_parquet(path, Default::default())?
            .collect()
            .context("Failed to read cached data")?;

        tracing::info!("Loaded {} candles from cache: {:?}", df.height(), path);
        Ok(Some(df))
    }

    /// Save DataFrame to cache.
    pub fn save_to_cache(&self, symbol: &str, interval: &str, df: &DataFrame) -> Result<()> {
        if !self.cache.enabled {
            return Ok(());
        }

        let path = self.cache_key(symbol, interval);
        let path = Path::new(&path);

        // Create cache directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create cache directory: {:?}", parent))?;
        }

        df.clone()
            .lazy()
            .sink_parquet(path.to_path_buf(), Default::default())
            .context("Failed to write cache file")?;

        tracing::info!("Saved {} candles to cache: {:?}", df.height(), path);
        Ok(())
    }

    /// Save DataFrame to a Parquet file.
    pub fn save_parquet(&self, df: &DataFrame, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }

        df.clone()
            .lazy()
            .sink_parquet(path.to_path_buf(), Default::default())
            .context("Failed to write Parquet file")?;

        tracing::info!("Saved {} candles to {:?}", df.height(), path);
        Ok(())
    }

    /// Save DataFrame to a CSV file.
    pub fn save_csv(&self, df: &DataFrame, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }

        let mut file = std::fs::File::create(path)
            .with_context(|| format!("Failed to create CSV file: {:?}", path))?;

        CsvWriter::new(&mut file)
            .include_header(true)
            .finish(&mut df.clone())
            .context("Failed to write CSV file")?;

        tracing::info!("Saved {} candles to {:?}", df.height(), path);
        Ok(())
    }

    /// Load DataFrame from a Parquet file.
    pub fn load_parquet(path: &Path) -> Result<DataFrame> {
        LazyFrame::scan_parquet(path, Default::default())?
            .collect()
            .context("Failed to read Parquet file")
    }

    /// Load DataFrame from a CSV file.
    pub fn load_csv(path: &Path) -> Result<DataFrame> {
        LazyCsvReader::new(path)
            .finish()?
            .collect()
            .context("Failed to read CSV file")
    }

    /// Fetches candles with pagination (1000 max per request on Binance).
    ///
    /// # Arguments
    /// * `symbol` - Trading pair (e.g., "BTCUSDT")
    /// * `interval` - Candle interval (e.g., "1h", "4h", "1d")
    /// * `total_candles` - Number of candles to fetch (max 65535)
    ///
    /// # Returns
    /// DataFrame with columns: time, open, high, low, close, volume
    ///
    /// # Errors
    /// Returns error if:
    /// - Network request fails
    /// - Invalid symbol or interval
    /// - No data returned
    pub async fn fetch_data(
        &self,
        symbol: &str,
        interval: &str,
        total_candles: u16,
    ) -> Result<DataFrame> {
        tracing::info!(
            "Fetching {} {} candles for {} from Binance",
            total_candles, interval, symbol
        );

        let mut all_klines = Vec::new();
        let mut remaining = total_candles as usize;
        let mut end_time: Option<u64> = None;
        let mut requests_made = 0;

        while remaining > 0 {
            let fetch_limit = remaining.min(1000) as u16;
            
            let resp = self
                .market
                .get_klines(symbol, interval, Some(fetch_limit), None, end_time)
                .await
                .with_context(|| {
                    format!(
                        "Failed to fetch {} candles for {} @ {} (end_time: {:?})",
                        fetch_limit, symbol, interval, end_time
                    )
                })?;

            requests_made += 1;

            let KlineSummaries::AllKlineSummaries(mut batch) = resp;
            if batch.is_empty() {
                tracing::warn!("No more data available from Binance (request {})", requests_made);
                break;
            }

            // Prepare for older page: ask for klines ending before the earliest we just got
            end_time = batch.first().map(|k| k.open_time.saturating_sub(1) as u64); // avoid overlap

            remaining = remaining.saturating_sub(batch.len());
            all_klines.append(&mut batch);

            // Small pause to avoid rate limits (100ms = ~600 requests/min, well under 1200/min limit)
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        if all_klines.is_empty() {
            anyhow::bail!(
                "No data returned for symbol '{}' with interval '{}'. Check that the symbol exists.",
                symbol, interval
            );
        }

        // Oldest -> newest and dedupe any overlap
        all_klines.sort_by_key(|k| k.open_time);
        all_klines.dedup_by_key(|k| k.open_time);

        tracing::info!(
            "Fetched {} candles for {} in {} API requests",
            all_klines.len(), symbol, requests_made
        );

        // Convert to DataFrame
        let df = self.klines_to_dataframe(all_klines)?;

        // Validate data
        self.validate_dataframe(&df, symbol)?;

        Ok(df)
    }

    /// Fetch data and cache it locally.
    ///
    /// This will first try to load from cache. If not found or stale,
    /// it will fetch from Binance and save to cache.
    pub async fn fetch_with_cache(
        &self,
        symbol: &str,
        interval: &str,
        total_candles: u16,
    ) -> Result<DataFrame> {
        // Try cache first
        if let Some(cached) = self.load_from_cache(symbol, interval)? {
            if cached.height() >= total_candles as usize {
                // Return cached data (sliced to requested size)
                return Ok(cached.slice((cached.height() - total_candles as usize) as i64, total_candles as usize));
            }
        }

        // Fetch fresh data
        let df = self.fetch_data(symbol, interval, total_candles).await?;

        // Save to cache
        self.save_to_cache(symbol, interval, &df)?;

        Ok(df)
    }

    /// Convert raw klines to a Polars DataFrame.
    fn klines_to_dataframe(&self, klines: Vec<binance::rest_model::KlineSummary>) -> Result<DataFrame> {
        let n = klines.len();
        let mut open_times = Vec::with_capacity(n);
        let mut opens = Vec::with_capacity(n);
        let mut highs = Vec::with_capacity(n);
        let mut lows = Vec::with_capacity(n);
        let mut closes = Vec::with_capacity(n);
        let mut volumes = Vec::with_capacity(n);

        for k in klines {
            let secs = k.open_time / 1000;
            let nsecs = ((k.open_time % 1000) * 1_000_000) as u32;
            if let Some(dt) = DateTime::from_timestamp(secs, nsecs) {
                open_times.push(dt.naive_utc());
            }
            opens.push(k.open);
            highs.push(k.high);
            lows.push(k.low);
            closes.push(k.close);
            volumes.push(k.volume);
        }

        let df = df!(
            "time" => open_times,
            "open" => opens,
            "high" => highs,
            "low" => lows,
            "close" => closes,
            "volume" => volumes
        )?;

        Ok(df)
    }

    /// Validate DataFrame has expected structure and no obvious issues.
    fn validate_dataframe(&self, df: &DataFrame, symbol: &str) -> Result<()> {
        // Check required columns
        let required = ["time", "open", "high", "low", "close", "volume"];
        for col in &required {
            df.column(col)
                .with_context(|| format!("Missing required column '{}' in data for {}", col, symbol))?;
        }

        // Check for reasonable price values (not zero/negative)
        let close = df.column("close")?;
        let close_f64 = close.f64()
            .context("Close column should be f64")?;

        for (i, val) in close_f64.into_iter().enumerate() {
            if let Some(v) = val {
                if v <= 0.0 {
                    anyhow::bail!(
                        "Invalid close price at index {}: {} (symbol: {})",
                        i, v, symbol
                    );
                }
            }
        }

        // Check time ordering
        let time = df.column("time")?;
        let time_dt = time.datetime()
            .context("Time column should be datetime")?;

        for i in 1..time_dt.len() {
            let prev = time_dt.get(i - 1);
            let curr = time_dt.get(i);
            if let (Some(p), Some(c)) = (prev, curr) {
                if c <= p {
                    tracing::warn!(
                        "Time not strictly increasing at index {} for {} (prev: {}, curr: {})",
                        i, symbol, p, c
                    );
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_format() {
        let loader = DataLoader::new(None, None);
        let key = loader.cache_key("BTCUSDT", "1h");
        assert!(key.contains("btcusdt"));
        assert!(key.contains("1h"));
        assert!(key.ends_with(".parquet"));
    }

    #[test]
    fn test_cache_config_default() {
        let config = CacheConfig::default();
        assert!(config.enabled);
        assert_eq!(config.cache_dir, "data/cache");
    }
}

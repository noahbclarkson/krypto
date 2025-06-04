use binance::market::Market;
use serde::{Deserialize, Serialize}; // Added for caching
use tracing::{debug, error, info, instrument, warn};

use crate::{
    config::KryptoConfig,
    data::{candlestick::Candlestick, interval::Interval, technicals::Technicals},
    error::KryptoError,
    util::date_utils::{date_to_datetime, get_timestamps},
};

use super::cache::{load_from_cache, save_to_cache}; // Import cache functions

/// Represents the processed data for a single symbol at a specific interval,
/// ready for caching or direct use.
#[derive(Debug, Clone, Serialize, Deserialize)] // Added Serialize, Deserialize
pub struct RawSymbolData {
    symbol: String,
    interval: Interval,
    candles: Vec<Candlestick>,
    technicals: Vec<Technicals>,
    labels: Vec<f64>, // Store raw percentage change? Or signum? Storing raw allows flexibility.
                      // Add metadata like last updated time?
                      // last_updated: DateTime<Utc>,
}

/// Represents the final features (X), labels (y), and corresponding candles
/// prepared for input into a machine learning model or backtester for a specific symbol.
#[derive(Debug, Clone)] // Added Debug, Clone
pub struct SymbolDataset {
    features: Vec<Vec<f64>>,
    labels: Vec<f64>, // Should be the target variable (e.g., signum of next change)
    candles: Vec<Candlestick>, // Candles corresponding to the labels
}

impl SymbolDataset {
    pub fn new(features: Vec<Vec<f64>>, labels: Vec<f64>, candles: Vec<Candlestick>) -> Self {
        // Add validation here?
        // assert_eq!(features.len(), labels.len());
        // assert_eq!(features.len(), candles.len());
        Self {
            features,
            labels,
            candles,
        }
    }

    pub fn get_features(&self) -> &Vec<Vec<f64>> {
        &self.features
    }

    pub fn get_labels(&self) -> &Vec<f64> {
        &self.labels
    }

    pub fn get_candles(&self) -> &Vec<Candlestick> {
        &self.candles
    }

    /// Returns the number of samples (rows) in the dataset.
    /// Returns error if internal lengths are inconsistent.
    pub fn len(&self) -> Result<usize, KryptoError> {
        let len = self.features.len();
        if self.labels.len() == len && self.candles.len() == len {
            Ok(len)
        } else {
            Err(KryptoError::InvalidDatasetLengths(
                len,
                self.labels.len(),
                self.candles.len(),
            ))
        }
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty() // Assuming all vecs have same length if not empty
    }
}

impl RawSymbolData {
    /// Loads data for a symbol/interval, utilizing cache if enabled and valid.
    #[instrument(skip(config, market), fields(symbol=symbol, interval=%interval))]
    pub async fn load(
        interval: Interval,
        symbol: &str,
        end_fetch_time: i64, // Use consistent end time for all symbols in an IntervalData load
        config: &KryptoConfig,
        market: &Market, // Pass market client
    ) -> Result<Self, KryptoError> {
        // 1. Try loading from cache
        if config.cache_enabled {
            match load_from_cache(config, symbol, &interval) {
                Ok(Some(cached_data)) => {
                    // Optional: Check if cache is up-to-date enough, e.g., based on last candle time
                    // For simplicity now, just return cached data if found and deserialized correctly.
                    // Need to handle incremental updates later if required for trading.
                    return Ok(cached_data);
                }
                Ok(None) => {
                    // Cache miss or invalid, proceed to fetch
                    debug!("Cache miss for {} {}, fetching from API.", symbol, interval);
                }
                Err(e) => {
                    // Log cache load error but proceed to fetch
                    warn!(
                        "Error loading cache for {} {}: {}. Fetching from API.",
                        symbol, interval, e
                    );
                }
            }
        }

        // 2. Fetch from API if cache missed or disabled
        info!("Fetching data from Binance API for {} {}", symbol, interval);
        let start_time = date_to_datetime(&config.start_date)?.timestamp_millis();
        let timestamps = get_timestamps(start_time, end_fetch_time, interval)?;

        let mut candles = Vec::new();
        let chunk_futures = timestamps
            .into_iter()
            .map(|(start, end)| Self::load_chunk(market, symbol, &interval, start, end));

        let chunk_results = futures::future::join_all(chunk_futures).await;

        for result in chunk_results {
            match result {
                Ok(mut chunk) => candles.append(&mut chunk),
                Err(e) => {
                    // Log error but continue processing other chunks? Or fail fast?
                    // Failing fast might be safer if partial data is unusable.
                    error!("Failed to load a chunk for {} {}: {}", symbol, interval, e);
                    return Err(e);
                }
            }
        }

        if candles.is_empty() {
            warn!("No candles loaded from API for {} {}", symbol, interval);
            // Return empty data or error? Returning empty allows processing to continue for other symbols.
            return Ok(Self {
                symbol: symbol.to_string(),
                interval,
                candles: vec![],
                technicals: vec![],
                labels: vec![],
            });
        }

        // Sort and deduplicate candles
        candles.sort_unstable(); // Use unstable sort as order is based on time
        candles.dedup_by_key(|c| c.open_time);

        // 3. Calculate Technicals and Labels
        let technicals = Technicals::get_technicals(&candles, &config.technicals)?;
        let labels = Self::calculate_labels(&candles);

        // Ensure consistency
        if candles.len() != technicals.len() || candles.len() != labels.len() {
            debug!(
                "Inconsistent lengths after loading data for {} {}: Candles: {}, Labels: {}, Technicals: {}",
                symbol, interval, candles.len(), labels.len(), technicals.len()
            );
            return Err(KryptoError::InvalidDatasetLengths(
                candles.len(),
                labels.len(),
                technicals.len(),
            ));
        }

        let loaded_data = Self {
            symbol: symbol.to_string(),
            interval,
            candles,
            technicals,
            labels,
        };

        debug!(
            "Loaded {} candles ({} labels, {} technical sets) for {} {}",
            loaded_data.candles.len(),
            loaded_data.labels.len(),
            loaded_data.technicals.len(),
            symbol,
            interval
        );

        // 4. Save to cache if enabled
        if config.cache_enabled {
            if let Err(e) = save_to_cache(config, symbol, &interval, &loaded_data) {
                // Log cache save error but don't fail the overall load
                warn!(
                    "Failed to save data to cache for {} {}: {}",
                    symbol, interval, e
                );
            }
        }

        Ok(loaded_data)
    }

    /// Fetches a single chunk (up to 1000 klines) from Binance.
    async fn load_chunk(
        market: &Market,
        symbol: &str,
        interval: &Interval,
        start_time: i64,
        end_time: i64,
    ) -> Result<Vec<Candlestick>, KryptoError> {
        debug!(
            "Loading chunk for {} {} from {} to {}",
            symbol, interval, start_time, end_time
        );
        let summaries = market
            .get_klines(
                symbol,
                interval.to_string(),
                Some(1000), // Limit
                Some(start_time as u64),
                Some(end_time as u64),
            )
            .await
            .map_err(|e| KryptoError::BinanceApiError(e.to_string()))?;

        Candlestick::map_to_candlesticks(summaries, symbol, interval)
    }

    /// Calculates labels (raw percentage change) for the given candles.
    /// Label `i` represents the change from candle `i-1`'s close to candle `i`'s close.
    /// The first label is always 0.0.
    fn calculate_labels(candles: &[Candlestick]) -> Vec<f64> {
        if candles.is_empty() {
            return Vec::new();
        }

        let mut labels = vec![0.0]; // First label has no prior candle
        labels.reserve(candles.len() - 1);

        for i in 1..candles.len() {
            let prev_close = candles[i - 1].close;
            let current_close = candles[i].close;

            let percentage_change = if prev_close.abs() > f64::EPSILON {
                (current_close - prev_close) / prev_close
            } else {
                0.0 // Avoid division by zero if previous close was 0
            };
            // Handle potential NaN/Infinity from calculation
            labels.push(if percentage_change.is_finite() {
                percentage_change
            } else {
                0.0
            });
        }
        labels
    }

    pub fn len(&self) -> usize {
        self.candles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candles.is_empty() // Assuming consistency after load
    }

    pub fn get_candles(&self) -> &Vec<Candlestick> {
        &self.candles
    }

    pub fn get_technicals(&self) -> &Vec<Technicals> {
        &self.technicals
    }

    /// Returns the raw labels (percentage change). Use `.signum()` if needed for classification.
    pub fn get_labels(&self) -> &Vec<f64> {
        &self.labels
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    pub fn interval(&self) -> Interval {
        self.interval
    }

    /// Recomputes technical indicators based on a new list of names.
    /// Modifies the internal state.
    pub fn recompute_technicals(&mut self, technical_names: &[String]) -> Result<(), KryptoError> {
        debug!(
            "Recomputing technicals for {} {} with: {:?}",
            self.symbol, self.interval, technical_names
        );
        self.technicals = Technicals::get_technicals(&self.candles, technical_names)?;
        // Ensure consistency again after recomputation
        if self.candles.len() != self.technicals.len() {
            // This indicates a bug in Technicals::get_technicals if it happens
            error!(
                "Inconsistent technicals length after recomputation for {} {}",
                self.symbol, self.interval
            );
            return Err(KryptoError::InvalidDatasetLengths(
                self.candles.len(),
                self.labels.len(),
                self.technicals.len(),
            ));
        }
        Ok(())
    }
}

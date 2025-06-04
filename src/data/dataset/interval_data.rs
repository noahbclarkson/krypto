use std::collections::HashMap;

use binance::market::Market;
use chrono::Utc;
use tracing::{debug, instrument, warn};

use crate::{
    algorithm::algo::AlgorithmSettings,
    config::KryptoConfig,
    data::{candlestick::Candlestick, interval::Interval},
    error::KryptoError,
};

use super::{
    get_normalized_predictors, get_records,
    symbol_data::{RawSymbolData, SymbolDataset},
};

/**
The dataset for a given interval. This contains all the data for all symbols at the given interval.
 */
#[derive(Debug)] // Added Debug derive
pub struct IntervalData {
    interval: Interval, // Store the interval for context
    symbol_data_map: HashMap<String, RawSymbolData>,
    // Cache normalized predictors for the *initial* set of technicals
    // This needs recalculation if technicals change via get_specific_tickers_and_technicals
    normalized_predictors: Vec<Vec<f64>>,
}

impl IntervalData {
    #[instrument(skip(config))]
    pub async fn load(interval: Interval, config: &KryptoConfig) -> Result<Self, KryptoError> {
        let market: Market = config.get_binance();
        // Use current time as end for fetching latest data
        let end_fetch_time = Utc::now().timestamp_millis();

        let tasks = config.symbols.iter().map(|symbol| {
            let market_clone = market.clone(); // Clone market for async task
            let config_clone = config.clone(); // Clone config for async task
            async move {
                RawSymbolData::load(
                    interval,
                    symbol,
                    end_fetch_time,
                    &config_clone,
                    &market_clone, // Pass reference to cloned market
                )
                .await
            }
        });

        let results = futures::future::try_join_all(tasks).await?;

        let symbol_data_map: HashMap<String, RawSymbolData> = results
            .into_iter()
            .map(|data| (data.symbol().to_string(), data))
            .collect();

        if symbol_data_map.is_empty() {
            return Err(KryptoError::InsufficientData {
                got: 0,
                required: 1,
                context: format!("No symbol data could be loaded for interval {}", interval),
            });
        }
        if symbol_data_map.values().any(|d| d.is_empty()) {
            warn!("Some symbols have empty data for interval {}", interval);
            // Decide if this should be an error or just a warning
        }

        let records = get_records(&symbol_data_map)?;
        let normalized_predictors = get_normalized_predictors(records);

        Ok(Self {
            interval,
            symbol_data_map,
            normalized_predictors,
        })
    }

    pub fn interval(&self) -> Interval {
        self.interval
    }

    pub fn len(&self) -> usize {
        self.symbol_data_map.len()
    }

    pub fn get(&self, symbol: &str) -> Option<&RawSymbolData> {
        self.symbol_data_map.get(symbol)
    }

    pub fn values(&self) -> impl Iterator<Item = &RawSymbolData> {
        self.symbol_data_map.values()
    }

    pub fn is_empty(&self) -> bool {
        self.symbol_data_map.is_empty()
    }

    // --- Accessors for underlying data (consider removing if get_symbol_dataset is sufficient) ---

    // pub fn get_labels(&self, symbol: &str) -> Result<&Vec<f64>, KryptoError> {
    //     self.get(symbol)
    //         .map(|data| data.get_labels())
    //         .ok_or_else(|| KryptoError::SymbolNotFound(symbol.to_string()))
    // }

    // pub fn get_candles(&self, symbol: &str) -> Result<&Vec<Candlestick>, KryptoError> {
    //      self.get(symbol)
    //         .map(|data| data.get_candles())
    //         .ok_or_else(|| KryptoError::SymbolNotFound(symbol.to_string()))
    // }

    // pub fn get_technicals(&self, symbol: &str) -> Result<&Vec<Technicals>, KryptoError> {
    //      self.get(symbol)
    //         .map(|data| data.get_technicals())
    //         .ok_or_else(|| KryptoError::SymbolNotFound(symbol.to_string()))
    // }

    /**
    Get the dataset tailored for a specific algorithm setting (symbol, depth).
    This constructs the feature matrix (X) and target vector (y) for model training/prediction.

    Features (X): For each time step `t`, the features are the normalized technical indicators
                  for *all* symbols in this `IntervalData` instance, concatenated across
                  the lookback window defined by `settings.depth`.
                  Shape: `[num_samples, depth * num_symbols * num_technicals]`

    Labels (y): The target variable for the specific `settings.symbol` at time `t + 1`.
                Typically the sign (+1, -1, 0) of the next candle's close price change.
                Shape: `[num_samples]`

    Candles: The candlesticks corresponding to the labels for `settings.symbol`, starting
             from `settings.depth`. Used for backtesting simulation.
             Shape: `[num_samples]`

    ## Arguments
    * `settings` - The algorithm settings specifying the target symbol and lookback depth.

    ## Returns
    A `SymbolDataset` containing the features, labels, and candles, or an error if data is insufficient.
     */
    #[instrument(skip(self, settings), fields(symbol=%settings.symbol, depth=settings.depth, interval=%self.interval))]
    pub fn get_symbol_dataset(
        &self,
        settings: &AlgorithmSettings,
    ) -> Result<SymbolDataset, KryptoError> {
        let num_predictors = self.normalized_predictors.len();
        let required_length = settings.depth + 1; // Need depth for features + 1 for the label

        if num_predictors < required_length {
            return Err(KryptoError::InsufficientData {
                got: num_predictors,
                required: required_length,
                context: format!(
                    "Not enough normalized predictor data points for symbol {} on interval {} with depth {}",
                    settings.symbol, self.interval, settings.depth
                ),
            });
        }

        // Create features by windowing and flattening
        // `windows` creates overlapping slices. We want features at time `t` to predict label at `t+1`.
        // Window `t-depth+1` to `t` gives features for predicting `t+1`.
        let features: Vec<Vec<f64>> = self
            .normalized_predictors
            .windows(settings.depth) // Creates windows of size `depth`
            .map(|window| window.iter().flatten().cloned().collect())
            .collect();

        // Features[i] corresponds to data from candles [i, i+depth-1]
        // We need to predict the label for candle i+depth.
        // So, we take features up to the second-to-last possible window.
        let features = features[..features.len() - 1].to_vec(); // Shape: [num_samples, num_flat_features]

        let symbol_data = self
            .get(&settings.symbol)
            .ok_or_else(|| KryptoError::SymbolNotFound(settings.symbol.clone()))?;

        // Labels start from index `depth` because the first `depth` candles are used for the first feature set.
        // Label[i] (original index i+depth) corresponds to Features[i]
        let labels: Vec<f64> = symbol_data
            .get_labels()
            .iter()
            .skip(settings.depth) // Skip labels corresponding to initial feature buildup
            .map(|&v| if v.is_nan() { 0.0 } else { v.signum() }) // Use signum, handle NaN as 0 (neutral)
            .collect();

        // Candles also start from index `depth` to align with labels and feature windows.
        let candles: Vec<Candlestick> = symbol_data
            .get_candles()
            .iter()
            .skip(settings.depth)
            .cloned()
            .collect();

        // Final length check after alignment
        let num_samples = features.len();
        if num_samples == 0 {
            return Err(KryptoError::InsufficientData {
                got: 0,
                required: 1,
                context: format!(
                    "No samples generated after windowing for symbol {} on interval {} with depth {}",
                    settings.symbol, self.interval, settings.depth
                ),
            });
        }
        if labels.len() != num_samples || candles.len() != num_samples {
            debug!(
                "Mismatch in lengths: Features: {}, Labels: {}, Candles: {}",
                num_samples, labels.len(), candles.len()
            );
            return Err(KryptoError::InvalidDatasetLengths(
                num_samples,
                labels.len(),
                candles.len(),
            ));
        }

        let feature_dim = features.first().map_or(0, |f| f.len());
        debug!(
            "Generated SymbolDataset for {}: Features: {}x{}, Labels: {}, Candles: {}",
            settings.symbol,
            num_samples,
            feature_dim, // features[0].len()
            labels.len(),
            candles.len()
        );

        Ok(SymbolDataset::new(features, labels, candles))
    }

    /// Creates a *new* `IntervalData` instance containing only the specified tickers
    /// and recomputed technicals based on the `new_technicals` list.
    /// Note: This is potentially expensive as it recomputes technicals and normalization.
    #[instrument(skip(self, tickers, new_technicals))]
    pub fn get_specific_tickers_and_technicals(
        &self,
        tickers: &[String],
        new_technicals: &[String],
    ) -> Result<Self, KryptoError> {
        if tickers.is_empty() {
            return Err(KryptoError::ConfigError(
                "Tickers list cannot be empty for filtering.".to_string(),
            ));
        }
        if new_technicals.is_empty() {
            return Err(KryptoError::ConfigError(
                "New technicals list cannot be empty for recomputing.".to_string(),
            ));
        }

        let mut new_symbol_data_map: HashMap<String, RawSymbolData> = HashMap::new();

        for ticker in tickers {
            if let Some(symbol_data) = self.symbol_data_map.get(ticker) {
                let mut new_data = symbol_data.clone();
                // Recompute technicals for this specific symbol data
                new_data.recompute_technicals(new_technicals)?; // Pass error up
                new_symbol_data_map.insert(ticker.clone(), new_data);
            } else {
                // Warn or error if a requested ticker wasn't originally loaded?
                warn!(
                    "Requested ticker {} not found in original IntervalData for interval {}",
                    ticker, self.interval
                );
            }
        }

        if new_symbol_data_map.is_empty() {
            return Err(KryptoError::InsufficientData {
                got: 0,
                required: 1,
                context: format!(
                    "No requested tickers found or processed for interval {}",
                    self.interval
                ),
            });
        }

        // Recompute the normalized predictors with the new symbol data and technicals
        let records = get_records(&new_symbol_data_map)?;
        let normalized_predictors = get_normalized_predictors(records);

        Ok(Self {
            interval: self.interval, // Keep the same interval
            symbol_data_map: new_symbol_data_map,
            normalized_predictors,
        })
    }
}

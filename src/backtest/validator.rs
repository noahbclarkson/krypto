//! Lower-interval stop validation for more accurate backtesting.
//!
//! When backtesting on higher timeframes (e.g., 1h candles), we cannot determine
//! whether a stop loss was actually triggered within that candle. This module
//! provides validation by fetching lower-interval data (e.g., 5m) to check
//! intra-bar price action.
//!
//! # Problem
//!
//! Consider a 1h candle with:
//! - Open: 100
//! - High: 105
//! - Low: 95
//! - Close: 102
//!
//! If we have a trailing stop at 97, the backtester sees the low (95) and assumes
//! the stop was hit. But what if price went 100 → 105 → 102 → 95? The stop at 97
//! would have triggered on the way down, not at the low.
//!
//! # Solution
//!
//! Fetch 5m (or other lower-interval) data for the same period and walk through
//! each candle to determine the exact sequence of price movements.
//!
//! # Example
//!
//! ```rust,no_run
//! use krypto::backtest::validator::{LowerIntervalValidator, ValidatorConfig, PositionDirection};
//! use krypto::data::loader::DataLoader;
//!
//! async fn run() -> anyhow::Result<()> {
//!     let loader = DataLoader::new(None, None);
//!     let validator = LowerIntervalValidator::with_defaults(loader);
//!
//!     let result = validator.validate_candle(
//!         "BTCUSDT",
//!         1700000000000, // candle start in ms
//!         3600_000,      // 1h duration in ms
//!         PositionDirection::Long,
//!         Some(95.0),
//!         None,
//!     ).await?;
//!
//!     if result.triggered {
//!         println!("Stop hit at {}", result.trigger_price.unwrap());
//!     }
//!     Ok(())
//! }
//! ```

use anyhow::Result;
use polars::prelude::*;

/// A candle entry for batch validation: (start_ms, duration_ms, direction, stop, take_profit)
pub type BatchCandle = (i64, u64, PositionDirection, Option<f64>, Option<f64>);

/// Represents a price level that can trigger an exit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExitLevel {
    /// Stop loss price (exit to limit losses).
    StopLoss(f64),
    /// Take profit price (exit to secure gains).
    TakeProfit(f64),
}

/// Direction of the position being validated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionDirection {
    /// Long position: stop is below entry, TP is above.
    Long,
    /// Short position: stop is above entry, TP is below.
    Short,
}

/// Result of validating a single higher-timeframe candle.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether any exit level was triggered.
    pub triggered: bool,
    /// Which level was triggered (if any).
    pub trigger_level: Option<ExitLevel>,
    /// Exact price at which the exit occurred.
    pub trigger_price: Option<f64>,
    /// Timestamp when the exit occurred (ms).
    pub trigger_time_ms: Option<i64>,
    /// Index of the lower-interval candle that triggered the exit.
    pub trigger_candle_index: Option<usize>,
    /// Whether this was a gap open scenario (price opened beyond the level).
    pub is_gap_open: bool,
}

impl ValidationResult {
    /// No exit was triggered within this candle.
    pub fn no_trigger() -> Self {
        Self {
            triggered: false,
            trigger_level: None,
            trigger_price: None,
            trigger_time_ms: None,
            trigger_candle_index: None,
            is_gap_open: false,
        }
    }

    /// Exit was triggered.
    fn triggered(
        level: ExitLevel,
        price: f64,
        time_ms: i64,
        candle_idx: usize,
        is_gap_open: bool,
    ) -> Self {
        Self {
            triggered: true,
            trigger_level: Some(level),
            trigger_price: Some(price),
            trigger_time_ms: Some(time_ms),
            trigger_candle_index: Some(candle_idx),
            is_gap_open,
        }
    }
}

/// Configuration for the lower-interval validator.
#[derive(Debug, Clone)]
pub struct ValidatorConfig {
    /// Lower timeframe to use for validation (e.g., "5m", "15m").
    pub lower_interval: String,
    /// Duration in milliseconds of one lower-interval candle (for time range calculation).
    pub lower_interval_ms: u64,
    /// Whether to treat gap opens as triggered stops.
    pub gap_opens_trigger_stops: bool,
    /// Whether to treat gap opens as triggered take profits.
    pub gap_opens_trigger_tp: bool,
    /// Maximum number of lower-interval candles to fetch per request.
    pub max_candles_per_fetch: u16,
    /// Behavior when lower-interval data is unavailable.
    pub on_missing_data: MissingDataBehavior,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            lower_interval: "5m".to_string(),
            lower_interval_ms: 5 * 60 * 1000,
            gap_opens_trigger_stops: true,
            gap_opens_trigger_tp: true,
            max_candles_per_fetch: 1000,
            on_missing_data: MissingDataBehavior::Conservative,
        }
    }
}

impl ValidatorConfig {
    /// Create a config for 1m lower interval.
    pub fn one_minute() -> Self {
        Self {
            lower_interval: "1m".to_string(),
            lower_interval_ms: 60 * 1000,
            ..Default::default()
        }
    }

    /// Create a config for 15m lower interval.
    pub fn fifteen_minute() -> Self {
        Self {
            lower_interval: "15m".to_string(),
            lower_interval_ms: 15 * 60 * 1000,
            ..Default::default()
        }
    }
}

/// How to handle missing lower-interval data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MissingDataBehavior {
    /// Assume worst case: stop was triggered (conservative for backtesting).
    Conservative,
    /// Assume best case: no stop was triggered (optimistic for backtesting).
    Optimistic,
    /// Return an error when data is missing.
    Error,
    /// Skip validation for this candle (return no_trigger with a warning log).
    SkipWithWarning,
}

/// Main validator struct for lower-interval stop validation.
pub struct LowerIntervalValidator {
    /// Data loader for fetching lower-interval candles.
    loader: crate::data::DataLoader,
    /// Configuration for validation behavior.
    config: ValidatorConfig,
    /// Cache of fetched lower-interval data.
    /// Key: (symbol, higher_candle_start_ms) → DataFrame of lower-interval candles
    cache: std::collections::HashMap<(String, i64), DataFrame>,
}

impl LowerIntervalValidator {
    /// Create a new validator with the given data loader and configuration.
    pub fn new(loader: crate::data::DataLoader, config: ValidatorConfig) -> Self {
        Self {
            loader,
            config,
            cache: std::collections::HashMap::new(),
        }
    }

    /// Create a validator with default configuration (5m lower interval).
    pub fn with_defaults(loader: crate::data::DataLoader) -> Self {
        Self::new(loader, ValidatorConfig::default())
    }

    /// Validate a single higher-timeframe candle.
    ///
    /// Fetches lower-interval data for the candle's time range and determines
    /// if the stop loss or take profit was triggered.
    ///
    /// # Arguments
    ///
    /// * `symbol` - Trading pair (e.g., "BTCUSDT")
    /// * `candle_start_ms` - Unix timestamp (ms) of the higher-timeframe candle start
    /// * `candle_duration_ms` - Duration of the higher candle in ms (e.g., 3600000 for 1h)
    /// * `direction` - Long or short position
    /// * `stop_price` - Stop loss price level (optional)
    /// * `take_profit_price` - Take profit price level (optional)
    pub async fn validate_candle(
        &self,
        symbol: &str,
        candle_start_ms: i64,
        candle_duration_ms: u64,
        direction: PositionDirection,
        stop_price: Option<f64>,
        take_profit_price: Option<f64>,
    ) -> Result<ValidationResult> {
        if stop_price.is_none() && take_profit_price.is_none() {
            return Ok(ValidationResult::no_trigger());
        }

        let start_ms = candle_start_ms as u64;
        let end_ms = start_ms + candle_duration_ms;

        // Fetch lower-interval data
        let lower_df = match self
            .fetch_lower_interval_data(symbol, start_ms, end_ms)
            .await
        {
            Ok(df) => df,
            Err(e) => {
                return self.handle_missing_data(e, stop_price);
            }
        };

        self.scan_for_trigger(&lower_df, direction, stop_price, take_profit_price)
    }

    /// Validate multiple candles in batch.
    ///
    /// More efficient than calling `validate_candle` multiple times because
    /// it fetches a larger chunk of lower-interval data at once.
    pub async fn validate_candles(
        &self,
        symbol: &str,
        candles: &[BatchCandle],
    ) -> Result<Vec<ValidationResult>> {
        if candles.is_empty() {
            return Ok(vec![]);
        }

        // Calculate overall time range
        let overall_start = candles
            .iter()
            .map(|(t, _, _, _, _)| *t as u64)
            .min()
            .unwrap_or(0);
        let overall_end = candles
            .iter()
            .map(|(t, dur, _, _, _)| *t as u64 + *dur)
            .max()
            .unwrap_or(0);

        // Fetch all lower-interval data in one request
        let lower_df = match self
            .fetch_lower_interval_data(symbol, overall_start, overall_end)
            .await
        {
            Ok(df) => df,
            Err(e) => {
                tracing::warn!("Failed to fetch lower interval data for batch: {}", e);
                // Return conservative/optimistic results for all
                return Ok(candles
                    .iter()
                    .map(|(_, _, _, stop, _)| {
                        self.handle_missing_data(anyhow::anyhow!("No data"), *stop)
                            .unwrap_or_else(|_| ValidationResult::no_trigger())
                    })
                    .collect());
            }
        };

        // Extract time column
        let time_col = lower_df.column("time")?.datetime()?;

        let mut results = Vec::with_capacity(candles.len());
        for (candle_start_ms, candle_dur_ms, direction, stop, tp) in candles {
            let start_ms = *candle_start_ms;
            let end_ms = start_ms + *candle_dur_ms as i64;

            // Filter lower_df to this candle's window
            let mask = time_col
                .into_iter()
                .map(|t| t.map(|v| v >= start_ms && v < end_ms).unwrap_or(false))
                .collect::<BooleanChunked>();

            let candle_df = lower_df.filter(&mask)?;

            if candle_df.is_empty() {
                results.push(
                    self.handle_missing_data(anyhow::anyhow!("No lower data for candle"), *stop)
                        .unwrap_or_else(|_| ValidationResult::no_trigger()),
                );
            } else {
                results.push(self.scan_for_trigger(&candle_df, *direction, *stop, *tp)?);
            }
        }

        Ok(results)
    }

    /// Scan a DataFrame of lower-interval candles for a stop/TP trigger.
    fn scan_for_trigger(
        &self,
        df: &DataFrame,
        direction: PositionDirection,
        stop_price: Option<f64>,
        take_profit_price: Option<f64>,
    ) -> Result<ValidationResult> {
        let opens = df.column("open")?.f64()?;
        let highs = df.column("high")?.f64()?;
        let lows = df.column("low")?.f64()?;
        let closes = df.column("close")?.f64()?;
        let times = df.column("time")?.datetime()?;

        for i in 0..df.height() {
            let open = opens.get(i).unwrap_or(f64::NAN);
            let high = highs.get(i).unwrap_or(f64::NAN);
            let low = lows.get(i).unwrap_or(f64::NAN);
            let close = closes.get(i).unwrap_or(f64::NAN);
            let time_ms = times.get(i).unwrap_or(0);

            if open.is_nan() || high.is_nan() || low.is_nan() || close.is_nan() {
                continue;
            }

            if let Some((level, price, is_gap)) =
                self.check_candle_trigger(open, high, low, direction, stop_price, take_profit_price)
            {
                return Ok(ValidationResult::triggered(
                    level, price, time_ms, i, is_gap,
                ));
            }
        }

        Ok(ValidationResult::no_trigger())
    }

    /// Check if a single lower-interval candle triggers an exit.
    ///
    /// Returns `Some((ExitLevel, fill_price, is_gap_open))` if triggered.
    ///
    /// # Edge Cases
    ///
    /// - **Gap open**: If open is beyond stop/TP, fill at open price
    /// - **Both hit in same candle**: Assume stop hit first (conservative)
    fn check_candle_trigger(
        &self,
        open: f64,
        high: f64,
        low: f64,
        direction: PositionDirection,
        stop_price: Option<f64>,
        take_profit_price: Option<f64>,
    ) -> Option<(ExitLevel, f64, bool)> {
        let mut stop_hit: Option<(ExitLevel, f64, bool)> = None;
        let mut tp_hit: Option<(ExitLevel, f64, bool)> = None;

        match direction {
            PositionDirection::Long => {
                // Long: stop triggers when price falls to/below stop, TP when rises to/above TP
                if let Some(stop) = stop_price {
                    if low <= stop {
                        let is_gap = open < stop;
                        let fill = if is_gap && self.config.gap_opens_trigger_stops {
                            open // gapped through — fill at open
                        } else if is_gap {
                            return None; // gap but not configured to trigger
                        } else {
                            stop
                        };
                        stop_hit = Some((ExitLevel::StopLoss(stop), fill, is_gap));
                    }
                }
                if let Some(tp) = take_profit_price {
                    if high >= tp {
                        let is_gap = open > tp;
                        let fill = if is_gap && self.config.gap_opens_trigger_tp {
                            open
                        } else if is_gap {
                            return None;
                        } else {
                            tp
                        };
                        tp_hit = Some((ExitLevel::TakeProfit(tp), fill, is_gap));
                    }
                }
            }
            PositionDirection::Short => {
                // Short: stop triggers when price rises to/above stop, TP when falls to/below TP
                if let Some(stop) = stop_price {
                    if high >= stop {
                        let is_gap = open > stop;
                        let fill = if is_gap && self.config.gap_opens_trigger_stops {
                            open
                        } else if is_gap {
                            return None;
                        } else {
                            stop
                        };
                        stop_hit = Some((ExitLevel::StopLoss(stop), fill, is_gap));
                    }
                }
                if let Some(tp) = take_profit_price {
                    if low <= tp {
                        let is_gap = open < tp;
                        let fill = if is_gap && self.config.gap_opens_trigger_tp {
                            open
                        } else if is_gap {
                            return None;
                        } else {
                            tp
                        };
                        tp_hit = Some((ExitLevel::TakeProfit(tp), fill, is_gap));
                    }
                }
            }
        }

        // If both triggered in same candle, prefer stop (conservative)
        stop_hit.or(tp_hit)
    }

    /// Fetch lower-interval data for a given time range.
    async fn fetch_lower_interval_data(
        &self,
        symbol: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<DataFrame> {
        self.loader
            .fetch_data_in_range(symbol, &self.config.lower_interval, start_ms, end_ms)
            .await
    }

    /// Handle missing data according to configured behavior.
    fn handle_missing_data(
        &self,
        err: anyhow::Error,
        stop_price: Option<f64>,
    ) -> Result<ValidationResult> {
        match self.config.on_missing_data {
            MissingDataBehavior::Conservative => {
                // Assume stop was hit
                tracing::debug!("Missing lower data (conservative): {}", err);
                let level = stop_price
                    .map(ExitLevel::StopLoss)
                    .unwrap_or(ExitLevel::StopLoss(0.0));
                let price = stop_price.unwrap_or(0.0);
                Ok(ValidationResult::triggered(level, price, 0, 0, false))
            }
            MissingDataBehavior::Optimistic => {
                tracing::debug!("Missing lower data (optimistic): {}", err);
                Ok(ValidationResult::no_trigger())
            }
            MissingDataBehavior::Error => Err(err),
            MissingDataBehavior::SkipWithWarning => {
                tracing::warn!("Missing lower data, skipping candle: {}", err);
                Ok(ValidationResult::no_trigger())
            }
        }
    }

    /// Clear the internal cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Get number of cached entries.
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
}

/// Determine which level was hit first when both stop and TP are in range.
///
/// Conservative default: assume stop hits first (protects capital).
/// Future: use tick data or statistical models for better accuracy.
pub fn determine_first_trigger(stop_price: f64, _tp_price: f64) -> ExitLevel {
    ExitLevel::StopLoss(stop_price)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_result_no_trigger() {
        let result = ValidationResult::no_trigger();
        assert!(!result.triggered);
        assert!(result.trigger_level.is_none());
        assert!(result.trigger_price.is_none());
    }

    #[test]
    fn test_validator_config_default() {
        let config = ValidatorConfig::default();
        assert_eq!(config.lower_interval, "5m");
        assert!(config.gap_opens_trigger_stops);
        assert_eq!(config.on_missing_data, MissingDataBehavior::Conservative);
    }

    #[test]
    fn test_validator_config_variants() {
        let c1m = ValidatorConfig::one_minute();
        assert_eq!(c1m.lower_interval, "1m");
        assert_eq!(c1m.lower_interval_ms, 60_000);

        let c15m = ValidatorConfig::fifteen_minute();
        assert_eq!(c15m.lower_interval, "15m");
        assert_eq!(c15m.lower_interval_ms, 15 * 60 * 1000);
    }

    /// Test the core candle trigger logic without any network calls.
    #[test]
    fn test_check_candle_trigger_long_stop_hit() {
        use crate::data::loader::DataLoader;
        let loader = DataLoader::new(None, None);
        let validator = LowerIntervalValidator::with_defaults(loader);

        // Long position, stop at 95, candle dips to 90
        let result = validator.check_candle_trigger(
            100.0,
            105.0,
            90.0,
            PositionDirection::Long,
            Some(95.0),
            None,
        );
        assert!(result.is_some());
        let (level, fill, is_gap) = result.unwrap();
        assert_eq!(level, ExitLevel::StopLoss(95.0));
        assert_eq!(fill, 95.0);
        assert!(!is_gap);
    }

    #[test]
    fn test_check_candle_trigger_long_tp_hit() {
        use crate::data::loader::DataLoader;
        let loader = DataLoader::new(None, None);
        let validator = LowerIntervalValidator::with_defaults(loader);

        // Long, TP at 110, candle reaches 115
        let result = validator.check_candle_trigger(
            100.0,
            115.0,
            98.0,
            PositionDirection::Long,
            None,
            Some(110.0),
        );
        assert!(result.is_some());
        let (level, fill, _is_gap) = result.unwrap();
        assert_eq!(level, ExitLevel::TakeProfit(110.0));
        assert_eq!(fill, 110.0);
    }

    #[test]
    fn test_check_candle_trigger_long_no_hit() {
        use crate::data::loader::DataLoader;
        let loader = DataLoader::new(None, None);
        let validator = LowerIntervalValidator::with_defaults(loader);

        // Long, stop at 90, TP at 115, candle stays 100-105
        let result = validator.check_candle_trigger(
            100.0,
            105.0,
            98.0,
            PositionDirection::Long,
            Some(90.0),
            Some(115.0),
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_check_candle_trigger_long_both_hit_prefers_stop() {
        use crate::data::loader::DataLoader;
        let loader = DataLoader::new(None, None);
        let validator = LowerIntervalValidator::with_defaults(loader);

        // Both in range — stop should win (conservative)
        let result = validator.check_candle_trigger(
            100.0,
            120.0,
            85.0,
            PositionDirection::Long,
            Some(90.0),
            Some(115.0),
        );
        assert!(result.is_some());
        let (level, _, _) = result.unwrap();
        assert!(matches!(level, ExitLevel::StopLoss(_)));
    }

    #[test]
    fn test_check_candle_trigger_long_gap_open() {
        use crate::data::loader::DataLoader;
        let loader = DataLoader::new(None, None);
        let validator = LowerIntervalValidator::with_defaults(loader);

        // Gap down: open below stop (85 < 90 stop), so fills at open
        let result = validator.check_candle_trigger(
            85.0,
            88.0,
            82.0,
            PositionDirection::Long,
            Some(90.0),
            None,
        );
        assert!(result.is_some());
        let (level, fill, is_gap) = result.unwrap();
        assert_eq!(level, ExitLevel::StopLoss(90.0));
        assert_eq!(fill, 85.0); // fills at open, not stop
        assert!(is_gap);
    }

    #[test]
    fn test_check_candle_trigger_short_stop_hit() {
        use crate::data::loader::DataLoader;
        let loader = DataLoader::new(None, None);
        let validator = LowerIntervalValidator::with_defaults(loader);

        // Short, stop at 110, candle reaches 115
        let result = validator.check_candle_trigger(
            100.0,
            115.0,
            95.0,
            PositionDirection::Short,
            Some(110.0),
            None,
        );
        assert!(result.is_some());
        let (level, fill, _) = result.unwrap();
        assert_eq!(level, ExitLevel::StopLoss(110.0));
        assert_eq!(fill, 110.0);
    }

    #[test]
    fn test_check_candle_trigger_short_tp_hit() {
        use crate::data::loader::DataLoader;
        let loader = DataLoader::new(None, None);
        let validator = LowerIntervalValidator::with_defaults(loader);

        // Short, TP at 85, candle dips to 80
        let result = validator.check_candle_trigger(
            100.0,
            102.0,
            80.0,
            PositionDirection::Short,
            None,
            Some(85.0),
        );
        assert!(result.is_some());
        let (level, fill, _) = result.unwrap();
        assert_eq!(level, ExitLevel::TakeProfit(85.0));
        assert_eq!(fill, 85.0);
    }

    #[test]
    fn test_determine_first_trigger() {
        let result = determine_first_trigger(90.0, 115.0);
        assert!(matches!(result, ExitLevel::StopLoss(p) if p == 90.0));
    }
}

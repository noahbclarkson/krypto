//! Passive execution model for FDUSD pairs with 0% maker fees.
//!
//! Instead of executing at market (paying taker fees), this module simulates
//! placing limit orders below each 1m candle's open and walking forward.
//!
//! # How it works (correct model)
//!
//! For each higher-timeframe bar (e.g., 1h):
//! 1. Iterate through each 1m candle within that bar
//! 2. At each 1m open, place limit at `open - N_ticks`
//! 3. If `low <= limit`: filled at limit (0 fees, better price!)
//! 4. If not filled: move to next 1m candle, update limit to new `open - N_ticks`
//! 5. Repeat until filled (almost 100% fill rate since most candles dip below open)
//!
//! # Why this works
//!
//! Almost every candle has intrabar movement where `low < open`. By constantly
//! updating our limit to be just below each new open, we capture that dip.
//!
//! # Example
//!
//! ```no_run
//! use krypto::backtest::passive::{PassiveExecutor, PassiveConfig, TickSize};
//! use krypto::data::loader::DataLoader;
//!
//! async fn example() -> anyhow::Result<()> {
//!     let loader = DataLoader::new(None, None);
//!     
//!     let df_1h = loader.fetch_data("BTCFDUSD", "1h", 1000).await?;
//!     let df_1m = loader.fetch_data("BTCFDUSD", "1m", 60000).await?;
//!     
//!     let signals = df_1h.column("close")?.f64()?.clone(); // Your strategy signals
//!     
//!     let config = PassiveConfig {
//!         ticks_below_open: 3,      // 3 ticks below each 1m open
//!         tick_size: TickSize::from_symbol("BTCFDUSD"), // 0.01 for BTC
//!         max_wait_bars: 60,        // Max 60 1m candles (1 hour)
//!         maker_fee: 0.0,           // FDUSD = 0%
//!     };
//!     
//!     let executor = PassiveExecutor::new(config);
//!     let fills = executor.simulate(&df_1h, &df_1m, &signals.into()).await?;
//!     
//!     println!("Fill rate: {:.1}%", fills.fill_rate * 100.0);
//!     Ok(())
//! }
//! ```

use anyhow::Result;
use polars::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tick size configuration for different symbols.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TickSize(f64);

impl TickSize {
    /// Get tick size for a symbol.
    pub fn from_symbol(symbol: &str) -> Self {
        let tick = match symbol {
            // BTC pairs - 0.01 minimum
            s if s.starts_with("BTC") => 0.01,
            // ETH pairs - 0.001 minimum
            s if s.starts_with("ETH") => 0.001,
            // SOL pairs - 0.0001 minimum
            s if s.starts_with("SOL") => 0.0001,
            // BNB pairs - 0.01 minimum
            s if s.starts_with("BNB") => 0.01,
            // XRP pairs - 0.00001 minimum
            s if s.starts_with("XRP") => 0.00001,
            // Default - 0.0001
            _ => 0.0001,
        };
        Self(tick)
    }

    /// Get the tick size value.
    pub fn value(&self) -> f64 {
        self.0
    }
}

impl Default for TickSize {
    fn default() -> Self {
        Self(0.0001)
    }
}

/// Configuration for passive limit order execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveConfig {
    /// Number of ticks below each 1m open to place limit.
    pub ticks_below_open: u32,
    /// Tick size for the symbol.
    pub tick_size: TickSize,
    /// Maximum lower-timeframe bars to wait for a fill (e.g., 60 for 1h with 1m data).
    pub max_wait_bars: usize,
    /// Maker fee rate (0.0 for FDUSD pairs).
    pub maker_fee: f64,
    /// If true, only update limit when price moves X ticks above current limit.
    pub update_threshold_ticks: Option<u32>,
}

impl Default for PassiveConfig {
    fn default() -> Self {
        Self {
            ticks_below_open: 3,
            tick_size: TickSize::default(),
            max_wait_bars: 60,
            maker_fee: 0.0,
            update_threshold_ticks: None,
        }
    }
}

/// Result of simulating passive execution over a backtest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveFillStats {
    /// Total signals generated.
    pub total_signals: usize,
    /// Signals that resulted in fills.
    pub filled_signals: usize,
    /// Signals that were skipped (no fill within timeout).
    pub skipped_signals: usize,
    /// Fill rate (filled / total).
    pub fill_rate: f64,
    /// Average price improvement in ticks (positive = better than market).
    pub avg_price_improvement_ticks: f64,
    /// Total fees saved by using maker vs taker.
    pub total_fees_saved: f64,
    /// Average bars to fill.
    pub avg_bars_to_fill: f64,
}

/// A single fill event from passive execution.
#[derive(Debug, Clone)]
pub struct FillEvent {
    /// Bar index in the higher timeframe where signal occurred.
    pub signal_bar: usize,
    /// Bar index in lower timeframe where fill occurred.
    pub fill_bar_lower: usize,
    /// Signal direction: 1.0 = long, -1.0 = short.
    pub direction: f64,
    /// Market price at signal time (higher-timeframe close).
    pub market_price: f64,
    /// Actual fill price.
    pub fill_price: f64,
    /// How many lower-timeframe bars until fill.
    pub bars_to_fill: usize,
    /// Fee paid on this fill.
    pub fee: f64,
}

/// Passive execution simulator.
pub struct PassiveExecutor {
    config: PassiveConfig,
}

impl PassiveExecutor {
    /// Create a new passive executor with the given configuration.
    pub fn new(config: PassiveConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(PassiveConfig::default())
    }

    /// Simulate passive execution on a signal series.
    ///
    /// Walks forward through lower-timeframe candles, placing limit orders
    /// below each open until filled.
    pub async fn simulate(
        &self,
        df_high: &DataFrame,
        df_low: &DataFrame,
        signals: &Series,
    ) -> Result<(Vec<FillEvent>, PassiveFillStats)> {
        let high_times = df_high.column("time")?.datetime()?;
        let high_closes = df_high.column("close")?.f64()?;

        let low_times = df_low.column("time")?.datetime()?;
        let low_opens = df_low.column("open")?.f64()?;
        let low_highs = df_low.column("high")?.f64()?;
        let low_lows = df_low.column("low")?.f64()?;

        let signals_f64 = signals.f64()?;

        let mut fills: Vec<FillEvent> = Vec::new();
        let mut total_signals = 0usize;
        let mut filled_signals = 0usize;
        let mut skipped_signals = 0usize;
        let mut price_improvements: Vec<f64> = Vec::new();
        let mut bars_to_fill_list: Vec<usize> = Vec::new();

        let tick = self.config.tick_size.value();
        let offset = self.config.ticks_below_open as f64 * tick;

        for i in 0..df_high.height() {
            let sig = signals_f64.get(i).unwrap_or(0.0);
            if sig.abs() < 0.01 {
                continue;
            }

            total_signals += 1;

            let market_price = high_closes.get(i).unwrap_or(0.0);
            if market_price <= 0.0 {
                continue;
            }

            let direction = if sig > 0.0 { 1.0 } else { -1.0 };

            // Find the time range for this higher-timeframe bar
            let bar_start = high_times.get(i).unwrap_or(0);
            let bar_end = if i + 1 < df_high.height() {
                high_times.get(i + 1).unwrap_or(i64::MAX)
            } else {
                i64::MAX
            };

            // Walk forward through lower-timeframe candles
            let mut filled = false;
            let mut fill_price = 0.0;
            let mut fill_bar_lower = 0;
            let mut bars_checked = 0;
            let mut current_limit: Option<f64> = None;

            for j in 0..df_low.height() {
                let low_time = low_times.get(j).unwrap_or(0);
                if low_time < bar_start {
                    continue;
                }
                if low_time >= bar_end {
                    break;
                }

                let open = low_opens.get(j).unwrap_or(0.0);
                let high = low_highs.get(j).unwrap_or(0.0);
                let low = low_lows.get(j).unwrap_or(0.0);

                if open <= 0.0 {
                    continue;
                }

                bars_checked += 1;
                if bars_checked > self.config.max_wait_bars {
                    break;
                }

                // Calculate limit price for this candle
                let limit_price = if direction > 0.0 {
                    // Long: place limit below open
                    open - offset
                } else {
                    // Short: place limit above open
                    open + offset
                };

                // Check if we should update the limit
                let should_update = match (current_limit, self.config.update_threshold_ticks) {
                    (None, _) => true,
                    (Some(_), None) => true,
                    (Some(prev_limit), Some(threshold)) => {
                        let threshold_value = threshold as f64 * tick;
                        if direction > 0.0 {
                            // Long: update if price moved up (open > prev_limit + threshold)
                            open > prev_limit + threshold_value
                        } else {
                            // Short: update if price moved down (open < prev_limit - threshold)
                            open < prev_limit - threshold_value
                        }
                    }
                };

                if should_update {
                    current_limit = Some(limit_price);
                }

                let limit = current_limit.unwrap_or(limit_price);

                // Check if limit would have been hit
                let hit = if direction > 0.0 {
                    // Long: fills if low <= limit
                    low <= limit
                } else {
                    // Short: fills if high >= limit
                    high >= limit
                };

                if hit {
                    filled = true;
                    fill_price = limit;
                    fill_bar_lower = j;
                    break;
                }
            }

            if filled {
                filled_signals += 1;

                // Calculate price improvement
                let improvement_ticks = if direction > 0.0 {
                    // Long: better = lower fill price
                    (market_price - fill_price) / tick
                } else {
                    // Short: better = higher fill price
                    (fill_price - market_price) / tick
                };

                price_improvements.push(improvement_ticks);
                bars_to_fill_list.push(bars_checked);

                let fee = self.config.maker_fee;

                fills.push(FillEvent {
                    signal_bar: i,
                    fill_bar_lower,
                    direction,
                    market_price,
                    fill_price,
                    bars_to_fill: bars_checked,
                    fee,
                });
            } else {
                skipped_signals += 1;
            }
        }

        let avg_improvement = if price_improvements.is_empty() {
            0.0
        } else {
            price_improvements.iter().sum::<f64>() / price_improvements.len() as f64
        };

        let avg_bars = if bars_to_fill_list.is_empty() {
            0.0
        } else {
            bars_to_fill_list.iter().sum::<usize>() as f64 / bars_to_fill_list.len() as f64
        };

        // Assume taker fee would be 0.1% for comparison
        let taker_fee = 0.001;
        let fees_saved = filled_signals as f64 * (taker_fee - self.config.maker_fee);

        let stats = PassiveFillStats {
            total_signals,
            filled_signals,
            skipped_signals,
            fill_rate: if total_signals > 0 {
                filled_signals as f64 / total_signals as f64
            } else {
                0.0
            },
            avg_price_improvement_ticks: avg_improvement,
            total_fees_saved: fees_saved,
            avg_bars_to_fill: avg_bars,
        };

        Ok((fills, stats))
    }

    /// Convert fill events to a signal series for the backtest engine.
    pub fn fills_to_signals(&self, fills: &[FillEvent], num_bars: usize) -> Series {
        let mut signals = vec![0.0f64; num_bars];

        for fill in fills {
            if fill.signal_bar < num_bars {
                signals[fill.signal_bar] = fill.direction;
            }
        }

        Series::new("signal".into(), signals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use polars::time::*;

    fn make_test_data() -> (DataFrame, DataFrame) {
        // 1h data: 2 bars
        let high_times: Vec<i64> = vec![0, 3600_000]; // 1h in ms
        let high_closes = vec![100.0, 101.0];
        let high_highs = vec![101.0, 102.0];
        let high_lows = vec![99.0, 100.0];
        let high_vols = vec![1000.0; 2];

        let high_times_dt = DatetimeChunked::from_naive_datetime(
            "time".into(),
            high_times.iter().map(|&ms| {
                chrono::DateTime::from_timestamp_millis(ms)
                    .unwrap()
                    .naive_utc()
            }),
            TimeUnit::Milliseconds,
        );

        let df_high = df!(
            "time" => high_times_dt,
            "open" => high_closes.clone(),
            "high" => high_highs,
            "low" => high_lows,
            "close" => high_closes.clone(),
            "volume" => high_vols,
        ).unwrap();

        // 1m data: 3 minutes per hour
        // Most candles have low < open, so we should get fills
        let mut low_times = Vec::new();
        let mut low_opens = Vec::new();
        let mut low_highs = Vec::new();
        let mut low_lows = Vec::new();
        let mut low_closes = Vec::new();
        let mut low_vols = Vec::new();

        for (&base_time, &close) in high_times.iter().zip(high_closes.iter()) {
            for m in 0..3 {
                low_times.push(base_time + m as i64 * 60_000);
                // Simulate typical candle: open, goes up, goes down, closes
                let open = close;
                let high = open + 0.5;
                let low = open - 0.3; // Low is below open!
                let close = open + 0.1;

                low_opens.push(open);
                low_highs.push(high);
                low_lows.push(low);
                low_closes.push(close);
                low_vols.push(100.0);
            }
        }

        let low_times_dt = DatetimeChunked::from_naive_datetime(
            "time".into(),
            low_times.iter().map(|&ms| {
                chrono::DateTime::from_timestamp_millis(ms)
                    .unwrap()
                    .naive_utc()
            }),
            TimeUnit::Milliseconds,
        );

        let df_low = df!(
            "time" => low_times_dt,
            "open" => low_opens,
            "high" => low_highs,
            "low" => low_lows,
            "close" => low_closes,
            "volume" => low_vols,
        ).unwrap();

        (df_high, df_low)
    }

    #[tokio::test]
    async fn test_high_fill_rate_long() {
        let (df_high, df_low) = make_test_data();

        // Signal: buy on bar 0
        let signals = Series::new("signal".into(), vec![1.0, 0.0]);

        let config = PassiveConfig {
            ticks_below_open: 3,
            tick_size: TickSize(0.01),
            max_wait_bars: 60,
            maker_fee: 0.0,
            update_threshold_ticks: None,
        };
        let executor = PassiveExecutor::new(config);
        let (fills, stats) = executor.simulate(&df_high, &df_low, &signals).await.unwrap();

        assert_eq!(stats.total_signals, 1);
        assert_eq!(stats.filled_signals, 1, "Should fill when low < open");
        assert!(stats.fill_rate > 0.99, "Fill rate should be near 100%");
    }

    #[tokio::test]
    async fn test_price_improvement() {
        let (df_high, df_low) = make_test_data();

        let signals = Series::new("signal".into(), vec![1.0, 0.0]);

        let config = PassiveConfig {
            ticks_below_open: 3,
            tick_size: TickSize(0.01),
            max_wait_bars: 60,
            maker_fee: 0.0,
            update_threshold_ticks: None,
        };
        let executor = PassiveExecutor::new(config);
        let (fills, stats) = executor.simulate(&df_high, &df_low, &signals).await.unwrap();

        // Fill price should be below market price for long
        assert!(!fills.is_empty());
        let fill = &fills[0];
        assert!(fill.fill_price < fill.market_price, "Long fill should be below market");
        assert!(stats.avg_price_improvement_ticks > 0.0, "Should have positive improvement");
    }

    #[tokio::test]
    async fn test_tick_size_lookup() {
        assert_eq!(TickSize::from_symbol("BTCFDUSD").value(), 0.01);
        assert_eq!(TickSize::from_symbol("ETHFDUSD").value(), 0.001);
        assert_eq!(TickSize::from_symbol("SOLFDUSD").value(), 0.0001);
        assert_eq!(TickSize::from_symbol("BNBFDUSD").value(), 0.01);
        assert_eq!(TickSize::from_symbol("XRPFDUSD").value(), 0.00001);
    }
}

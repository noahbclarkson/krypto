//! Passive execution model for FDUSD pairs with 0% maker fees.
//!
//! Instead of executing at market (paying taker fees), this module simulates
//! placing limit orders below the current price and waiting for fills.
//! On Binance FDUSD pairs, maker fees are 0%, so this can significantly
//! improve returns for strategies with slight edges.
//!
//! # How it works
//!
//! 1. Signal fires on 1h/4h candle close
//! 2. Instead of market buy, place limit at `close * (1 - tick_offset)`
//! 3. Scan 1m candles within that bar:
//!    - If `low <= limit_price`: filled at limit (0 fees, better price)
//!    - If not filled by bar end: skip signal OR market execute
//! 4. For shorts: place limit above current price
//!
//! # Example
//!
//! ```no_run
//! use krypto::backtest::passive::{PassiveExecutor, PassiveConfig};
//! use krypto::data::loader::DataLoader;
//! use polars::prelude::*;
//!
//! async fn example() -> anyhow::Result<()> {
//!     let loader = DataLoader::new(None, None);
//!     
//!     // Fetch 1h and 1m data
//!     let df_1h = loader.fetch_data("BTCFDUSD", "1h", 1000).await?;
//!     let df_1m = loader.fetch_data("BTCFDUSD", "1m", 60000).await?;
//!     
//!     // Your signal series (from a strategy)
//!     let signals = Series::new("signal".into(), vec![0.0; 1000]);
//!     
//!     // Configure passive execution
//!     let config = PassiveConfig {
//!         tick_offset_bps: 5.0,      // 5 bps below market
//!         max_wait_bars: 1,          // Fill within 1 bar or skip
//!         force_market_on_timeout: false,
//!         maker_fee: 0.0,            // FDUSD pairs have 0% maker fee
//!         taker_fee: 0.001,          // 0.1% if we have to market execute
//!     };
//!     
//!     let executor = PassiveExecutor::new(config);
//!     let fills = executor.simulate(&df_1h, &df_1m, &signals).await?;
//!     
//!     println!("Fill rate: {:.1}%", fills.fill_rate * 100.0);
//!     println!("Avg price improvement: {:.2} bps", fills.avg_price_improvement_bps);
//!     
//!     Ok(())
//! }
//! ```

use anyhow::Result;
use polars::prelude::*;
use serde::{Deserialize, Serialize};

/// Configuration for passive limit order execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveConfig {
    /// How far below (longs) or above (shorts) market to place limit, in basis points.
    /// E.g., 5.0 = place limit 0.05% below current price.
    pub tick_offset_bps: f64,
    /// Maximum bars to wait for a fill before giving up.
    pub max_wait_bars: usize,
    /// If true, execute at market after timeout. If false, skip the signal.
    pub force_market_on_timeout: bool,
    /// Maker fee rate (0.0 for FDUSD pairs).
    pub maker_fee: f64,
    /// Taker fee rate (used if force_market_on_timeout is true).
    pub taker_fee: f64,
}

impl Default for PassiveConfig {
    fn default() -> Self {
        Self {
            tick_offset_bps: 5.0,        // 5 bps = 0.05%
            max_wait_bars: 1,            // Fill within current bar
            force_market_on_timeout: false,
            maker_fee: 0.0,              // FDUSD pairs
            taker_fee: 0.001,            // 0.1% standard
        }
    }
}

/// Result of simulating passive execution over a backtest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveFillStats {
    /// Total signals generated.
    pub total_signals: usize,
    /// Signals that resulted in fills (either limit or market).
    pub filled_signals: usize,
    /// Signals that were skipped (no fill within timeout).
    pub skipped_signals: usize,
    /// Fill rate (filled / total).
    pub fill_rate: f64,
    /// Average price improvement in basis points (negative = worse).
    pub avg_price_improvement_bps: f64,
    /// Total fees saved by using maker vs taker.
    pub total_fees_saved: f64,
    /// Estimated PnL improvement from better fills (excluding fees).
    pub pnl_improvement_pct: f64,
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
    /// Market price at signal time.
    pub market_price: f64,
    /// Limit price that was set.
    pub limit_price: f64,
    /// Actual fill price (limit price if filled, market if timeout forced).
    pub fill_price: f64,
    /// How many lower-timeframe bars until fill.
    pub bars_to_fill: usize,
    /// Fee paid on this fill.
    pub fee: f64,
    /// Whether this was a limit fill (true) or market fill (false).
    pub was_limit_fill: bool,
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

    /// Create with default configuration (5 bps offset, 1 bar timeout, 0% maker fee).
    pub fn with_defaults() -> Self {
        Self::new(PassiveConfig::default())
    }

    /// Simulate passive execution on a signal series.
    ///
    /// # Arguments
    ///
    /// * `df_high` - Higher timeframe OHLCV data (e.g., 1h)
    /// * `df_low` - Lower timeframe OHLCV data (e.g., 1m) covering the same period
    /// * `signals` - Signal series aligned with `df_high`
    ///
    /// # Returns
    ///
    /// A vector of fill events and aggregate statistics.
    pub async fn simulate(
        &self,
        df_high: &DataFrame,
        df_low: &DataFrame,
        signals: &Series,
    ) -> Result<(Vec<FillEvent>, PassiveFillStats)> {
        let high_times = df_high.column("time")?.datetime()?;
        let high_closes = df_high.column("close")?.f64()?;
        let high_highs = df_high.column("high")?.f64()?;
        let high_lows = df_high.column("low")?.f64()?;

        let low_times = df_low.column("time")?.datetime()?;
        let _low_opens = df_low.column("open")?.f64()?;
        let low_highs = df_low.column("high")?.f64()?;
        let low_lows = df_low.column("low")?.f64()?;
        let _low_closes = df_low.column("close")?.f64()?;

        let signals_f64 = signals.f64()?;

        let mut fills: Vec<FillEvent> = Vec::new();
        let mut total_signals = 0usize;
        let mut filled_signals = 0usize;
        let mut skipped_signals = 0usize;
        let mut price_improvements_bps: Vec<f64> = Vec::new();
        let mut total_fees_saved = 0.0;
        let mut pnl_improvement = 0.0;

        let tick_offset = self.config.tick_offset_bps / 10_000.0;

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

            // Determine limit price based on direction
            let (limit_price, direction) = if sig > 0.0 {
                // Long: place limit below market
                (market_price * (1.0 - tick_offset), 1.0)
            } else {
                // Short: place limit above market
                (market_price * (1.0 + tick_offset), -1.0)
            };

            // Find the time range for this higher-timeframe bar
            let bar_start = high_times.get(i).unwrap_or(0);
            let bar_end = if i + 1 < df_high.height() {
                high_times.get(i + 1).unwrap_or(i64::MAX)
            } else {
                i64::MAX
            };

            // Find lower-timeframe bars within this range
            let mut filled = false;
            let mut fill_price = 0.0;
            let mut fill_bar_lower = 0;
            let mut bars_to_fill = 0;

            for j in 0..df_low.height() {
                let low_time = low_times.get(j).unwrap_or(0);
                if low_time < bar_start {
                    continue;
                }
                if low_time >= bar_end {
                    break;
                }

                bars_to_fill += 1;

                let low_low = low_lows.get(j).unwrap_or(f64::MAX);
                let low_high = low_highs.get(j).unwrap_or(f64::MIN);

                // Check if limit would have been hit
                let hit = if direction > 0.0 {
                    // Long: limit below market, fills if low <= limit
                    low_low <= limit_price
                } else {
                    // Short: limit above market, fills if high >= limit
                    low_high >= limit_price
                };

                if hit {
                    filled = true;
                    fill_price = limit_price;
                    fill_bar_lower = j;
                    break;
                }
            }

            if !filled && self.config.force_market_on_timeout {
                // Force market execution at bar close
                fill_price = high_closes.get(i).unwrap_or(market_price);
                filled = true;
            }

            if filled {
                filled_signals += 1;

                let was_limit = (fill_price - limit_price).abs() < 0.0001;
                let fee = if was_limit {
                    self.config.maker_fee
                } else {
                    self.config.taker_fee
                };

                // Calculate price improvement vs market
                let improvement_bps = if direction > 0.0 {
                    // Long: better = lower fill price
                    ((market_price - fill_price) / market_price) * 10_000.0
                } else {
                    // Short: better = higher fill price
                    ((fill_price - market_price) / market_price) * 10_000.0
                };

                price_improvements_bps.push(improvement_bps);

                if was_limit {
                    total_fees_saved += self.config.taker_fee - self.config.maker_fee;
                    pnl_improvement += improvement_bps / 10_000.0;
                }

                fills.push(FillEvent {
                    signal_bar: i,
                    fill_bar_lower,
                    direction,
                    market_price,
                    limit_price,
                    fill_price,
                    bars_to_fill,
                    fee,
                    was_limit_fill: was_limit,
                });
            } else {
                skipped_signals += 1;
            }
        }

        let avg_improvement = if price_improvements_bps.is_empty() {
            0.0
        } else {
            price_improvements_bps.iter().sum::<f64>() / price_improvements_bps.len() as f64
        };

        let stats = PassiveFillStats {
            total_signals,
            filled_signals,
            skipped_signals,
            fill_rate: if total_signals > 0 {
                filled_signals as f64 / total_signals as f64
            } else {
                0.0
            },
            avg_price_improvement_bps: avg_improvement,
            total_fees_saved,
            pnl_improvement_pct: pnl_improvement * 100.0,
        };

        Ok((fills, stats))
    }

    /// Convert fill events to a signal series that can be fed to the backtest engine.
    ///
    /// This creates a signal series where signals are only present at bars where
    /// passive execution would have resulted in a fill.
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
        // 1h data: 3 bars
        let high_times: Vec<i64> = vec![0, 3600_000, 7200_000]; // 1h in ms
        let high_closes = vec![100.0, 101.0, 99.0];
        let high_highs = vec![101.0, 102.0, 100.0];
        let high_lows = vec![99.0, 100.0, 98.0];
        let high_vols = vec![1000.0; 3];

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

        // 1m data: simulate 3 minutes per hour, with one bar dipping below limit
        let mut low_times = Vec::new();
        let mut low_opens = Vec::new();
        let mut low_highs = Vec::new();
        let mut low_lows = Vec::new();
        let mut low_closes = Vec::new();
        let mut low_vols = Vec::new();

        for (h, &base_time) in high_times.iter().enumerate() {
            for m in 0..3 {
                low_times.push(base_time + m as i64 * 60_000);
                // Bar 0, minute 1: price dips below 99.9 (limit at 99.95)
                if h == 0 && m == 1 {
                    low_opens.push(100.0);
                    low_highs.push(100.0);
                    low_lows.push(99.8); // Below limit!
                    low_closes.push(99.9);
                } else if h == 0 {
                    // Bar 0 other minutes
                    low_opens.push(100.0);
                    low_highs.push(100.5);
                    low_lows.push(99.9);
                    low_closes.push(100.2);
                } else {
                    // Bar 1 and 2: no dips, highs stay above 100.9 so short would fill
                    low_opens.push(101.0);
                    low_highs.push(101.5);
                    low_lows.push(100.9); // Above limit for long (100.9495)
                    low_closes.push(101.2);
                }
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
    async fn test_passive_fill_on_dip() {
        let (df_high, df_low) = make_test_data();

        // Signal: buy on bar 0
        let signals = Series::new("signal".into(), vec![1.0, 0.0, 0.0]);

        let executor = PassiveExecutor::with_defaults();
        let (fills, stats) = executor.simulate(&df_high, &df_low, &signals).await.unwrap();

        assert_eq!(stats.total_signals, 1);
        assert_eq!(stats.filled_signals, 1);
        assert_eq!(fills.len(), 1);

        let fill = &fills[0];
        assert!(fill.was_limit_fill, "Should have filled at limit price");
        assert_eq!(fill.direction, 1.0);
        assert!(fill.bars_to_fill > 0, "Should take at least 1 lower bar");
    }

    #[tokio::test]
    async fn test_skip_on_no_dip() {
        let (df_high, df_low) = make_test_data();

        // Signal: buy on bar 1 (no dip in that hour)
        let signals = Series::new("signal".into(), vec![0.0, 1.0, 0.0]);

        let executor = PassiveExecutor::with_defaults();
        let (_, stats) = executor.simulate(&df_high, &df_low, &signals).await.unwrap();

        assert_eq!(stats.total_signals, 1);
        assert_eq!(stats.filled_signals, 0);
        assert_eq!(stats.skipped_signals, 1);
    }

    #[tokio::test]
    async fn test_force_market_on_timeout() {
        let (df_high, df_low) = make_test_data();

        // Signal: buy on bar 1 (no dip)
        let signals = Series::new("signal".into(), vec![0.0, 1.0, 0.0]);

        let config = PassiveConfig {
            force_market_on_timeout: true,
            ..Default::default()
        };
        let executor = PassiveExecutor::new(config);
        let (fills, stats) = executor.simulate(&df_high, &df_low, &signals).await.unwrap();

        assert_eq!(stats.filled_signals, 1);
        assert!(!fills[0].was_limit_fill, "Should be market fill");
    }

    #[tokio::test]
    async fn test_short_execution() {
        let (df_high, df_low) = make_test_data();

        // Signal: short on bar 0
        let signals = Series::new("signal".into(), vec![-1.0, 0.0, 0.0]);

        let executor = PassiveExecutor::with_defaults();
        let (fills, stats) = executor.simulate(&df_high, &df_low, &signals).await.unwrap();

        assert_eq!(stats.total_signals, 1);
        // Short limit is above market, low_low going down won't fill it
        // So this should be skipped unless price goes UP
        assert_eq!(stats.filled_signals, 0);
    }
}

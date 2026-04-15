//! Adapter to use SignalGenerator strategies with the paper trading bot.
//!
//! Bridges the gap between:
//! - `SignalGenerator` trait (used by backtest engine and strategy registry)
//! - `Strategy` trait (used by paper trading bot for bar-by-bar processing)

use super::bot::{Bar, Strategy, Trade};
use crate::algo::SignalGenerator;
use crate::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::VecDeque;

/// Internal position tracking for stop loss calculation.
#[derive(Debug, Clone, Copy, Default)]
enum AdapterPosition {
    #[default]
    Flat,
    Long {
        entry_price: f64,
        highest_since_entry: f64,
    },
    Short {
        entry_price: f64,
        lowest_since_entry: f64, // Track lowest for trailing stop
    },
}

/// Adapter that wraps a SignalGenerator to work with the paper bot.
///
/// Maintains a rolling window of bars, converts them to a DataFrame,
/// adds technical indicators, and runs the signal generator.
///
/// Tracks entry price internally for accurate stop loss calculation.
pub struct SignalGeneratorAdapter {
    strategy: Box<dyn SignalGenerator>,
    lookback: usize,
    bars: VecDeque<Bar>,
    stop_mult: f64, // ATR multiplier for stop loss
    use_stops: bool,
    position: AdapterPosition, // Internal position tracking for stops
    entry_atr: f64,            // ATR at time of entry (fixed stop distance)
    verbose: bool,             // Debug logging
    bar_count: usize,          // Track bar count for debugging
}

impl SignalGeneratorAdapter {
    /// Create a new adapter wrapping a SignalGenerator.
    ///
    /// # Arguments
    /// * `strategy` - The signal generator to wrap
    /// * `lookback` - Number of bars to maintain for indicator calculation
    /// * `stop_mult` - ATR multiplier for stop loss (e.g., 0.30 for 0.3× ATR)
    pub fn new(strategy: Box<dyn SignalGenerator>, lookback: usize, stop_mult: f64) -> Self {
        Self {
            strategy,
            lookback,
            bars: VecDeque::with_capacity(lookback + 1),
            stop_mult,
            use_stops: stop_mult > 0.0,
            position: AdapterPosition::Flat,
            entry_atr: 0.0,
            verbose: false,
            bar_count: 0,
        }
    }

    /// Enable verbose logging for debugging.
    pub fn with_verbose(mut self) -> Self {
        self.verbose = true;
        self
    }

    /// Disable stop losses (rely on strategy signals only).
    pub fn without_stops(mut self) -> Self {
        self.use_stops = false;
        self
    }

    /// Convert bars to DataFrame for signal generation.
    fn bars_to_dataframe(&self) -> DataFrame {
        let n = self.bars.len();

        let mut times = Vec::with_capacity(n);
        let mut opens = Vec::with_capacity(n);
        let mut highs = Vec::with_capacity(n);
        let mut lows = Vec::with_capacity(n);
        let mut closes = Vec::with_capacity(n);
        let mut volumes = Vec::with_capacity(n);

        for bar in &self.bars {
            times.push(bar.time.timestamp_millis());
            opens.push(bar.open);
            highs.push(bar.high);
            lows.push(bar.low);
            closes.push(bar.close);
            volumes.push(bar.volume);
        }

        df! [
            "time" => times,
            "open" => opens,
            "high" => highs,
            "low" => lows,
            "close" => closes,
            "volume" => volumes,
        ]
        .unwrap_or_else(|_| DataFrame::default())
    }

    /// Get the most recent ATR value.
    fn current_atr(&self, df: &DataFrame) -> f64 {
        df.column("atr")
            .ok()
            .and_then(|s| s.f64().ok())
            .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
            .unwrap_or(0.0)
    }

    /// Sync internal position state with the paper bot's actual position.
    fn sync_position_state(&mut self, bot_position: f64, current_price: f64) {
        match (self.position, bot_position) {
            // Bot is flat but we think we're in a position - reset
            (AdapterPosition::Long { .. } | AdapterPosition::Short { .. }, 0.0) => {
                self.position = AdapterPosition::Flat;
                self.entry_atr = 0.0;
            }
            // Bot is in long position - update high tracking
            (
                AdapterPosition::Long {
                    entry_price,
                    highest_since_entry,
                },
                pos,
            ) if pos > 0.0 => {
                self.position = AdapterPosition::Long {
                    entry_price,
                    highest_since_entry: highest_since_entry.max(current_price),
                };
            }
            // Bot is in short position - update low tracking (for trailing stop)
            (
                AdapterPosition::Short {
                    entry_price,
                    lowest_since_entry,
                },
                pos,
            ) if pos < 0.0 => {
                self.position = AdapterPosition::Short {
                    entry_price,
                    lowest_since_entry: lowest_since_entry.min(current_price),
                };
            }
            // Bot has a position but we're flat - initialize from bot state
            // (this happens when adapter is created mid-trade, shouldn't normally occur)
            (AdapterPosition::Flat, pos) if pos > 0.0 => {
                self.position = AdapterPosition::Long {
                    entry_price: current_price,
                    highest_since_entry: current_price,
                };
            }
            (AdapterPosition::Flat, pos) if pos < 0.0 => {
                self.position = AdapterPosition::Short {
                    entry_price: current_price,
                    lowest_since_entry: current_price,
                };
            }
            _ => {}
        }
    }

    /// Check if trailing stop loss was hit.
    ///
    /// Uses TRAILING stop that follows the price:
    /// - Long: stop = highest_price * (1 - trailing_sl) - exits when price drops
    /// - Short: stop = lowest_price * (1 + trailing_sl) - exits when price rises
    ///
    /// The trailing_sl is computed as entry_atr / entry_price to convert
    /// ATR to a percentage, matching the backtest engine behavior.
    ///
    /// Returns Some(stop_price) if hit, None otherwise.
    fn check_stop(&self, bar: &Bar) -> Option<f64> {
        // Compute trailing_sl as percentage from ATR
        let entry_price = match self.position {
            AdapterPosition::Long { entry_price, .. } => entry_price,
            AdapterPosition::Short { entry_price, .. } => entry_price,
            AdapterPosition::Flat => return None,
        };

        if entry_price <= 0.0 || self.entry_atr <= 0.0 {
            return None;
        }

        let trailing_sl = (self.entry_atr * self.stop_mult) / entry_price;

        match self.position {
            AdapterPosition::Long {
                highest_since_entry,
                ..
            } => {
                let stop_price = highest_since_entry * (1.0 - trailing_sl);
                if bar.low <= stop_price {
                    Some(stop_price)
                } else {
                    None
                }
            }
            AdapterPosition::Short {
                lowest_since_entry, ..
            } => {
                let stop_price = lowest_since_entry * (1.0 + trailing_sl);
                if bar.high >= stop_price {
                    Some(stop_price)
                } else {
                    None
                }
            }
            AdapterPosition::Flat => None,
        }
    }
}

impl Strategy for SignalGeneratorAdapter {
    fn name(&self) -> &str {
        self.strategy.name()
    }

    fn on_bar(&mut self, bar: &Bar, position: f64, _history: &[Bar]) -> Option<Trade> {
        // Add bar to history
        self.bars.push_back(bar.clone());
        if self.bars.len() > self.lookback {
            self.bars.pop_front();
        }
        self.bar_count += 1;

        // Need enough bars for indicators
        // BollingerReversion needs: 20 bars for BB + 14 bars for RSI
        // Use minimum of 20 bars warmup (BB period) before generating signals
        const WARMUP_BARS: usize = 20;
        if self.bars.len() < WARMUP_BARS {
            if self.verbose && self.bar_count <= 30 {
                eprintln!(
                    "[Bar {}] Warmup: {}/{} bars",
                    self.bar_count,
                    self.bars.len(),
                    WARMUP_BARS
                );
            }
            return None;
        }

        // Convert to DataFrame and add indicators
        let df = self.bars_to_dataframe();
        let df_with_features = match FeatureEngine::add_technicals(&df, None) {
            Ok(d) => d,
            Err(e) => {
                if self.verbose {
                    eprintln!("[Bar {}] FeatureEngine error: {:?}", self.bar_count, e);
                }
                return None;
            }
        };

        // Get current ATR
        let current_atr = self.current_atr(&df_with_features);

        // Update internal position tracking based on bot's actual position
        self.sync_position_state(position, bar.close);

        // Check stop loss first if in position
        let mut _stop_hit = false;
        if self.use_stops && position != 0.0 {
            if let Some(stop_price) = self.check_stop(bar) {
                if self.verbose {
                    eprintln!(
                        "[Bar {}] STOP HIT at stop={:.2}, bar.low={:.2}, bar.high={:.2}",
                        self.bar_count, stop_price, bar.low, bar.high
                    );
                }
                self.position = AdapterPosition::Flat;
                self.entry_atr = 0.0;
                _stop_hit = true;
                // Don't return yet - check signal to see if we should reopen
            }
        }

        // Generate signals
        let signals = match self.strategy.predict(&df_with_features) {
            Ok(s) => s,
            Err(e) => {
                if self.verbose {
                    eprintln!("[Bar {}] Signal prediction error: {:?}", self.bar_count, e);
                }
                return None;
            }
        };

        // Get latest signal (from previous bar to avoid look-ahead bias)
        // signals[i] is based on data up to bar i (generated at close of bar i)
        // Backtest: at bar i, uses signals[i-1] (signal from bar i-1)
        // Paper bot: has bars 0 to n-1 (n bars), uses signals[n-2]
        // This matches: when processing bar n-1, use signal from bar n-2
        let signal_idx = signals.len().saturating_sub(2);
        let signal = signals
            .f64()
            .ok()
            .and_then(|ca| ca.get(signal_idx))
            .unwrap_or(0.0);

        // Debug logging
        if self.verbose && self.bar_count <= 30 {
            let atr_val = self.current_atr(&df_with_features);
            eprintln!(
                "[Bar {}] close={:.2}, signal_idx={}, signal={:.1}, position={:.1}, atr={:.2}",
                self.bar_count, bar.close, signal_idx, signal, position, atr_val
            );
        }

        // Convert signal to trade action
        let trade = match signal {
            1.0 => {
                // Long signal
                if position == 0.0 {
                    // Opening new long - record entry
                    self.position = AdapterPosition::Long {
                        entry_price: bar.close,
                        highest_since_entry: bar.high,
                    };
                    self.entry_atr = current_atr;
                    Some(Trade::Long { size: 1.0 })
                } else if position < 0.0 {
                    // Reversing from short to long
                    self.position = AdapterPosition::Long {
                        entry_price: bar.close,
                        highest_since_entry: bar.high,
                    };
                    self.entry_atr = current_atr;
                    Some(Trade::Reverse)
                } else {
                    None
                }
            }
            -1.0 => {
                // Short signal
                if position == 0.0 {
                    // Opening new short - record entry
                    self.position = AdapterPosition::Short {
                        entry_price: bar.close,
                        lowest_since_entry: bar.low, // Track lowest for trailing stop
                    };
                    self.entry_atr = current_atr;
                    Some(Trade::Short { size: 1.0 })
                } else if position > 0.0 {
                    // Reversing from long to short
                    self.position = AdapterPosition::Short {
                        entry_price: bar.close,
                        lowest_since_entry: bar.low,
                    };
                    self.entry_atr = current_atr;
                    Some(Trade::Reverse)
                } else {
                    None
                }
            }
            _ => None,
        };

        trade
    }

    fn reset(&mut self) {
        self.bars.clear();
        self.position = AdapterPosition::Flat;
        self.entry_atr = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algo::StrategyRegistry;
    use chrono::{TimeZone, Utc};

    fn make_bar(close: f64) -> Bar {
        Bar::new(
            Utc::now(),
            close - 1.0,
            close + 1.0,
            close - 1.0,
            close,
            1000.0,
        )
    }

    #[test]
    fn test_adapter_basic() {
        let registry = StrategyRegistry::new();
        let strategy = registry.create("bollinger_reversion").unwrap();
        let mut adapter = SignalGeneratorAdapter::new(strategy, 100, 0.30);

        // Feed enough bars to generate signals
        for i in 0..60 {
            let bar = make_bar(100.0 + (i as f64 * 0.1));
            adapter.on_bar(&bar, 0.0, &[]);
        }

        // Adapter should be ready to generate signals
        assert_eq!(adapter.name(), "Bollinger_Reversion");
    }

    #[test]
    fn test_adapter_maintains_history() {
        let registry = StrategyRegistry::new();
        let strategy = registry.create("bollinger_reversion").unwrap();
        let mut adapter = SignalGeneratorAdapter::new(strategy, 50, 0.30);

        // Add bars
        for i in 0..100 {
            let bar = make_bar(100.0 + i as f64);
            adapter.on_bar(&bar, 0.0, &[]);
        }

        // Should maintain lookback limit
        assert!(adapter.bars.len() <= 51); // lookback + 1
    }
}

use anyhow::Result;
use polars::prelude::*;

/// Position sizing strategy for backtesting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionSizing {
    /// Use 100% of available equity on every trade (default, backwards compatible)
    Full,
    /// Use a fixed fraction of equity per trade (e.g., 0.5 = 50%)
    FixedFraction(f64),
    /// Size position so that stop loss = X% of equity
    /// The parameter is the risk percentage (e.g., 0.02 = 2% risk per trade)
    RiskPerTrade(f64),
}

impl Default for PositionSizing {
    fn default() -> Self {
        PositionSizing::Full
    }
}

#[derive(Debug, Clone)]
pub struct BacktestResult {
    pub total_trades: usize,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub final_equity: f64,
    pub total_return_pct: f64,
    pub max_drawdown_pct: f64,
    pub sharpe_ratio: f64,
    pub kelly_fraction: f64,
    pub equity_curve: Vec<f64>,
    pub total_fees_paid: f64,
    pub average_position_size: f64,
}

/// Trading engine that simulates strategy execution over historical data.
///
/// **⚠️ KNOWN LIMITATIONS (To be fixed in V3 architecture):**
/// - **Long-only:** Only supports position sizes of `0.0` or `1.0`. It does not handle
///   short selling, even though some strategies generate `-1.0` signals.
/// - **Optimistic Trailing Stop:** The stop-loss is evaluated against the `close` price 
///   at the end of the candle. In reality, a stop would trigger intra-bar at the `low` price, 
///   meaning this backtester produces falsely optimistic results for volatile assets.
/// - **No Slippage by Default:** While the field exists, it is not consistently applied across 
///   all order types.
pub struct Backtester {
    initial_capital: f64,
    fee_pct: f64,
    /// Slippage in basis points (bps). Default: 5 bps = 0.05%.
    /// Applied to the fill price: buys pay more, sells receive less.
    slippage_bps: f64,
    /// Position sizing strategy. Default: Full (100% of equity)
    position_sizing: PositionSizing,
}

impl Backtester {
    pub fn new(initial_capital: f64, fee_pct: f64, slippage_bps: f64) -> Self {
        Self {
            initial_capital,
            fee_pct,
            slippage_bps,
            position_sizing: PositionSizing::Full,
        }
    }

    /// Create a backtester with sensible defaults (5 bps slippage).
    pub fn with_defaults(initial_capital: f64) -> Self {
        Self::new(initial_capital, 0.001, 5.0)
    }

    /// Set the position sizing strategy
    pub fn with_position_sizing(mut self, sizing: PositionSizing) -> Self {
        self.position_sizing = sizing;
        self
    }

    /// Convert slippage_bps to a multiplier factor (e.g. 5 bps → 0.0005).
    #[inline]
    fn slippage_factor(&self) -> f64 {
        self.slippage_bps / 10_000.0
    }

    /// Calculate position size based on the sizing strategy
    fn calculate_position_size(&self, equity: f64, entry_price: f64, trailing_sl: f64) -> f64 {
        match self.position_sizing {
            PositionSizing::Full => 1.0,
            PositionSizing::FixedFraction(fraction) => fraction.clamp(0.0, 1.0),
            PositionSizing::RiskPerTrade(risk_pct) => {
                // Risk per trade = position_size * entry_price * trailing_sl
                // We want: risk_pct * equity = position_size * entry_price * trailing_sl
                // So: position_size = (risk_pct * equity) / (entry_price * trailing_sl)
                if trailing_sl > 0.0 && entry_price > 0.0 {
                    let position_size = (risk_pct * equity) / (entry_price * trailing_sl);
                    // Cap at 1.0 to avoid over-leveraging
                    position_size.min(1.0)
                } else {
                    1.0
                }
            }
        }
    }

    pub fn run(&self, df: &DataFrame, signal: &Series, trailing_sl: f64) -> Result<BacktestResult> {
        let closes = df.column("close")?.f64()?;
        let highs = df.column("high")?.f64()?;
        let lows = df.column("low")?.f64()?;
        let signals = signal.f64()?;

        let slip = self.slippage_factor();

        let mut equity = self.initial_capital;
        let mut position = 0.0;
        let mut position_size = 0.0; // Track the position size (0.0 to 1.0)
        let mut entry_price = 0.0;
        let mut highest_price_in_trade = 0.0;
        let mut lowest_price_in_trade = 0.0;

        let mut wins = 0;
        let mut losses = 0;
        let mut gross_profit = 0.0;
        let mut gross_loss = 0.0;
        let mut total_fees_paid = 0.0;
        let mut _returns_list: Vec<f64> = Vec::new();

        let mut peak_equity = equity;
        let mut max_drawdown = 0.0;

        let mut equity_curve = Vec::with_capacity(closes.len());
        let mut position_sizes: Vec<f64> = Vec::new();

        for i in 0..closes.len() {
            let price = closes.get(i).unwrap_or(0.0);
            let high = highs.get(i).unwrap_or(price);
            let low = lows.get(i).unwrap_or(price);
            let sig = signals.get(i).unwrap_or(0.0);

            // ── Trailing stop logic ────────────────────────────────────────
            // We check the bar's LOW for longs and HIGH for shorts — stops
            // trigger intra-bar, not just at the close price.
            if position > 0.0 {
                if high > highest_price_in_trade {
                    highest_price_in_trade = high;
                }
                let stop_price = highest_price_in_trade * (1.0 - trailing_sl);
                if low < stop_price {
                    // Stop triggered: fill at the stop price (not close)
                    let notional = equity * position_size;
                    let fee = notional * self.fee_pct * 2.0; // entry + exit legs
                    let pnl_pct = (stop_price - entry_price) / entry_price;
                    let pnl_amount = notional * pnl_pct - fee;

                    equity += pnl_amount;
                    total_fees_paid += fee;

                    if pnl_amount > 0.0 {
                        wins += 1;
                        gross_profit += pnl_amount;
                    } else {
                        losses += 1;
                        gross_loss += pnl_amount.abs();
                    }
                    position = 0.0;
                    position_size = 0.0;
                }
            } else if position < 0.0 {
                if low < lowest_price_in_trade {
                    lowest_price_in_trade = low;
                }
                let stop_price = lowest_price_in_trade * (1.0 + trailing_sl);
                if high > stop_price {
                    // Stop triggered: fill at the stop price (not close)
                    let notional = equity * position_size;
                    let fee = notional * self.fee_pct * 2.0;
                    let pnl_pct = (entry_price - stop_price) / entry_price;
                    let pnl_amount = notional * pnl_pct - fee;

                    equity += pnl_amount;
                    total_fees_paid += fee;

                    if pnl_amount > 0.0 {
                        wins += 1;
                        gross_profit += pnl_amount;
                    } else {
                        losses += 1;
                        gross_loss += pnl_amount.abs();
                    }
                    position = 0.0;
                    position_size = 0.0;
                }
            }

            // ── Signal-driven entry / exit ─────────────────────────────────
            if (sig - position).abs() > 0.01 {
                // Apply slippage: buys fill higher, sells fill lower
                let exec_price = if sig > position {
                    price * (1.0 + slip) // buy: worse fill
                } else {
                    price * (1.0 - slip) // sell: worse fill
                };

                if position.abs() > 0.01 {
                    let raw_pnl_pct = if position > 0.0 {
                        (exec_price - entry_price) / entry_price
                    } else {
                        (entry_price - exec_price) / entry_price
                    };

                    // Fee applied to notional value of the trade
                    let notional = equity * position_size;
                    let fee = notional * self.fee_pct * 2.0;
                    let net_pnl_pct = raw_pnl_pct - (self.fee_pct * 2.0);
                    let pnl_amount = notional * net_pnl_pct;

                    equity += pnl_amount;
                    total_fees_paid += fee;
                    _returns_list.push(net_pnl_pct);

                    if pnl_amount > 0.0 {
                        wins += 1;
                        gross_profit += pnl_amount;
                    } else {
                        losses += 1;
                        gross_loss += pnl_amount.abs();
                    }
                }

                if sig.abs() > 0.01 {
                    // Calculate position size when entering a position
                    position_size = self.calculate_position_size(equity, exec_price, trailing_sl);
                    position_sizes.push(position_size);
                    entry_price = exec_price;
                    highest_price_in_trade = price;
                    lowest_price_in_trade = price;
                } else {
                    position_size = 0.0;
                }
                position = sig;
            }

            // ── Drawdown tracking ──────────────────────────────────────────
            if equity > peak_equity {
                peak_equity = equity;
            }
            let dd = (peak_equity - equity) / peak_equity;
            if dd > max_drawdown {
                max_drawdown = dd;
            }

            // ── Mark-to-market equity curve ────────────────────────────────
            let mtm_equity = if position.abs() > 0.01 {
                let current_pnl_pct = if position > 0.0 {
                    (price - entry_price) / entry_price
                } else {
                    (entry_price - price) / entry_price
                };
                equity + (equity * position_size * current_pnl_pct)
            } else {
                equity
            };
            equity_curve.push(mtm_equity);
        }

        let total_trades = wins + losses;
        let win_rate = if total_trades > 0 {
            wins as f64 / total_trades as f64
        } else {
            0.0
        };

        let profit_factor = if gross_loss.abs() > f64::EPSILON {
            gross_profit / gross_loss
        } else if gross_profit > 0.0 {
            100.0
        } else {
            0.0
        };

        let total_return = (equity - self.initial_capital) / self.initial_capital;

        let avg_win = if wins > 0 {
            gross_profit / wins as f64
        } else {
            0.0
        };

        let avg_loss = if losses > 0 {
            gross_loss / losses as f64
        } else if total_trades > 0 {
            1.0
        } else {
            0.0
        };

        let payoff_ratio = if avg_loss.abs() > f64::EPSILON {
            avg_win / avg_loss
        } else {
            0.0
        };

        let kelly = if payoff_ratio > 0.0 {
            win_rate - ((1.0 - win_rate) / payoff_ratio)
        } else {
            0.0
        };

        let kelly_fraction = (kelly * 0.5).clamp(0.0, 0.2);

        let sharpe = if max_drawdown > 0.0 {
            total_return / max_drawdown
        } else {
            0.0
        };

        let average_position_size = if !position_sizes.is_empty() {
            position_sizes.iter().sum::<f64>() / position_sizes.len() as f64
        } else {
            0.0
        };

        Ok(BacktestResult {
            total_trades,
            win_rate: win_rate * 100.0,
            profit_factor,
            final_equity: equity,
            total_return_pct: total_return * 100.0,
            max_drawdown_pct: max_drawdown * 100.0,
            sharpe_ratio: sharpe,
            kelly_fraction,
            equity_curve,
            total_fees_paid,
            average_position_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_dataframe() -> DataFrame {
        let closes = vec![100.0, 101.0, 102.0, 101.0, 100.0, 99.0, 100.0, 101.0, 102.0, 103.0];
        let highs = vec![101.0, 102.0, 103.0, 102.0, 101.0, 100.0, 101.0, 102.0, 103.0, 104.0];
        let lows = vec![99.0, 100.0, 101.0, 100.0, 99.0, 98.0, 99.0, 100.0, 101.0, 102.0];
        let volumes = vec![1000.0; 10];

        df!(
            "close" => closes,
            "high" => highs,
            "low" => lows,
            "volume" => volumes
        ).unwrap()
    }

    fn create_buy_signal() -> Series {
        // Buy at bar 1, sell at bar 5, buy at bar 6
        Series::new("signal".into(), vec![0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0])
    }

    #[test]
    fn test_full_position_sizing() {
        let df = create_test_dataframe();
        let signal = create_buy_signal();
        let backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::Full);
        
        let result = backtester.run(&df, &signal, 0.05).unwrap();
        
        // Full position sizing should use 100% of equity
        assert_eq!(result.average_position_size, 1.0);
        println!("Full sizing - Avg position size: {}, Final equity: ${:.2}", 
                 result.average_position_size, result.final_equity);
    }

    #[test]
    fn test_fixed_fraction_position_sizing() {
        let df = create_test_dataframe();
        let signal = create_buy_signal();
        let backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::FixedFraction(0.5));
        
        let result = backtester.run(&df, &signal, 0.05).unwrap();
        
        // Fixed fraction 0.5 should use 50% of equity
        assert_eq!(result.average_position_size, 0.5);
        println!("Fixed fraction 50% - Avg position size: {}, Final equity: ${:.2}", 
                 result.average_position_size, result.final_equity);
        
        // With 50% position sizing, we should have less volatility but similar returns
        let full_backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::Full);
        let full_result = full_backtester.run(&df, &signal, 0.05).unwrap();
        
        // Lower position size should result in smaller absolute returns but also smaller drawdowns
        assert!(result.final_equity < full_result.final_equity);
        assert!(result.max_drawdown_pct <= full_result.max_drawdown_pct);
    }

    #[test]
    fn test_risk_per_trade_position_sizing() {
        let df = create_test_dataframe();
        let signal = create_buy_signal();
        
        // 2% risk per trade with 5% trailing stop
        let backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::RiskPerTrade(0.02));
        
        let result = backtester.run(&df, &signal, 0.05).unwrap();
        
        // With 2% risk and 5% stop loss, position size should be 0.4 (40%)
        // position_size = (0.02 * equity) / (price * 0.05) = 0.4
        let expected_position_size = 0.4;
        assert!((result.average_position_size - expected_position_size).abs() < 0.01);
        println!("Risk per trade 2% - Avg position size: {}, Final equity: ${:.2}", 
                 result.average_position_size, result.final_equity);
    }

    #[test]
    fn test_risk_per_trade_different_stops() {
        let df = create_test_dataframe();
        let signal = create_buy_signal();
        
        // Test with tighter stop (2%)
        let backtester_tight = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::RiskPerTrade(0.02));
        let result_tight = backtester_tight.run(&df, &signal, 0.02).unwrap();
        
        // With tighter stop, position size should be larger to maintain same risk
        // position_size = (0.02 * equity) / (price * 0.02) = 1.0
        assert!((result_tight.average_position_size - 1.0).abs() < 0.01);
        
        // Test with wider stop (10%)
        let backtester_wide = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::RiskPerTrade(0.02));
        let result_wide = backtester_wide.run(&df, &signal, 0.10).unwrap();
        
        // With wider stop, position size should be smaller
        // position_size = (0.02 * equity) / (price * 0.10) = 0.2
        assert!((result_wide.average_position_size - 0.2).abs() < 0.01);
        
        println!("Tight stop (2%) - Position size: {}", result_tight.average_position_size);
        println!("Wide stop (10%) - Position size: {}", result_wide.average_position_size);
    }

    #[test]
    fn test_backwards_compatibility() {
        let df = create_test_dataframe();
        let signal = create_buy_signal();
        
        // Old API (without position sizing) should default to Full
        let old_backtester = Backtester::new(10_000.0, 0.001, 5.0);
        let old_result = old_backtester.run(&df, &signal, 0.05).unwrap();
        
        // New API with explicit Full should match
        let new_backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::Full);
        let new_result = new_backtester.run(&df, &signal, 0.05).unwrap();
        
        assert_eq!(old_result.average_position_size, new_result.average_position_size);
        assert_eq!(old_result.final_equity, new_result.final_equity);
        assert_eq!(old_result.total_trades, new_result.total_trades);
        
        println!("Backwards compatibility verified - both produce same results");
    }

    #[test]
    fn test_position_sizing_with_no_trades() {
        let df = create_test_dataframe();
        let signal = Series::new("signal".into(), vec![0.0; 10]); // No trades
        
        let backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::FixedFraction(0.5));
        
        let result = backtester.run(&df, &signal, 0.05).unwrap();
        
        assert_eq!(result.total_trades, 0);
        assert_eq!(result.average_position_size, 0.0);
        assert_eq!(result.final_equity, 10_000.0); // No change
    }

    #[test]
    fn test_fraction_bounds() {
        // Test that fractions are clamped to [0, 1]
        let df = create_test_dataframe();
        let signal = create_buy_signal();
        
        // Fraction > 1.0 should be clamped to 1.0
        let backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::FixedFraction(1.5));
        let result = backtester.run(&df, &signal, 0.05).unwrap();
        assert_eq!(result.average_position_size, 1.0);
        
        // Fraction < 0.0 should be clamped to 0.0
        let backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::FixedFraction(-0.5));
        let result = backtester.run(&df, &signal, 0.05).unwrap();
        assert_eq!(result.average_position_size, 0.0);
    }
}

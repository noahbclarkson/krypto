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
    // Extended metrics (V3)
    pub sortino_ratio: f64,
    pub calmar_ratio: f64,
    pub avg_trade_duration_bars: f64,
    pub max_consecutive_wins: usize,
    pub max_consecutive_losses: usize,
    pub avg_win_pct: f64,
    pub avg_loss_pct: f64,
    pub largest_win_pct: f64,
    pub largest_loss_pct: f64,
}

/// Trading engine that simulates strategy execution over historical data.
///
/// **✅ SUPPORTS:**
/// - **Long & Short Positions:** Fully supports signals from `-1.0` (short) to `1.0` (long).
///   Short positions profit when price drops and are correctly tracked in PnL calculations.
/// - **Position Sizing:** Multiple strategies including full equity, fixed fraction, and risk-based sizing.
/// - **Trailing Stops:** Dynamic stop-loss that follows price movements for both longs and shorts.
/// - **Fees & Slippage:** Realistic cost modeling with configurable fee percentages and slippage.
///
/// **⚠️ KNOWN LIMITATIONS:**
/// - **Optimistic Trailing Stop:** The stop-loss is evaluated against the `close` price 
///   at the end of the candle. In reality, a stop would trigger intra-bar at the `low` price, 
///   meaning this backtester produces falsely optimistic results for volatile assets.
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

    /// Update extended metrics when a trade is closed
    #[inline]
    fn update_trade_metrics(
        pnl_pct: f64,
        current_bar: usize,
        entry_bar: usize,
        trade_returns: &mut Vec<f64>,
        trade_durations: &mut Vec<usize>,
        consecutive_wins: &mut usize,
        consecutive_losses: &mut usize,
        max_consecutive_wins: &mut usize,
        max_consecutive_losses: &mut usize,
        largest_win_pct: &mut f64,
        largest_loss_pct: &mut f64,
    ) {
        // Track return percentage
        trade_returns.push(pnl_pct);
        
        // Track trade duration
        let duration = current_bar.saturating_sub(entry_bar);
        trade_durations.push(duration);
        
        // Track consecutive wins/losses and largest win/loss
        if pnl_pct > 0.0 {
            *consecutive_wins += 1;
            *consecutive_losses = 0;
            *max_consecutive_wins = (*max_consecutive_wins).max(*consecutive_wins);
            
            // Track largest win
            let win_pct = pnl_pct * 100.0;
            if win_pct > *largest_win_pct {
                *largest_win_pct = win_pct;
            }
        } else {
            *consecutive_losses += 1;
            *consecutive_wins = 0;
            *max_consecutive_losses = (*max_consecutive_losses).max(*consecutive_losses);
            
            // Track largest loss (most negative)
            let loss_pct = pnl_pct.abs() * 100.0;
            if loss_pct > *largest_loss_pct {
                *largest_loss_pct = loss_pct;
            }
        }
    }

    pub fn run(&self, df: &DataFrame, signal: &Series, trailing_sl: f64, take_profit: f64) -> Result<BacktestResult> {
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

        // Extended metrics tracking (V3)
        let mut trade_returns: Vec<f64> = Vec::new(); // For Sortino
        let mut trade_durations: Vec<usize> = Vec::new(); // Bars held
        let mut entry_bar: usize = 0; // Track entry bar for trade duration
        let mut consecutive_wins = 0; // Track consecutive wins
        let mut consecutive_losses = 0; // Track consecutive losses
        let mut max_consecutive_wins = 0;
        let mut max_consecutive_losses = 0;
        let mut largest_win_pct = 0.0;
        let mut largest_loss_pct = 0.0;

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
                    
                    // Update extended metrics
                    Self::update_trade_metrics(
                        pnl_pct,
                        i,
                        entry_bar,
                        &mut trade_returns,
                        &mut trade_durations,
                        &mut consecutive_wins,
                        &mut consecutive_losses,
                        &mut max_consecutive_wins,
                        &mut max_consecutive_losses,
                        &mut largest_win_pct,
                        &mut largest_loss_pct,
                    );
                    
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
                    
                    // Update extended metrics
                    Self::update_trade_metrics(
                        pnl_pct,
                        i,
                        entry_bar,
                        &mut trade_returns,
                        &mut trade_durations,
                        &mut consecutive_wins,
                        &mut consecutive_losses,
                        &mut max_consecutive_wins,
                        &mut max_consecutive_losses,
                        &mut largest_win_pct,
                        &mut largest_loss_pct,
                    );
                    
                    position = 0.0;
                    position_size = 0.0;
                }
            }

            // ── Take profit logic ──────────────────────────────────────────
            // Check if take profit was hit (only if TP > 0)
            if take_profit > 0.0 {
                if position > 0.0 {
                    let tp_price = entry_price * (1.0 + take_profit);
                    if high >= tp_price {
                        // TP hit: fill at TP price
                        let notional = equity * position_size;
                        let fee = notional * self.fee_pct * 2.0;
                        let pnl_pct = (tp_price - entry_price) / entry_price;
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
                        
                        // Update extended metrics
                        Self::update_trade_metrics(
                            pnl_pct,
                            i,
                            entry_bar,
                            &mut trade_returns,
                            &mut trade_durations,
                            &mut consecutive_wins,
                            &mut consecutive_losses,
                            &mut max_consecutive_wins,
                            &mut max_consecutive_losses,
                            &mut largest_win_pct,
                            &mut largest_loss_pct,
                        );
                        
                        position = 0.0;
                        position_size = 0.0;
                    }
                } else if position < 0.0 {
                    let tp_price = entry_price * (1.0 - take_profit);
                    if low <= tp_price {
                        // TP hit: fill at TP price
                        let notional = equity * position_size;
                        let fee = notional * self.fee_pct * 2.0;
                        let pnl_pct = (entry_price - tp_price) / entry_price;
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
                        
                        // Update extended metrics
                        Self::update_trade_metrics(
                            pnl_pct,
                            i,
                            entry_bar,
                            &mut trade_returns,
                            &mut trade_durations,
                            &mut consecutive_wins,
                            &mut consecutive_losses,
                            &mut max_consecutive_wins,
                            &mut max_consecutive_losses,
                            &mut largest_win_pct,
                            &mut largest_loss_pct,
                        );
                        
                        position = 0.0;
                        position_size = 0.0;
                    }
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
                    
                    // Update extended metrics
                    Self::update_trade_metrics(
                        net_pnl_pct,
                        i,
                        entry_bar,
                        &mut trade_returns,
                        &mut trade_durations,
                        &mut consecutive_wins,
                        &mut consecutive_losses,
                        &mut max_consecutive_wins,
                        &mut max_consecutive_losses,
                        &mut largest_win_pct,
                        &mut largest_loss_pct,
                    );
                }

                if sig.abs() > 0.01 {
                    // Calculate position size when entering a position
                    position_size = self.calculate_position_size(equity, exec_price, trailing_sl);
                    position_sizes.push(position_size);
                    entry_price = exec_price;
                    highest_price_in_trade = price;
                    lowest_price_in_trade = price;
                    entry_bar = i; // Track entry bar for duration calculation
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

        // Extended metrics (V3)
        // Sortino: use downside deviation instead of total volatility
        let downside_returns: Vec<f64> = trade_returns.iter().filter(|&&r| r < 0.0).copied().collect();
        let downside_std = if !downside_returns.is_empty() {
            let mean = downside_returns.iter().sum::<f64>() / downside_returns.len() as f64;
            (downside_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / downside_returns.len() as f64).sqrt()
        } else {
            0.0
        };
        let sortino_ratio = if downside_std > 0.0 {
            total_return / downside_std
        } else {
            0.0
        };

        // Calmar: annualized return / max drawdown (simplified: total return / max DD)
        let calmar_ratio = if max_drawdown > 0.0 {
            total_return / max_drawdown
        } else {
            0.0
        };

        // Average trade duration
        let avg_trade_duration_bars = if !trade_durations.is_empty() {
            trade_durations.iter().sum::<usize>() as f64 / trade_durations.len() as f64
        } else {
            0.0
        };

        // Avg win/loss percentages
        let avg_win_pct = if wins > 0 && gross_profit > 0.0 {
            (gross_profit / wins as f64) / self.initial_capital * 100.0
        } else {
            0.0
        };
        let avg_loss_pct = if losses > 0 && gross_loss > 0.0 {
            (gross_loss / losses as f64) / self.initial_capital * 100.0
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
            // Extended metrics (V3)
            sortino_ratio,
            calmar_ratio,
            avg_trade_duration_bars,
            max_consecutive_wins,
            max_consecutive_losses,
            avg_win_pct,
            avg_loss_pct,
            largest_win_pct,
            largest_loss_pct,
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
        
        let result = backtester.run(&df, &signal, 0.05, 0.0).unwrap();
        
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
        
        let result = backtester.run(&df, &signal, 0.05, 0.0).unwrap();
        
        // Fixed fraction 0.5 should use 50% of equity
        assert_eq!(result.average_position_size, 0.5);
        println!("Fixed fraction 50% - Avg position size: {}, Final equity: ${:.2}", 
                 result.average_position_size, result.final_equity);
        
        // With 50% position sizing, we should have less volatility but similar returns
        let full_backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::Full);
        let full_result = full_backtester.run(&df, &signal, 0.05, 0.0).unwrap();
        
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
        
        let result = backtester.run(&df, &signal, 0.05, 0.0).unwrap();
        
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
        let result_tight = backtester_tight.run(&df, &signal, 0.02, 0.0).unwrap();
        
        // With tighter stop, position size should be larger to maintain same risk
        // position_size = (0.02 * equity) / (price * 0.02) = 1.0
        assert!((result_tight.average_position_size - 1.0).abs() < 0.01);
        
        // Test with wider stop (10%)
        let backtester_wide = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::RiskPerTrade(0.02));
        let result_wide = backtester_wide.run(&df, &signal, 0.10, 0.0).unwrap();
        
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
        let old_result = old_backtester.run(&df, &signal, 0.05, 0.0).unwrap();
        
        // New API with explicit Full should match
        let new_backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::Full);
        let new_result = new_backtester.run(&df, &signal, 0.05, 0.0).unwrap();
        
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
        
        let result = backtester.run(&df, &signal, 0.05, 0.0).unwrap();
        
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
        let result = backtester.run(&df, &signal, 0.05, 0.0).unwrap();
        assert_eq!(result.average_position_size, 1.0);
        
        // Fraction < 0.0 should be clamped to 0.0
        let backtester = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::FixedFraction(-0.5));
        let result = backtester.run(&df, &signal, 0.05, 0.0).unwrap();
        assert_eq!(result.average_position_size, 0.0);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // SHORT SELLING TESTS
    // Verify that -1.0 signals enter real short positions with correct PnL.
    // ═══════════════════════════════════════════════════════════════════════════

    fn create_bearish_dataframe() -> DataFrame {
        // Price declines steadily from 100 to 90
        let closes  = vec![100.0, 99.0, 98.0, 97.0, 96.0, 95.0, 94.0, 93.0, 92.0, 90.0];
        let highs   = vec![101.0, 100.0, 99.0, 98.0, 97.0, 96.0, 95.0, 94.0, 93.0, 91.0];
        let lows    = vec![ 99.0,  98.0, 97.0, 96.0, 95.0, 94.0, 93.0, 92.0, 91.0, 89.0];
        let volumes = vec![1000.0f64; 10];
        df!(
            "close"  => closes,
            "high"   => highs,
            "low"    => lows,
            "volume" => volumes
        ).unwrap()
    }

    fn create_short_signal() -> Series {
        // Flat on bar 0, short bars 1-8, exit on bar 9
        Series::new("signal".into(),
            vec![0.0f64, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0, 0.0])
    }

    /// A -1.0 signal on a falling market must produce positive PnL.
    #[test]
    fn test_short_selling_profitable_when_price_drops() {
        let df = create_bearish_dataframe();
        let signal = create_short_signal();

        // No fees, no slippage — makes manual verification straightforward.
        let bt = Backtester::new(10_000.0, 0.0, 0.0);
        let result = bt.run(&df, &signal, 0.05, 0.0).unwrap();

        println!("Short sell — final: ${:.2}, return: {:.2}%",
                 result.final_equity, result.total_return_pct);

        assert!(
            result.final_equity > 10_000.0,
            "Short position should be profitable when price drops (got ${:.2})",
            result.final_equity
        );
        assert!(result.total_trades >= 1, "Should record at least one closed trade");
    }

    /// Shorts must outperform longs in a bearish trend.
    #[test]
    fn test_short_vs_long_on_bearish_trend() {
        let df = create_bearish_dataframe();
        let short_sig = create_short_signal();
        let long_sig  = Series::new("signal".into(),
            vec![0.0f64, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0]);

        let bt = Backtester::new(10_000.0, 0.0, 0.0);
        let short_r = bt.run(&df, &short_sig, 0.05, 0.0).unwrap();
        let long_r  = bt.run(&df, &long_sig,  0.05, 0.0).unwrap();

        println!("Short: ${:.2} ({:.2}%)", short_r.final_equity, short_r.total_return_pct);
        println!("Long:  ${:.2} ({:.2}%)", long_r.final_equity,  long_r.total_return_pct);

        assert!(short_r.total_return_pct > 0.0, "Short should profit in bear market");
        assert!(long_r.total_return_pct  < 0.0, "Long should lose in bear market");
        assert!(short_r.final_equity > long_r.final_equity,
                "Short equity should exceed long equity in a bearish trend");
    }

    /// Signal-driven short entry then signal exit must record exactly one trade.
    #[test]
    fn test_short_entry_and_signal_exit() {
        let df = create_bearish_dataframe();
        // Short bars 1-2, back to flat from bar 3
        let signal = Series::new("signal".into(),
            vec![0.0f64, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);

        let bt = Backtester::new(10_000.0, 0.0, 0.0);
        // Use wide trailing stop (50%) so it never triggers during this short test
        let result = bt.run(&df, &signal, 0.50, 0.0).unwrap();

        println!("Short entry/exit — trades: {}, final: ${:.2}",
                 result.total_trades, result.final_equity);

        assert_eq!(result.total_trades, 1,
                   "Should record exactly one completed short trade");
        // Entry at bar 1 close = 99, exit at bar 3 close = 97 → ~2% profit
        assert!(result.final_equity > 10_000.0, "Brief short should profit");
    }

    /// Fees must be deducted from short trades.
    #[test]
    fn test_short_selling_fees_reduce_profit() {
        let df = create_bearish_dataframe();
        let signal = create_short_signal();

        let bt_free = Backtester::new(10_000.0, 0.0,   0.0);
        let bt_fee  = Backtester::new(10_000.0, 0.001, 0.0);

        let r_free = bt_free.run(&df, &signal, 0.05, 0.0).unwrap();
        let r_fee  = bt_fee .run(&df, &signal, 0.05, 0.0).unwrap();

        println!("No fees: ${:.2}", r_free.final_equity);
        println!("Fees:    ${:.2} (paid ${:.2})", r_fee.final_equity, r_fee.total_fees_paid);

        assert!(r_fee.total_fees_paid > 0.0, "Fees should be non-zero for short trades");
        assert!(r_fee.final_equity < r_free.final_equity,
                "Fees must reduce profitability of short trades");
    }

    /// Slippage must reduce short-trade profitability (worse fill on entry & exit).
    #[test]
    fn test_short_selling_slippage_reduces_profit() {
        let df = create_bearish_dataframe();
        let signal = create_short_signal();

        let bt_clean = Backtester::new(10_000.0, 0.0,  0.0);
        let bt_slip  = Backtester::new(10_000.0, 0.0, 10.0); // 10 bps

        let r_clean = bt_clean.run(&df, &signal, 0.05, 0.0).unwrap();
        let r_slip  = bt_slip .run(&df, &signal, 0.05, 0.0).unwrap();

        println!("No slip: ${:.2}", r_clean.final_equity);
        println!("Slip:    ${:.2}", r_slip.final_equity);

        assert!(r_slip.final_equity < r_clean.final_equity,
                "Slippage should reduce profitability of short trades");
    }

    /// Position sizing must apply correctly to short trades.
    #[test]
    fn test_short_position_sizing_works() {
        let df = create_bearish_dataframe();
        let signal = create_short_signal();

        let bt_full = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::Full);
        let bt_half = Backtester::with_defaults(10_000.0)
            .with_position_sizing(PositionSizing::FixedFraction(0.5));

        let r_full = bt_full.run(&df, &signal, 0.05, 0.0).unwrap();
        let r_half = bt_half.run(&df, &signal, 0.05, 0.0).unwrap();

        println!("Full short: ${:.2}", r_full.final_equity);
        println!("Half short: ${:.2}", r_half.final_equity);

        assert!(r_full.final_equity > 10_000.0, "Full short should profit");
        assert!(r_half.final_equity > 10_000.0, "Half short should profit");
        assert!(r_full.final_equity > r_half.final_equity,
                "Full position should yield larger absolute profit than half position");
        assert_eq!(r_full.average_position_size, 1.0);
        assert_eq!(r_half.average_position_size, 0.5);
    }

    /// The mark-to-market equity curve must rise while holding a profitable short.
    #[test]
    fn test_short_equity_curve_rises_as_price_falls() {
        let df = create_bearish_dataframe();
        let signal = create_short_signal();

        let bt = Backtester::new(10_000.0, 0.0, 0.0);
        let result = bt.run(&df, &signal, 0.05, 0.0).unwrap();
        let curve = &result.equity_curve;

        // Bar 1 we just entered at ~99; bar 5 price has fallen to ~95.
        let eq_entry    = curve[1];
        let eq_mid      = curve[5];

        println!("MTM — entry bar: ${:.2}, mid-trade: ${:.2}", eq_entry, eq_mid);

        assert!(eq_mid > eq_entry,
                "MTM equity should increase as the short position gains value (price falls)");
    }
}

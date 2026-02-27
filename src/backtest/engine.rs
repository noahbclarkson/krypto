use anyhow::Result;
use polars::prelude::*;

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
/// - **100% Capital Allocation:** The engine always goes "all in" (using 100% of available capital)
///   on every signal. `Kelly_fraction` is computed but never applied to position sizing.
pub struct Backtester {
    initial_capital: f64,
    fee_pct: f64,
    /// Slippage in basis points (bps). Default: 5 bps = 0.05%.
    /// Applied to the fill price: buys pay more, sells receive less.
    slippage_bps: f64,
}

impl Backtester {
    pub fn new(initial_capital: f64, fee_pct: f64, slippage_bps: f64) -> Self {
        Self {
            initial_capital,
            fee_pct,
            slippage_bps,
        }
    }

    /// Create a backtester with sensible defaults (5 bps slippage).
    pub fn with_defaults(initial_capital: f64) -> Self {
        Self::new(initial_capital, 0.001, 5.0)
    }

    /// Convert slippage_bps to a multiplier factor (e.g. 5 bps → 0.0005).
    #[inline]
    fn slippage_factor(&self) -> f64 {
        self.slippage_bps / 10_000.0
    }

    pub fn run(&self, df: &DataFrame, signal: &Series, trailing_sl: f64) -> Result<BacktestResult> {
        let closes = df.column("close")?.f64()?;
        let highs = df.column("high")?.f64()?;
        let lows = df.column("low")?.f64()?;
        let signals = signal.f64()?;

        let slip = self.slippage_factor();

        let mut equity = self.initial_capital;
        let mut position = 0.0;
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
                    let notional = equity;
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
                }
            } else if position < 0.0 {
                if low < lowest_price_in_trade {
                    lowest_price_in_trade = low;
                }
                let stop_price = lowest_price_in_trade * (1.0 + trailing_sl);
                if high > stop_price {
                    // Stop triggered: fill at the stop price (not close)
                    let notional = equity;
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
                    let notional = equity;
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
                    entry_price = exec_price;
                    highest_price_in_trade = price;
                    lowest_price_in_trade = price;
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
                equity * (1.0 + current_pnl_pct)
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
        })
    }
}

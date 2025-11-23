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
}

pub struct Backtester {
    initial_capital: f64,
    fee_pct: f64,
    slippage_pct: f64,
}

impl Backtester {
    pub fn new(initial_capital: f64, fee_pct: f64, slippage_pct: f64) -> Self {
        Self {
            initial_capital,
            fee_pct,
            slippage_pct,
        }
    }

    pub fn run(&self, df: &DataFrame, signal: &Series, trailing_sl: f64) -> Result<BacktestResult> {
        let closes = df.column("close")?.f64()?;
        let highs = df.column("high")?.f64()?;
        let lows = df.column("low")?.f64()?;
        let signals = signal.f64()?;

        let mut equity = self.initial_capital;
        let mut position = 0.0;
        let mut entry_price = 0.0;
        let mut highest_price_in_trade = 0.0;
        let mut lowest_price_in_trade = 0.0;

        let mut wins = 0;
        let mut losses = 0;
        let mut gross_profit = 0.0;
        let mut gross_loss = 0.0;
        let mut _returns_list: Vec<f64> = Vec::new();

        let mut peak_equity = equity;
        let mut max_drawdown = 0.0;

        let mut equity_curve = Vec::with_capacity(closes.len());

        for i in 0..closes.len() {
            let price = closes.get(i).unwrap_or(0.0);
            let high = highs.get(i).unwrap_or(price);
            let low = lows.get(i).unwrap_or(price);
            let sig = signals.get(i).unwrap_or(0.0);

            // Trailing stop logic
            if position > 0.0 {
                if high > highest_price_in_trade {
                    highest_price_in_trade = high;
                }
                let stop_price = highest_price_in_trade * (1.0 - trailing_sl);
                if price < stop_price {
                    let pnl_pct = (stop_price - entry_price) / entry_price;
                    let pnl_amount = equity * pnl_pct - (equity * self.fee_pct * 2.0);
                    equity += pnl_amount;

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
                if price > stop_price {
                    let pnl_pct = (entry_price - stop_price) / entry_price;
                    let pnl_amount = equity * pnl_pct - (equity * self.fee_pct * 2.0);
                    equity += pnl_amount;

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

            if (sig - position).abs() > 0.01 {
                let exec_price = if sig > position {
                    price * (1.0 + self.slippage_pct)
                } else {
                    price * (1.0 - self.slippage_pct)
                };

                if position.abs() > 0.01 {
                    let raw_pnl_pct = if position > 0.0 {
                        (exec_price - entry_price) / entry_price
                    } else {
                        (entry_price - exec_price) / entry_price
                    };

                    let net_pnl_pct = raw_pnl_pct - (self.fee_pct * 2.0);
                    let pnl_amount = equity * net_pnl_pct;

                    equity += pnl_amount;
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

            if equity > peak_equity {
                peak_equity = equity;
            }
            let dd = (peak_equity - equity) / peak_equity;
            if dd > max_drawdown {
                max_drawdown = dd;
            }

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
        } else if total_trades > 0 { 1.0 } else { 0.0 };

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
        })
    }
}

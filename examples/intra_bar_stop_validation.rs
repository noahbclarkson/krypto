//! Intra-Bar Stop Validation — Is the edge real or optimistic execution?
//!
//! PROBLEM: Daily backtest assumes trailing stop benefits from the high
//! before checking if the low hit the stop. But we don't know the sequence.
//!
//! This example:
//! 1. Fetches 1d data + 30m data for same period
//! 2. Runs "optimistic" backtest (current behavior)
//! 3. Runs "realistic" backtest (walk through 30m candles in order)
//! 4. Compares Sharpe ratios to see how much of the edge is from optimistic execution
//!
//! Usage:
//!   cargo run --profile sweep --example intra_bar_stop_validation

use anyhow::Result;
use krypto::{
    algo::strategies::BollingerReversion, algo::SignalGenerator, backtest::engine::Backtester,
    data::loader::DataLoader, features::indicators::FeatureEngine,
};
use polars::prelude::*;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];
const CANDLES_1D: u32 = 2000; // ~8 years
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.3;

/// Compute ATR-based trailing stop from the last bar
fn compute_atr_stop(df: &DataFrame, atr_mult: f64) -> f64 {
    let atr = df
        .column("atr")
        .ok()
        .and_then(|s| {
            s.f64()
                .ok()
                .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
        })
        .unwrap_or(0.0);
    let close = df
        .column("close")
        .ok()
        .and_then(|s| {
            s.f64()
                .ok()
                .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
        })
        .unwrap_or(1.0);
    if close > 0.0 {
        (atr * atr_mult / close).clamp(0.005, 0.30)
    } else {
        0.05
    }
}

/// Result from realistic intra-bar backtest
struct IntraBarResult {
    total_return_pct: f64,
    annualised_sharpe: f64,
    total_trades: usize,
    max_drawdown: f64,
    optimistic_exits: usize,  // Exits that benefited from optimistic sequence
    pessimistic_exits: usize, // Exits that were worse due to actual sequence
}

/// Run backtest with intra-bar stop resolution using 30m data.
///
/// Key insight: Within a 30m bar, we STILL don't know if high came before low.
/// So we use a CONSERVATIVE model: if the stop could have been hit, it WAS hit.
///
/// For each 30m bar while in a position:
/// - Long: if low <= stop_price, exit at stop (we don't get the high's benefit)
/// - Short: if high >= stop_price, exit at stop (we don't get the low's benefit)
fn run_with_intra_bar_stops(
    df_1d: &DataFrame,
    df_30m: &DataFrame,
    signals: &[f64],
    trailing_sl: f64,
) -> Result<IntraBarResult> {
    let closes_1d = df_1d.column("close")?.f64()?;
    let times_1d = df_1d.column("time")?.datetime()?;
    let highs_30m = df_30m.column("high")?.f64()?;
    let lows_30m = df_30m.column("low")?.f64()?;
    let times_30m = df_30m.column("time")?.datetime()?;

    let n_1d = df_1d.height();
    let n_30m = df_30m.height();

    // Build a mapping: 1d_bar_index -> Vec<30m_bar_indices>
    // Each 1d bar at time T contains 30m bars from T (inclusive) to T+1d (exclusive)
    let mut daily_to_30m: Vec<Vec<usize>> = vec![Vec::new(); n_1d];
    {
        let mut current_1d_idx = 0;
        let mut current_1d_start = if n_1d > 0 {
            times_1d.get(0).unwrap_or(0)
        } else {
            0
        };
        let mut current_1d_end = current_1d_start + 24 * 60 * 60 * 1000;

        for i in 0..n_30m {
            let t_30m = times_30m.get(i).unwrap_or(0);

            // Find which 1d bar this 30m bar belongs to
            while t_30m >= current_1d_end && current_1d_idx < n_1d - 1 {
                current_1d_idx += 1;
                current_1d_start = times_1d.get(current_1d_idx).unwrap_or(0);
                current_1d_end = current_1d_start + 24 * 60 * 60 * 1000;
            }

            // Only add if within the day (not the first bar which might be from previous day)
            if t_30m >= current_1d_start && current_1d_idx < n_1d {
                daily_to_30m[current_1d_idx].push(i);
            }
        }
    }

    // Run backtest with intra-bar resolution
    let mut equity = CAPITAL;
    let mut position = 0.0;
    let mut position_size = 0.0;
    let mut entry_price = 0.0;
    let mut highest_price_in_trade = 0.0;
    let mut lowest_price_in_trade = 0.0;
    let mut entry_daily_bar = 0;

    let mut peak_equity = equity;
    let mut max_dd = 0.0;
    let mut trade_returns: Vec<f64> = Vec::new();

    let mut optimistic_exits = 0;
    let mut pessimistic_exits = 0;

    for i in 0..n_1d {
        let close_1d = closes_1d.get(i).unwrap_or(0.0);
        let high_1d = df_1d.column("high")?.f64()?.get(i).unwrap_or(0.0);
        let low_1d = df_1d.column("low")?.f64()?.get(i).unwrap_or(0.0);

        // Entry logic (same as standard backtest)
        if signals[i] != 0.0 && position == 0.0 {
            position = signals[i];
            position_size = 1.0;
            entry_price = close_1d;
            entry_daily_bar = i;
            highest_price_in_trade = entry_price;
            lowest_price_in_trade = entry_price;
        }

        // If in a position, walk through 30m bars of THIS day to resolve stops
        // Note: entry happened at close of PREVIOUS bar, so we're checking
        // the 30m bars of the current bar
        if position != 0.0 {
            let bar_30m_indices = &daily_to_30m[i];

            // Track the best possible trailing stop (optimistic case)
            let mut optimistic_highest = highest_price_in_trade;
            let mut optimistic_lowest = lowest_price_in_trade;

            for &j in bar_30m_indices {
                let high_30m = highs_30m.get(j).unwrap_or(0.0);
                let low_30m = lows_30m.get(j).unwrap_or(0.0);

                if position > 0.0 {
                    // Long position
                    // CONSERVATIVE: check if stop was hit BEFORE updating trailing
                    let current_stop = highest_price_in_trade * (1.0 - trailing_sl);

                    if low_30m <= current_stop {
                        // Stop was hit! Exit at stop price
                        let notional = equity * position_size;
                        let fee = notional * TAKER_FEE * 2.0;
                        let pnl_pct = (current_stop - entry_price) / entry_price;
                        let pnl_amount = notional * pnl_pct - fee;

                        equity += pnl_amount;
                        trade_returns.push(pnl_pct);

                        // Check if optimistic would have been better
                        let optimistic_stop =
                            optimistic_highest.max(high_30m) * (1.0 - trailing_sl);
                        if low_1d > optimistic_stop {
                            // Optimistic would have survived (high came first in daily)
                            pessimistic_exits += 1;
                        }

                        position = 0.0;
                        position_size = 0.0;
                        break;
                    }

                    // Update trailing AFTER checking stop (conservative)
                    if high_30m > highest_price_in_trade {
                        highest_price_in_trade = high_30m;
                    }
                    optimistic_highest = optimistic_highest.max(high_30m);
                } else if position < 0.0 {
                    // Short position
                    let current_stop = lowest_price_in_trade * (1.0 + trailing_sl);

                    if high_30m >= current_stop {
                        let notional = equity * position_size;
                        let fee = notional * TAKER_FEE * 2.0;
                        let pnl_pct = (entry_price - current_stop) / entry_price;
                        let pnl_amount = notional * pnl_pct - fee;

                        equity += pnl_amount;
                        trade_returns.push(pnl_pct);

                        let optimistic_stop = optimistic_lowest.min(low_30m) * (1.0 + trailing_sl);
                        if high_1d < optimistic_stop {
                            pessimistic_exits += 1;
                        }

                        position = 0.0;
                        position_size = 0.0;
                        break;
                    }

                    if low_30m < lowest_price_in_trade {
                        lowest_price_in_trade = low_30m;
                    }
                    optimistic_lowest = optimistic_lowest.min(low_30m);
                }
            }

            // If still in position at end of day, check signal exit
            if position != 0.0 && signals[i] == 0.0 {
                let notional = equity * position_size;
                let fee = notional * TAKER_FEE * 2.0;
                let pnl_pct = if position > 0.0 {
                    (close_1d - entry_price) / entry_price
                } else {
                    (entry_price - close_1d) / entry_price
                };
                let pnl_amount = notional * pnl_pct - fee;

                equity += pnl_amount;
                trade_returns.push(pnl_pct);

                optimistic_exits += 1;
                position = 0.0;
                position_size = 0.0;
            }
        }

        // Update drawdown
        if equity > peak_equity {
            peak_equity = equity;
        }
        let dd = (peak_equity - equity) / peak_equity;
        if dd > max_dd {
            max_dd = dd;
        }
    }

    // Close any remaining position
    if position != 0.0 {
        let last_close = closes_1d.get(n_1d.saturating_sub(1)).unwrap_or(0.0);
        let notional = equity * position_size;
        let fee = notional * TAKER_FEE * 2.0;
        let pnl_pct = if position > 0.0 {
            (last_close - entry_price) / entry_price
        } else {
            (entry_price - last_close) / entry_price
        };
        let pnl_amount = notional * pnl_pct - fee;

        equity += pnl_amount;
        trade_returns.push(pnl_pct);
    }

    let total_return_pct = (equity - CAPITAL) / CAPITAL * 100.0;
    let total_trades = trade_returns.len();

    // Compute time span
    let backtest_years = if n_1d > 0 {
        let first = times_1d.get(0).unwrap_or(0);
        let last = times_1d.get(n_1d.saturating_sub(1)).unwrap_or(0);
        let diff_ms = (last - first).abs() as f64;
        diff_ms / (365.25 * 24.0 * 3600.0 * 1000.0)
    } else {
        0.0
    };

    let trades_per_year = if backtest_years > 0.0 {
        total_trades as f64 / backtest_years
    } else {
        0.0
    };

    // Sharpe = total_return / max_dd, annualized
    let sharpe = if max_dd > 0.0 {
        total_return_pct / 100.0 / max_dd
    } else {
        0.0
    };
    let annualised_sharpe = if trades_per_year > 0.0 {
        sharpe * trades_per_year.sqrt()
    } else {
        0.0
    };

    Ok(IntraBarResult {
        total_return_pct,
        annualised_sharpe,
        total_trades,
        max_drawdown: max_dd * 100.0,
        optimistic_exits,
        pessimistic_exits,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80));
    println!("  INTRA-BAR STOP VALIDATION");
    println!("  Testing if the edge is real or from optimistic execution");
    println!("{}", "━".repeat(80));
    println!("\n  Method: Compare daily backtest vs 30m intra-bar resolution");
    println!("  Conservative model: check stop BEFORE updating trailing within each 30m bar");
    println!("  If Sharpe drops significantly, the edge is from optimistic execution.\n");

    let loader = DataLoader::new(None, None);
    let strat = BollingerReversion::new();

    // Results table
    println!(
        "{:<12} {:>10} {:>10} {:>8} {:>10} {:>10} {:>6}",
        "Symbol", "Opt Sharpe", "Real Sharpe", "DiffPct", "Opt Ret", "Real Ret", "Trades"
    );
    println!("{}", "-".repeat(80));

    let mut opt_sharpes: Vec<f64> = Vec::new();
    let mut real_sharpes: Vec<f64> = Vec::new();
    let mut total_pessimistic = 0;

    for symbol in SYMBOLS {
        // Fetch 1d data
        print!("{}: fetching 1d... ", symbol);
        let raw_1d = match loader.fetch_data(symbol, "1d", CANDLES_1D).await {
            Ok(d) => d,
            Err(e) => {
                println!("SKIP ({})", e);
                continue;
            }
        };
        let df_1d = FeatureEngine::add_technicals(&raw_1d, None)?;
        let n_1d = df_1d.height();
        print!("{} bars, 30m... ", n_1d);

        // Fetch 30m data (48x more bars)
        let candles_30m = CANDLES_1D * 48;
        let df_30m = match loader.fetch_data(symbol, "30m", candles_30m).await {
            Ok(d) => d,
            Err(e) => {
                println!("30m fetch failed: {}", e);
                continue;
            }
        };
        let n_30m = df_30m.height();
        println!("{} bars", n_30m);

        // Generate signals on 1d data
        let signals_series = strat.predict(&df_1d)?;
        let signals: Vec<f64> = signals_series
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect();

        let trailing_sl = compute_atr_stop(&df_1d, ATR_MULT);

        // Run optimistic backtest (current behavior)
        let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
        let opt_result = bt.run(&df_1d, &signals_series, trailing_sl, 0.0)?;

        // Run realistic backtest (intra-bar resolution)
        let real_result = run_with_intra_bar_stops(&df_1d, &df_30m, &signals, trailing_sl)?;

        let diff_pct = if opt_result.annualised_sharpe > 0.0 {
            (real_result.annualised_sharpe - opt_result.annualised_sharpe)
                / opt_result.annualised_sharpe
                * 100.0
        } else {
            0.0
        };

        println!(
            "{:<12} {:>10.1} {:>10.1} {:>7.1}pct {:>9.1}% {:>9.1}% {:>6}",
            symbol,
            opt_result.annualised_sharpe,
            real_result.annualised_sharpe,
            diff_pct,
            opt_result.total_return_pct,
            real_result.total_return_pct,
            real_result.total_trades
        );

        println!(
            "             pessimistic_exits: {}, optimistic_exits: {}, max_dd: {:.1}%",
            real_result.pessimistic_exits, real_result.optimistic_exits, real_result.max_drawdown
        );

        opt_sharpes.push(opt_result.annualised_sharpe);
        real_sharpes.push(real_result.annualised_sharpe);
        total_pessimistic += real_result.pessimistic_exits;
    }

    // Summary
    println!("\n{}", "━".repeat(80));
    if !opt_sharpes.is_empty() {
        let avg_opt: f64 = opt_sharpes.iter().sum::<f64>() / opt_sharpes.len() as f64;
        let avg_real: f64 = real_sharpes.iter().sum::<f64>() / real_sharpes.len() as f64;
        let avg_diff = if avg_opt > 0.0 {
            (avg_real - avg_opt) / avg_opt * 100.0
        } else {
            0.0
        };

        println!("  Average Optimistic Sharpe:  {:.1}", avg_opt);
        println!("  Average Realistic Sharpe:   {:.1}", avg_real);
        println!("  Difference:                 {:.1}pct", avg_diff);
        println!(
            "  Total pessimistic exits:    {} (stops hit before trailing benefit)",
            total_pessimistic
        );

        if avg_diff < -20.0 {
            println!("\n  WARNING: SIGNIFICANT EDGE EROSION - realistic execution reduces Sharpe by >20pct");
        } else if avg_diff < -10.0 {
            println!("\n  WARNING: MODERATE EDGE EROSION - realistic execution reduces Sharpe by 10-20pct");
        } else {
            println!("\n  OK: EDGE SURVIVES - realistic execution similar to optimistic");
        }
    }
    println!("{}", "━".repeat(80));

    Ok(())
}

//! Quantified Intra-Bar Impact — How much does optimistic execution inflate returns?
//!
//! Finding: Daily high/low sequence is 51%/49% (coin flip)
//!
//! This example:
//! 1. Runs the optimistic backtest (current behavior)
//! 2. For each trade, uses 30m data to determine ACTUAL exit price
//! 3. Computes realistic return
//!
//! Usage:
//!   cargo run --profile sweep --example intra_bar_impact_quantified

use anyhow::Result;
use krypto::{
    algo::strategies::BollingerReversion, algo::SignalGenerator, backtest::engine::Backtester,
    data::loader::DataLoader, features::indicators::FeatureEngine,
};
use polars::prelude::*;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT"];
const CANDLES_1D: u32 = 2000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.3;

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

#[derive(Debug)]
struct TradeOutcome {
    entry_price: f64,
    optimistic_exit: f64, // What the daily backtest says
    realistic_exit: f64,  // What 30m data says
    direction: f64,       // 1.0 = long, -1.0 = short
    optimistic_pnl_pct: f64,
    realistic_pnl_pct: f64,
    exit_type: String, // "stop", "signal", "eod"
}

/// Walk through 30m bars to find the ACTUAL exit for a trade
/// Returns (exit_price, exit_type)
fn find_realistic_exit(
    highs_30m: &[f64],
    lows_30m: &[f64],
    entry_price: f64,
    direction: f64,
    trailing_sl: f64,
) -> (f64, String) {
    let mut highest = entry_price;
    let mut lowest = entry_price;

    for i in 0..highs_30m.len() {
        let h = highs_30m[i];
        let l = lows_30m[i];

        if direction > 0.0 {
            // Long - check if stop hit BEFORE updating trailing
            let stop = highest * (1.0 - trailing_sl);
            if l <= stop {
                return (stop, "stop".to_string());
            }
            // Update trailing
            if h > highest {
                highest = h;
            }
        } else {
            // Short - check if stop hit BEFORE updating trailing
            let stop = lowest * (1.0 + trailing_sl);
            if h >= stop {
                return (stop, "stop".to_string());
            }
            // Update trailing
            if l < lowest {
                lowest = l;
            }
        }
    }

    // No stop hit - exit at last close (simulated)
    let last_close = if direction > 0.0 {
        highs_30m.last().copied().unwrap_or(entry_price)
    } else {
        lows_30m.last().copied().unwrap_or(entry_price)
    };
    (last_close, "eod".to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80));
    println!("  INTRA-BAR IMPACT QUANTIFIED");
    println!("  Measuring actual return erosion from realistic execution");
    println!("{}", "━".repeat(80));
    println!("\n  Finding: Daily high/low sequence is 51pct/49pct (coin flip)");
    println!("  Method: Compare optimistic daily exit vs realistic 30m sequence\n");

    let loader = DataLoader::new(None, None);
    let strat = BollingerReversion::new();

    println!(
        "{:<10} {:>8} {:>8} {:>8} {:>10} {:>10} {:>7}",
        "Symbol", "Opt Sharpe", "Real Sharpe", "Diff", "Opt Ret", "Real Ret", "Trades"
    );
    println!("{}", "-".repeat(80));

    let mut all_opt_sharpes: Vec<f64> = Vec::new();
    let mut all_real_sharpes: Vec<f64> = Vec::new();
    let mut all_opt_returns: Vec<f64> = Vec::new();
    let mut all_real_returns: Vec<f64> = Vec::new();

    for symbol in SYMBOLS {
        print!("{}: loading... ", symbol);

        // Fetch data
        let raw_1d = match loader.fetch_data(symbol, "1d", CANDLES_1D).await {
            Ok(d) => d,
            Err(e) => {
                println!("SKIP ({})", e);
                continue;
            }
        };
        let df_1d = FeatureEngine::add_technicals(&raw_1d, None)?;
        let n_1d = df_1d.height();

        let candles_30m = CANDLES_1D * 48;
        let df_30m = match loader.fetch_data(symbol, "30m", candles_30m).await {
            Ok(d) => d,
            Err(e) => {
                println!("30m fetch failed: {}", e);
                continue;
            }
        };
        let n_30m = df_30m.height();
        println!("1d={}, 30m={}", n_1d, n_30m);

        // Generate signals
        let signals_series = strat.predict(&df_1d)?;
        let signals: Vec<f64> = signals_series
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect();

        let trailing_sl = compute_atr_stop(&df_1d, ATR_MULT);

        // Run optimistic backtest
        let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
        let opt_result = bt.run(&df_1d, &signals_series, trailing_sl, 0.0)?;

        // Build 30m bar mapping per day
        let times_1d = df_1d.column("time")?.datetime()?;
        let times_30m = df_30m.column("time")?.datetime()?;
        let highs_30m = df_30m.column("high")?.f64()?;
        let lows_30m = df_30m.column("low")?.f64()?;
        let closes_1d = df_1d.column("close")?.f64()?;

        // Map each 1d bar to its 30m bars
        let mut day_to_30m: Vec<Vec<usize>> = vec![Vec::new(); n_1d];
        let mut current_day_idx = 0;
        let mut current_day_end = times_1d.get(0).unwrap_or(0) + 24 * 60 * 60 * 1000;

        for i in 0..n_30m {
            let t = times_30m.get(i).unwrap_or(0);
            while t >= current_day_end && current_day_idx < n_1d - 1 {
                current_day_idx += 1;
                current_day_end = times_1d.get(current_day_idx).unwrap_or(0) + 24 * 60 * 60 * 1000;
            }
            if current_day_idx < n_1d {
                day_to_30m[current_day_idx].push(i);
            }
        }

        // Simulate realistic trades
        let mut trades: Vec<TradeOutcome> = Vec::new();
        let mut position = 0.0;
        let mut entry_price = 0.0;
        let mut entry_bar = 0;
        let mut highest_in_trade = 0.0;
        let mut lowest_in_trade = 0.0;

        for i in 0..n_1d {
            let close = closes_1d.get(i).unwrap_or(0.0);

            // Entry
            if signals[i] != 0.0 && position == 0.0 {
                position = signals[i];
                entry_price = close;
                entry_bar = i;
                highest_in_trade = entry_price;
                lowest_in_trade = entry_price;
            }

            // If in position, check this day's 30m bars for realistic exit
            if position != 0.0 {
                let bars = &day_to_30m[i];
                let day_highs: Vec<f64> = bars
                    .iter()
                    .map(|&j| highs_30m.get(j).unwrap_or(0.0))
                    .collect();
                let day_lows: Vec<f64> = bars
                    .iter()
                    .map(|&j| lows_30m.get(j).unwrap_or(0.0))
                    .collect();

                let (real_exit, exit_type) = find_realistic_exit(
                    &day_highs,
                    &day_lows,
                    highest_in_trade, // Use current trailing level
                    position,
                    trailing_sl,
                );

                // Compute optimistic exit (daily bar, assumes high before low)
                let high_1d = df_1d.column("high")?.f64()?.get(i).unwrap_or(close);
                let low_1d = df_1d.column("low")?.f64()?.get(i).unwrap_or(close);

                let (opt_exit, stopped_opt) = if position > 0.0 {
                    // Optimistic: high first, update trailing, then check stop
                    let new_highest = highest_in_trade.max(high_1d);
                    let opt_stop = new_highest * (1.0 - trailing_sl);
                    if low_1d <= opt_stop {
                        (opt_stop, true)
                    } else {
                        highest_in_trade = new_highest;
                        (close, false) // No stop, continue
                    }
                } else {
                    let new_lowest = lowest_in_trade.min(low_1d);
                    let opt_stop = new_lowest * (1.0 + trailing_sl);
                    if high_1d >= opt_stop {
                        (opt_stop, true)
                    } else {
                        lowest_in_trade = new_lowest;
                        (close, false)
                    }
                };

                // If realistic hit stop but optimistic didn't, record the trade
                if exit_type == "stop" || stopped_opt || signals[i] == 0.0 {
                    let opt_pnl = if position > 0.0 {
                        (opt_exit - entry_price) / entry_price
                    } else {
                        (entry_price - opt_exit) / entry_price
                    };
                    let real_pnl = if position > 0.0 {
                        (real_exit - entry_price) / entry_price
                    } else {
                        (entry_price - real_exit) / entry_price
                    };

                    trades.push(TradeOutcome {
                        entry_price,
                        optimistic_exit: opt_exit,
                        realistic_exit: real_exit,
                        direction: position,
                        optimistic_pnl_pct: opt_pnl,
                        realistic_pnl_pct: real_pnl,
                        exit_type,
                    });

                    position = 0.0;
                }
            }
        }

        // Compute realistic equity curve
        let mut real_equity = CAPITAL;
        let mut peak = CAPITAL;
        let mut max_dd = 0.0;

        for trade in &trades {
            let notional = real_equity;
            let fee = notional * TAKER_FEE * 2.0;
            let pnl = notional * trade.realistic_pnl_pct - fee;
            real_equity += pnl;

            if real_equity > peak {
                peak = real_equity;
            }
            let dd = (peak - real_equity) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }

        let real_return_pct = (real_equity - CAPITAL) / CAPITAL * 100.0;
        let n_trades = trades.len();

        // Compute realistic Sharpe
        let backtest_years = if n_1d > 0 {
            let first = times_1d.get(0).unwrap_or(0);
            let last = times_1d.get(n_1d.saturating_sub(1)).unwrap_or(0);
            (last - first).abs() as f64 / (365.25 * 24.0 * 3600.0 * 1000.0)
        } else {
            1.0
        };
        let trades_per_year = n_trades as f64 / backtest_years;
        let real_sharpe = if max_dd > 0.0 {
            (real_return_pct / 100.0 / max_dd) * trades_per_year.sqrt()
        } else {
            0.0
        };

        let diff_pct = if opt_result.annualised_sharpe > 0.0 {
            (real_sharpe - opt_result.annualised_sharpe) / opt_result.annualised_sharpe * 100.0
        } else {
            0.0
        };

        println!(
            "{:<10} {:>8.1} {:>8.1} {:>7.1}pct {:>9.1}% {:>9.1}% {:>7}",
            symbol,
            opt_result.annualised_sharpe,
            real_sharpe,
            diff_pct,
            opt_result.total_return_pct,
            real_return_pct,
            n_trades
        );

        // Analyze trades
        let worse_trades = trades
            .iter()
            .filter(|t| t.realistic_pnl_pct < t.optimistic_pnl_pct)
            .count();
        let avg_erosion = if !trades.is_empty() {
            let total_opt: f64 = trades.iter().map(|t| t.optimistic_pnl_pct).sum();
            let total_real: f64 = trades.iter().map(|t| t.realistic_pnl_pct).sum();
            (total_real - total_opt) / trades.len() as f64 * 100.0
        } else {
            0.0
        };
        println!(
            "           worse exits: {}/{} ({:.0}pct), avg erosion: {:.2}pct per trade",
            worse_trades,
            n_trades,
            worse_trades as f64 / n_trades.max(1) as f64 * 100.0,
            avg_erosion
        );

        all_opt_sharpes.push(opt_result.annualised_sharpe);
        all_real_sharpes.push(real_sharpe);
        all_opt_returns.push(opt_result.total_return_pct);
        all_real_returns.push(real_return_pct);
    }

    // Summary
    println!("\n{}", "━".repeat(80));
    if !all_opt_sharpes.is_empty() {
        let avg_opt_sharpe: f64 =
            all_opt_sharpes.iter().sum::<f64>() / all_opt_sharpes.len() as f64;
        let avg_real_sharpe: f64 =
            all_real_sharpes.iter().sum::<f64>() / all_real_sharpes.len() as f64;
        let avg_opt_ret: f64 = all_opt_returns.iter().sum::<f64>() / all_opt_returns.len() as f64;
        let avg_real_ret: f64 =
            all_real_returns.iter().sum::<f64>() / all_real_returns.len() as f64;

        let sharpe_erosion = (avg_real_sharpe - avg_opt_sharpe) / avg_opt_sharpe * 100.0;
        let return_erosion = (avg_real_ret - avg_opt_ret) / avg_opt_ret * 100.0;

        println!("  Average Optimistic Sharpe:  {:.1}", avg_opt_sharpe);
        println!("  Average Realistic Sharpe:   {:.1}", avg_real_sharpe);
        println!("  Sharpe Erosion:             {:.1}pct", sharpe_erosion);
        println!();
        println!("  Average Optimistic Return:  {:.1}pct", avg_opt_ret);
        println!("  Average Realistic Return:   {:.1}pct", avg_real_ret);
        println!("  Return Erosion:             {:.1}pct", return_erosion);

        if sharpe_erosion < -30.0 {
            println!("\n  CRITICAL: Edge largely from optimistic execution");
        } else if sharpe_erosion < -15.0 {
            println!("\n  WARNING: Significant edge erosion from realistic execution");
        } else {
            println!("\n  OK: Edge survives realistic execution");
        }
    }
    println!("{}", "━".repeat(80));

    Ok(())
}

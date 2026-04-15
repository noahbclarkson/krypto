//! Simple Intra-Bar Analysis — Impact on trailing stops
//!
//! Key question: When a trailing stop would be hit on a given day,
//! what's the difference between optimistic (high first) vs realistic (unknown sequence)?
//!
//! This analyzes the WORST CASE: assume low always comes before high.
//! If the edge survives even this, it's robust.

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

/// Simulate trade with WORST-CASE intra-bar assumption
/// (low always comes before high - maximum pessimism)
fn simulate_worst_case(
    df_1d: &DataFrame,
    df_30m: &DataFrame,
    signals: &[f64],
    trailing_sl: f64,
) -> Result<(f64, usize, f64)> {
    let n_1d = df_1d.height();
    let closes_1d = df_1d.column("close")?.f64()?;
    let highs_1d = df_1d.column("high")?.f64()?;
    let lows_1d = df_1d.column("low")?.f64()?;
    let times_1d = df_1d.column("time")?.datetime()?;

    let times_30m = df_30m.column("time")?.datetime()?;
    let highs_30m = df_30m.column("high")?.f64()?;
    let lows_30m = df_30m.column("low")?.f64()?;
    let n_30m = df_30m.height();

    // Build day -> 30m mapping
    let mut day_to_30m: Vec<(usize, usize)> = vec![(0, 0); n_1d]; // (start_idx, end_idx exclusive)
    let mut day_idx = 0;
    let mut day_start = 0;
    let mut day_end_ts = times_1d.get(0).unwrap_or(0) + 24 * 60 * 60 * 1000;

    for i in 0..n_30m {
        let t = times_30m.get(i).unwrap_or(0);

        // Check if we've moved to a new day
        while t >= day_end_ts && day_idx < n_1d - 1 {
            day_to_30m[day_idx] = (day_start, i);
            day_idx += 1;
            day_start = i;
            day_end_ts = times_1d.get(day_idx).unwrap_or(0) + 24 * 60 * 60 * 1000;
        }
    }
    // Final day
    day_to_30m[day_idx] = (day_start, n_30m);

    // Simulate trades with worst-case assumption
    let mut equity = CAPITAL;
    let mut position = 0.0;
    let mut entry_price = 0.0;
    let mut trail_ref = 0.0; // highest (long) or lowest (short) seen in trade
    let mut n_trades = 0;
    let mut peak = equity;
    let mut max_dd = 0.0;

    for i in 0..n_1d {
        let close = closes_1d.get(i).unwrap_or(0.0);
        let high = highs_1d.get(i).unwrap_or(0.0);
        let low = lows_1d.get(i).unwrap_or(0.0);

        // Entry
        if signals[i] != 0.0 && position == 0.0 {
            position = signals[i];
            entry_price = close;
            trail_ref = close;
            continue;
        }

        // If in position
        if position != 0.0 {
            let (start_30m, end_30m) = day_to_30m[i];

            // WORST CASE: assume low comes before high in each 30m bar
            // For long: check if low <= stop BEFORE updating trail
            // For short: check if high >= stop BEFORE updating trail

            let mut stopped = false;
            let mut exit_price = close;

            for j in start_30m..end_30m {
                let h = highs_30m.get(j).unwrap_or(0.0);
                let l = lows_30m.get(j).unwrap_or(0.0);

                if position > 0.0 {
                    // Long: check stop FIRST
                    let stop = trail_ref * (1.0 - trailing_sl);
                    if l <= stop {
                        stopped = true;
                        exit_price = stop;
                        break;
                    }
                    // Then update trail
                    if h > trail_ref {
                        trail_ref = h;
                    }
                } else {
                    // Short: check stop FIRST
                    let stop = trail_ref * (1.0 + trailing_sl);
                    if h >= stop {
                        stopped = true;
                        exit_price = stop;
                        break;
                    }
                    // Then update trail
                    if l < trail_ref {
                        trail_ref = l;
                    }
                }
            }

            // Exit conditions
            if stopped || signals[i] == 0.0 {
                if !stopped {
                    exit_price = close; // Signal exit at close
                }

                // Compute PnL
                let pnl_pct = if position > 0.0 {
                    (exit_price - entry_price) / entry_price
                } else {
                    (entry_price - exit_price) / entry_price
                };
                let notional = equity;
                let fee = notional * TAKER_FEE * 2.0;
                equity += notional * pnl_pct - fee;
                n_trades += 1;

                // Update DD
                if equity > peak {
                    peak = equity;
                }
                let dd = (peak - equity) / peak;
                if dd > max_dd {
                    max_dd = dd;
                }

                position = 0.0;
            }
        }
    }

    // Close remaining
    if position != 0.0 {
        let last_close = closes_1d.get(n_1d.saturating_sub(1)).unwrap_or(0.0);
        let pnl_pct = if position > 0.0 {
            (last_close - entry_price) / entry_price
        } else {
            (entry_price - last_close) / entry_price
        };
        let notional = equity;
        let fee = notional * TAKER_FEE * 2.0;
        equity += notional * pnl_pct - fee;
        n_trades += 1;
    }

    let return_pct = (equity - CAPITAL) / CAPITAL * 100.0;
    Ok((return_pct, n_trades, max_dd * 100.0))
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80));
    println!("  INTRA-BAR WORST-CASE ANALYSIS");
    println!("  Assuming low always comes before high (maximum pessimism)");
    println!("{}", "━".repeat(80));
    println!("\n  If the edge survives even this pessimistic assumption, it's robust.\n");

    let loader = DataLoader::new(None, None);
    let strat = BollingerReversion::new();

    println!(
        "{:<10} {:>10} {:>10} {:>8} {:>10} {:>10} {:>6}",
        "Symbol", "Opt Sharpe", "WC Sharpe", "Diff", "Opt Ret", "WC Ret", "Trades"
    );
    println!("{}", "-".repeat(80));

    let mut opt_sharpes: Vec<f64> = Vec::new();
    let mut wc_sharpes: Vec<f64> = Vec::new();

    for symbol in SYMBOLS {
        print!("{}: loading... ", symbol);

        let raw_1d = match loader.fetch_data(symbol, "1d", CANDLES_1D).await {
            Ok(d) => d,
            Err(e) => {
                println!("SKIP ({})", e);
                continue;
            }
        };
        let df_1d = FeatureEngine::add_technicals(&raw_1d, None)?;
        let n_1d = df_1d.height();

        let df_30m = match loader.fetch_data(symbol, "30m", CANDLES_1D * 48).await {
            Ok(d) => d,
            Err(e) => {
                println!("30m failed: {}", e);
                continue;
            }
        };
        println!("1d={}, 30m={}", n_1d, df_30m.height());

        let signals_series = strat.predict(&df_1d)?;
        let signals: Vec<f64> = signals_series
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect();
        let trailing_sl = compute_atr_stop(&df_1d, ATR_MULT);

        // Optimistic
        let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
        let opt = bt.run(&df_1d, &signals_series, trailing_sl, 0.0)?;

        // Worst-case
        let (wc_ret, n_trades, wc_dd) =
            simulate_worst_case(&df_1d, &df_30m, &signals, trailing_sl)?;

        // Compute WC Sharpe
        let times_1d = df_1d.column("time")?.datetime()?;
        let years = if n_1d > 0 {
            let first = times_1d.get(0).unwrap_or(0);
            let last = times_1d.get(n_1d.saturating_sub(1)).unwrap_or(0);
            (last - first).abs() as f64 / (365.25 * 24.0 * 3600.0 * 1000.0)
        } else {
            1.0
        };
        let trades_per_year = n_trades as f64 / years;
        let wc_sharpe = if wc_dd > 0.0 {
            (wc_ret / 100.0 / (wc_dd / 100.0)) * trades_per_year.sqrt()
        } else {
            0.0
        };

        let diff = if opt.annualised_sharpe > 0.0 {
            (wc_sharpe - opt.annualised_sharpe) / opt.annualised_sharpe * 100.0
        } else {
            0.0
        };

        println!(
            "{:<10} {:>10.1} {:>10.1} {:>7.1}pct {:>9.1}% {:>9.1}% {:>6}",
            symbol, opt.annualised_sharpe, wc_sharpe, diff, opt.total_return_pct, wc_ret, n_trades
        );

        opt_sharpes.push(opt.annualised_sharpe);
        wc_sharpes.push(wc_sharpe);
    }

    println!("\n{}", "━".repeat(80));
    if !opt_sharpes.is_empty() {
        let avg_opt: f64 = opt_sharpes.iter().sum::<f64>() / opt_sharpes.len() as f64;
        let avg_wc: f64 = wc_sharpes.iter().sum::<f64>() / wc_sharpes.len() as f64;
        let erosion = if avg_opt > 0.0 {
            (avg_wc - avg_opt) / avg_opt * 100.0
        } else {
            0.0
        };

        println!("  Avg Optimistic Sharpe:  {:.1}", avg_opt);
        println!("  Avg Worst-Case Sharpe:  {:.1}", avg_wc);
        println!("  Erosion:                {:.1}pct", erosion);

        if erosion < -50.0 {
            println!("\n  CRITICAL: Edge mostly from optimistic execution");
        } else if erosion < -20.0 {
            println!("\n  WARNING: Significant erosion from realistic execution");
        } else {
            println!("\n  OK: Edge survives worst-case execution");
        }
    }
    println!("{}", "━".repeat(80));

    Ok(())
}

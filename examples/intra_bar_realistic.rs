//! Realistic Intra-Bar Analysis — Using actual 30m sequence
//!
//! Previous findings:
//! - Optimistic (high before low): +642-30744% returns
//! - Worst-Case (low before high): -23% to -49% returns
//! - Reality: 51% high first, 49% low first
//!
//! This example simulates using the ACTUAL 30m sequence for each day.

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

/// Simulate using ACTUAL 30m sequence (walk bars in order)
fn simulate_realistic(
    df_1d: &DataFrame,
    df_30m: &DataFrame,
    signals: &[f64],
    trailing_sl: f64,
) -> Result<(f64, usize, f64)> {
    let n_1d = df_1d.height();
    let closes_1d = df_1d.column("close")?.f64()?;
    let times_1d = df_1d.column("time")?.datetime()?;

    let times_30m = df_30m.column("time")?.datetime()?;
    let highs_30m = df_30m.column("high")?.f64()?;
    let lows_30m = df_30m.column("low")?.f64()?;
    let n_30m = df_30m.height();

    // Build day -> 30m mapping
    let mut day_to_30m: Vec<(usize, usize)> = vec![(0, 0); n_1d];
    let mut day_idx = 0;
    let mut day_start = 0;
    let mut day_end_ts = times_1d.get(0).unwrap_or(0) + 24 * 60 * 60 * 1000;

    for i in 0..n_30m {
        let t = times_30m.get(i).unwrap_or(0);
        while t >= day_end_ts && day_idx < n_1d - 1 {
            day_to_30m[day_idx] = (day_start, i);
            day_idx += 1;
            day_start = i;
            day_end_ts = times_1d.get(day_idx).unwrap_or(0) + 24 * 60 * 60 * 1000;
        }
    }
    day_to_30m[day_idx] = (day_start, n_30m);

    let mut equity = CAPITAL;
    let mut position = 0.0;
    let mut entry_price = 0.0;
    let mut trail_ref = 0.0;
    let mut n_trades = 0;
    let mut peak = equity;
    let mut max_dd = 0.0;

    for i in 0..n_1d {
        let close = closes_1d.get(i).unwrap_or(0.0);

        if signals[i] != 0.0 && position == 0.0 {
            position = signals[i];
            entry_price = close;
            trail_ref = close;
            continue;
        }

        if position != 0.0 {
            let (start_30m, end_30m) = day_to_30m[i];
            let mut stopped = false;
            let mut exit_price = close;

            // Walk 30m bars in ACTUAL sequence
            for j in start_30m..end_30m {
                let h = highs_30m.get(j).unwrap_or(0.0);
                let l = lows_30m.get(j).unwrap_or(0.0);

                if position > 0.0 {
                    // Within this 30m bar, we still don't know if h or l came first
                    // Conservative: if stop could be hit, assume it was
                    let stop = trail_ref * (1.0 - trailing_sl);
                    if l <= stop {
                        stopped = true;
                        exit_price = stop;
                        break;
                    }
                    // Update trail (optimistic within bar, but we checked stop first)
                    if h > trail_ref {
                        trail_ref = h;
                    }
                } else {
                    let stop = trail_ref * (1.0 + trailing_sl);
                    if h >= stop {
                        stopped = true;
                        exit_price = stop;
                        break;
                    }
                    if l < trail_ref {
                        trail_ref = l;
                    }
                }
            }

            if stopped || signals[i] == 0.0 {
                if !stopped {
                    exit_price = close;
                }

                let pnl_pct = if position > 0.0 {
                    (exit_price - entry_price) / entry_price
                } else {
                    (entry_price - exit_price) / entry_price
                };
                let notional = equity;
                let fee = notional * TAKER_FEE * 2.0;
                equity += notional * pnl_pct - fee;
                n_trades += 1;

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

    if position != 0.0 {
        let last = closes_1d.get(n_1d.saturating_sub(1)).unwrap_or(0.0);
        let pnl_pct = if position > 0.0 {
            (last - entry_price) / entry_price
        } else {
            (entry_price - last) / entry_price
        };
        equity += equity * pnl_pct - equity * TAKER_FEE * 2.0;
        n_trades += 1;
    }

    Ok((
        (equity - CAPITAL) / CAPITAL * 100.0,
        n_trades,
        max_dd * 100.0,
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80));
    println!("  REALISTIC INTRA-BAR ANALYSIS");
    println!("  Using actual 30m sequence (conservative within each bar)");
    println!("{}", "━".repeat(80));
    println!("\n  Previous: Optimistic +642-30744pct, Worst-Case -23 to -49pct");
    println!("  Reality check: 51pct high-first, 49pct low-first\n");

    let loader = DataLoader::new(None, None);
    let strat = BollingerReversion::new();

    println!(
        "{:<10} {:>10} {:>10} {:>8} {:>10} {:>10} {:>6}",
        "Symbol", "Opt Sharpe", "Real Sharpe", "Diff", "Opt Ret", "Real Ret", "Trades"
    );
    println!("{}", "-".repeat(80));

    let mut results: Vec<(f64, f64, f64, f64)> = Vec::new();

    for symbol in SYMBOLS {
        print!("{}: loading... ", symbol);

        let raw_1d = loader.fetch_data(symbol, "1d", CANDLES_1D).await?;
        let df_1d = FeatureEngine::add_technicals(&raw_1d, None)?;
        let n_1d = df_1d.height();

        let df_30m = loader.fetch_data(symbol, "30m", CANDLES_1D * 48).await?;
        println!("1d={}, 30m={}", n_1d, df_30m.height());

        let signals_series = strat.predict(&df_1d)?;
        let signals: Vec<f64> = signals_series
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect();
        let trailing_sl = compute_atr_stop(&df_1d, ATR_MULT);

        let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
        let opt = bt.run(&df_1d, &signals_series, trailing_sl, 0.0)?;

        let (real_ret, n_trades, real_dd) =
            simulate_realistic(&df_1d, &df_30m, &signals, trailing_sl)?;

        let times_1d = df_1d.column("time")?.datetime()?;
        let years = if n_1d > 0 {
            let first = times_1d.get(0).unwrap_or(0);
            let last = times_1d.get(n_1d.saturating_sub(1)).unwrap_or(0);
            (last - first).abs() as f64 / (365.25 * 24.0 * 3600.0 * 1000.0)
        } else {
            1.0
        };
        let tpy = n_trades as f64 / years;
        let real_sharpe = if real_dd > 0.0 {
            (real_ret / 100.0 / (real_dd / 100.0)) * tpy.sqrt()
        } else {
            0.0
        };

        let diff = if opt.annualised_sharpe > 0.0 {
            (real_sharpe - opt.annualised_sharpe) / opt.annualised_sharpe * 100.0
        } else {
            0.0
        };

        println!(
            "{:<10} {:>10.1} {:>10.1} {:>7.1}pct {:>9.1}% {:>9.1}% {:>6}",
            symbol,
            opt.annualised_sharpe,
            real_sharpe,
            diff,
            opt.total_return_pct,
            real_ret,
            n_trades
        );

        results.push((
            opt.annualised_sharpe,
            real_sharpe,
            opt.total_return_pct,
            real_ret,
        ));
    }

    println!("\n{}", "━".repeat(80));
    if !results.is_empty() {
        let avg_opt_s: f64 = results.iter().map(|r| r.0).sum::<f64>() / results.len() as f64;
        let avg_real_s: f64 = results.iter().map(|r| r.1).sum::<f64>() / results.len() as f64;
        let avg_opt_r: f64 = results.iter().map(|r| r.2).sum::<f64>() / results.len() as f64;
        let avg_real_r: f64 = results.iter().map(|r| r.3).sum::<f64>() / results.len() as f64;
        let erosion = if avg_opt_s > 0.0 {
            (avg_real_s - avg_opt_s) / avg_opt_s * 100.0
        } else {
            0.0
        };

        println!("  Avg Optimistic Sharpe:  {:.1}", avg_opt_s);
        println!("  Avg Realistic Sharpe:   {:.1}", avg_real_s);
        println!("  Sharpe Erosion:         {:.1}pct", erosion);
        println!();
        println!("  Avg Optimistic Return:  {:.1}pct", avg_opt_r);
        println!("  Avg Realistic Return:   {:.1}pct", avg_real_r);

        if avg_real_r < 0.0 {
            println!("\n  CRITICAL: Realistic execution turns profit into loss");
        } else if erosion < -50.0 {
            println!("\n  SEVERE: >50pct of edge from optimistic execution");
        } else if erosion < -20.0 {
            println!("\n  WARNING: 20-50pct of edge from optimistic execution");
        } else {
            println!("\n  OK: Edge survives realistic execution");
        }
    }
    println!("{}", "━".repeat(80));

    Ok(())
}

//! T97: FRESHNESS_COOLDOWN=93 held-out validation (pre-2021 data)
//!
//! Validates that FC=93 improves over FC=0 on held-out (pre-2021) data.
//! FC=93 was promoted to production bot.rs in T96 via 101-value in-sample sweep.
//! Same pattern previously produced EP=24 (false positive), HAP=0.09 (false positive).
//!
//! Run: cargo run --example t97_fc93_held_out --profile sweep

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES: usize = 3;
const FC_VALUES: [usize; 2] = [0, 93];

fn main() -> Result<()> {
    let symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];
    let window_count = 6;
    let held_out_cutoff = 1000; // roughly 2021-01-01

    let mut fc_results: HashMap<usize, Vec<WindowResult>> = HashMap::new();
    for &fc in &FC_VALUES {
        fc_results.insert(fc, vec![]);
    }

    for &sym in &symbols {
        let data = load_symbol(sym)?;
        if data.close.len() < held_out_cutoff + TEST_BARS + TRAIN_BARS {
            println!("  [{}] insufficient data ({} bars)", sym, data.close.len());
            continue;
        }

        // Walk forward: train on 252 bars, test on next 252
        for w in 0..window_count {
            let train_start = w * TEST_BARS;
            let test_start = train_start + TRAIN_BARS;
            let test_end = test_start + TEST_BARS;

            if test_end > data.close.len() || test_start >= held_out_cutoff {
                continue;
            }

            // Cap test at held_out_cutoff
            let actual_test_end = test_end.min(held_out_cutoff);
            let actual_test_len = actual_test_end - test_start;
            if actual_test_len < MIN_TRADES * 5 {
                continue;
            }

            for &fc in &FC_VALUES {
                let trades = run_backtest(
                    &data.close,
                    &data.high,
                    &data.low,
                    test_start,
                    actual_test_end,
                    fc,
                );

                let (ret, sharpe, dd) = compute_metrics(&trades);

                fc_results.get_mut(&fc).unwrap().push(WindowResult {
                    symbol: sym.to_string(),
                    window: w,
                    trades: trades.len(),
                    return_pct: ret,
                    sharpe,
                    max_dd: dd,
                });
            }
        }
    }

    // Report
    let mut csv_lines = vec!["fc,symbol,window,trades,return_pct,sharpe,max_dd".to_string()];
    println!("\n=== FC Held-Out Results (pre-2021) ===\n");
    for &fc in &FC_VALUES {
        let results = fc_results.get(&fc).unwrap();
        let n = results.len();
        if n == 0 {
            println!("FC={}: no windows", fc);
            continue;
        }
        let pass = results.iter().filter(|r| r.sharpe > 0.0 && r.return_pct > 0.0).count();
        let avg_ret: f64 = results.iter().map(|r| r.return_pct).sum::<f64>() / n as f64;
        let avg_sharpe: f64 = results.iter().map(|r| r.sharpe).sum::<f64>() / n as f64;
        let avg_dd: f64 = results.iter().map(|r| r.max_dd).sum::<f64>() / n as f64;
        let total_trades: usize = results.iter().map(|r| r.trades).sum();

        println!("FC={}: {}/{} windows pass | avg Sharpe {:.3f} | avg return {:+.1f}% | avg DD {:.1f}% | {} trades",
            fc, pass, n, avg_sharpe, avg_ret, avg_dd, total_trades);

        for r in results {
            csv_lines.push(format!("{},{},{},{},{},{:.4f},{:.2f}", fc, r.symbol, r.window, r.trades, r.return_pct, r.sharpe, r.max_dd));
        }
    }

    let start = std::time::Instant::now();
    File::create("snapshots/t97_fc93_held_out.csv")?
        .write_all(csv_lines.join("\n").as_bytes())?;
    println!("\nDone in {:.1}s. CSV: snapshots/t97_fc93_held_out.csv", start.elapsed().as_secs_f32());

    Ok(())
}

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
}

fn load_symbol(symbol: &str) -> Result<SymData> {
    let path = format!("data/cache/crypto/bars/{}.parquet", symbol);
    let df = polars::prelude::ParquetReader::new(std::fs::File::open(&path)?).finish()?;
    let close: Vec<f64> = df.column("close")?.f64()?.to_vec();
    let high: Vec<f64> = df.column("high")?.f64()?.to_vec();
    let low: Vec<f64> = df.column("low")?.f64()?.to_vec();
    Ok(SymData { close, high, low })
}

struct Trade { pct: f64 }
struct WindowResult {
    symbol: String,
    window: usize,
    trades: usize,
    return_pct: f64,
    sharpe: f64,
    max_dd: f64,
}

fn run_backtest(close: &[f64], high: &[f64], low: &[f64], start: usize, end: usize, fc: usize) -> Vec<Trade> {
    let mut trades = vec![];
    let mut last_exit_bar = HashMap::<String, usize>::new();
    let mut position: Option<(f64, usize, f64, VecDeque<f64>)> = None; // (entry, bar, highest_high, atr_buf)

    let ep = 21;
    let atr_period = 24;
    let atr_mult = 2.0;
    let hold_max = 15;

    let mut equity_curve = vec![1.0_f64];
    let mut equity = 1.0;

    for i in start..end {
        // Check exit first
        if let Some((entry, entry_bar, mut hh, ref mut atr_buf)) = position {
            // Compute ATR
            let atr_start = i.saturating_sub(atr_period);
            let atr = if i - atr_start >= atr_period {
                high[atr_start..i].iter().zip(low[atr_start..i].iter())
                    .map(|(h, l)| h - l).sum::<f64>() / atr_period as f64
            } else {
                continue;
            };
            let stop = hh - atr_mult * atr;
            hh = hh.max(high[i]);

            // Exit check
            let bars_held = i - entry_bar;
            let exited = low[i] <= stop || bars_held >= hold_max;

            if exited {
                let pct = (close[i] - entry) / entry;
                trades.push(Trade { pct });
                equity *= 1.0 + pct;
                equity_curve.push(equity);
                position = None;
            } else {
                equity_curve.push(equity);
            }
        }

        // Check entry (skip if position open or fresh exit)
        if position.is_none() {
            // Freshness check
            if let Some(&last_exit) = last_exit_bar.get("sym") {
                if i - last_exit < fc { continue; }
            }

            // Turtle entry: close > max(close) over EP bars
            if i >= ep {
                let lookback_start = i.saturating_sub(ep);
                let max_close = close[lookback_start..i].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                if close[i] > max_close {
                    position = Some((close[i], i, high[i], VecDeque::new()));
                }
            }
        }
    }

    if let Some((_, _, _, _)) = position {
        // Open position at end — close at last close
        // No trade if not exited
    }

    trades
}

fn compute_metrics(trades: &[Trade]) -> (f64, f64, f64) {
    if trades.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let total: f64 = trades.iter().map(|t| t.pct).sum();
    let n = trades.len() as f64;
    let avg = total / n;

    // Annualized Sharpe (252 days, daily returns = trade return / bars_held approx)
    // Simpler: use trade-level returns
    let variance: f64 = trades.iter().map(|t| {
        let r = t.pct - avg;
        r * r
    }).sum::<f64>() / n.max(1.0);
    let std_dev = variance.sqrt();
    let sharpe = if std_dev > 0.0 { (avg / std_dev) * (252_f64.sqrt()) } else { 0.0 };

    // Max drawdown from equity curve
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    for t in trades {
        equity *= 1.0 + t.pct;
        peak = peak.max(equity);
        let dd = (equity - peak) / peak;
        max_dd = max_dd.min(dd);
    }
    max_dd = max_dd.abs() * 100.0;

    (total * 100.0, sharpe, max_dd)
}
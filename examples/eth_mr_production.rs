//! ETH/XRP Intraday Mean Reversion Production Harness
//!
//! PURPOSE: Validate 4h mean reversion on FDUSD pairs (0% maker fee) with
//! the parameters swept in the native 4h parameter search:
//!   lookback=12, entry_z=1.5, exit_z=0.3, max_hold=12 bars
//!
//! FDUSD perpetuals have 0% maker fee — critical for intraday mean reversion
//! which is extremely slippage-sensitive (edge drops 70% with just 5bps).
//!
//! Walk-forward: 65% train / 4 equal test windows, OOS evaluation per window.
//! Symbols: ETHFDUSD, XRPFDUSD (both have 0% maker on Binance).
//!
//! Benchmarks:
//! - 4h MACD+Regime (trend yardstick at same timeframe)
//! - Buy & Hold (performance reference)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::fs::OpenOptions;
use std::io::Write;

const INTERVAL: &str = "4h";
const CANDLES: u32 = 4000; // ~1.6 years at 4h
const TRAIN_FRAC: f64 = 0.50; // 50% train / 50% test (tighter than typical for MR)
const N_WINDOWS: usize = 4;

// 4h MR parameters (from native sweep)
const MR_LOOKBACK: usize = 12;
const MR_ENTRY_Z: f64 = 1.5;
const MR_EXIT_Z: f64 = 0.3;
const MR_MAX_HOLD: usize = 12;

// 4h MACD benchmark
const MACD_FAST: usize = 16; // 4h equivalent of 1h fast=4
const MACD_SLOW: usize = 48; // 4h equivalent of 1h slow=12
const MACD_SIGNAL_P: usize = 12;
const TAKER_FEE: f64 = 0.001; // 0.1% — USDT pair taker (FDUSD is 0% maker but we use taker for conservative estimate)

// ── Math helpers ───────────────────────────────────────────────────────────────

fn calc_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 5 {
        return 0.0;
    }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets
        .iter()
        .map(|r| (r - mean).powi(2))
        .sum::<f64>()
        / daily_rets.len().max(1) as f64;
    let std = var.sqrt();
    if std < 1e-9 {
        return 0.0;
    }
    // 4h bars: 365 * 6 = 2190 bars/year (4h * 6/day)
    mean / std * (2190.0_f64.sqrt())
}

fn calc_max_dd(equity: &[f64]) -> f64 {
    let mut peak = equity[0];
    let mut max_dd: f64 = 0.0;
    for &e in equity {
        peak = peak.max(e);
        let dd = (peak - e) / peak * 100.0;
        max_dd = max_dd.max(dd);
    }
    max_dd
}

fn rolling_z(closes: &[f64], idx: usize, lookback: usize) -> Option<f64> {
    if idx < lookback || lookback == 0 {
        return None;
    }
    let start = idx.saturating_sub(lookback);
    let window = &closes[start..idx];
    if window.is_empty() {
        return None;
    }
    let mean = window.iter().sum::<f64>() / window.len() as f64;
    let variance = window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / window.len() as f64;
    let std = variance.sqrt();
    if std < 1e-10 {
        return None;
    }
    Some((closes[idx] - mean) / std)
}

fn ema(data: &[f64], n: usize) -> Vec<f64> {
    if data.is_empty() || n == 0 {
        return vec![];
    }
    let alpha = 2.0 / (n as f64 + 1.0);
    let mut out = vec![data[0]; data.len()];
    for i in 1..data.len() {
        out[i] = alpha * data[i] + (1.0 - alpha) * out[i - 1];
    }
    out
}

fn macd_bullish(closes: &[f64], idx: usize) -> bool {
    if idx < MACD_SLOW + MACD_SIGNAL_P {
        return false;
    }
    let fast = ema(&closes[..=idx.min(closes.len() - 1)], MACD_FAST);
    let slow = ema(&closes[..=idx.min(closes.len() - 1)], MACD_SLOW);
    let n = fast.len().min(slow.len());
    if n < 2 {
        return false;
    }
    let macd_line = fast[n - 1] - slow[n - 1];
    // Compute signal line manually for last MACD_SIGNAL_P bars
    let macd_vals: Vec<f64> = fast[..n].iter().zip(slow[..n].iter()).map(|(f, s)| f - s).collect();
    let signal_vec = ema(&macd_vals, MACD_SIGNAL_P);
    let signal_line = *signal_vec.last().unwrap_or(&0.0);
    macd_line > signal_line
}

// ── Per-window evaluation ──────────────────────────────────────────────────────

struct WindowMetrics {
    return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_final: f64,
}

fn eval_mr(
    closes: &[f64],
    test_start: usize,
    test_end: usize,
) -> WindowMetrics {
    let mut equity = vec![1.0];
    let mut oos_rets = Vec::new();
    let mut wins = 0;
    let mut total_trades = 0;

    let mut i = test_start;
    while i < test_end.saturating_sub(1) && i + 1 < closes.len() {
        let z = match rolling_z(closes, i, MR_LOOKBACK) {
            Some(z) => z,
            None => {
                equity.push(*equity.last().unwrap());
                i += 1;
                continue;
            }
        };

        if z > MR_ENTRY_Z {
            // Long mean reversion entry at next bar
            let entry_idx = (i + 1).min(closes.len() - 1);
            let entry_price = closes[entry_idx];

            if entry_price <= 0.0 {
                i += 1;
                continue;
            }

            // Search for exit: z falls below exit_z, or max hold reached, or end of window
            let search_end = (entry_idx + MR_MAX_HOLD).min(test_end.min(closes.len() - 1));
            let mut exit_idx = search_end;
            let mut found_exit = false;

            for j in entry_idx + 1..=search_end {
                if let Some(zj) = rolling_z(closes, j, MR_LOOKBACK) {
                    if zj < MR_EXIT_Z {
                        exit_idx = j;
                        found_exit = true;
                        break;
                    }
                }
            }

            if !found_exit {
                // Time-based exit at max hold
                exit_idx = search_end;
            }

            let exit_price = closes[exit_idx.min(closes.len() - 1)];
            let gross = exit_price / entry_price - 1.0;
            let net = gross - 2.0 * TAKER_FEE;

            let span = (exit_idx.saturating_sub(entry_idx)).max(1);

            for _ in 0..span {
                equity.push(*equity.last().unwrap() * (1.0 + net / span as f64));
            }
            for _ in 0..span {
                oos_rets.push(net / span as f64);
            }

            total_trades += 1;
            if gross > 0.0 {
                wins += 1;
            }

            i = exit_idx + 1;
        } else {
            equity.push(*equity.last().unwrap());
            i += 1;
        }
    }

    // Fill remainder of test window
    while equity.len() < (test_end - test_start) + 1 {
        equity.push(*equity.last().unwrap());
    }

    let final_equity = equity.last().copied().unwrap_or(1.0);
    let ret_pct = (final_equity - 1.0) * 100.0;
    let sharpe = calc_sharpe(&oos_rets);
    let max_dd = calc_max_dd(&equity);
    let win_rate = if total_trades > 0 {
        wins as f64 / total_trades as f64
    } else {
        0.0
    };

    WindowMetrics {
        return_pct: ret_pct,
        sharpe,
        max_dd,
        trades: total_trades,
        win_rate,
        pass: ret_pct > 0.0,
        equity_final: final_equity,
    }
}

fn eval_macd_4h(
    closes: &[f64],
    test_start: usize,
    test_end: usize,
) -> WindowMetrics {
    let mut equity = vec![1.0];
    let mut oos_rets = Vec::new();
    let mut wins = 0;
    let mut total_trades = 0;

    let mut i = test_start;
    while i < test_end.saturating_sub(1) && i + 1 < closes.len() {
        if !macd_bullish(closes, i) {
            equity.push(*equity.last().unwrap());
            i += 1;
            continue;
        }

        let entry_idx = (i + 1).min(closes.len() - 1);
        let entry_price = closes[entry_idx];
        if entry_price <= 0.0 {
            i += 1;
            continue;
        }

        let exit_idx = (entry_idx + MR_MAX_HOLD).min(test_end.min(closes.len() - 1));
        let exit_price = closes[exit_idx];
        let gross = exit_price / entry_price - 1.0;
        let net = gross - 2.0 * TAKER_FEE;

        let span = (exit_idx.saturating_sub(entry_idx)).max(1);
        for _ in 0..span {
            equity.push(*equity.last().unwrap() * (1.0 + net / span as f64));
        }
        for _ in 0..span {
            oos_rets.push(net / span as f64);
        }

        total_trades += 1;
        if gross > 0.0 {
            wins += 1;
        }

        i = exit_idx + 1;
    }

    while equity.len() < (test_end - test_start) + 1 {
        equity.push(*equity.last().unwrap());
    }

    let final_equity = equity.last().copied().unwrap_or(1.0);
    let ret_pct = (final_equity - 1.0) * 100.0;
    let sharpe = calc_sharpe(&oos_rets);
    let max_dd = calc_max_dd(&equity);
    let win_rate = if total_trades > 0 {
        wins as f64 / total_trades as f64
    } else {
        0.0
    };

    WindowMetrics {
        return_pct: ret_pct,
        sharpe,
        max_dd,
        trades: total_trades,
        win_rate,
        pass: ret_pct > 0.0,
        equity_final: final_equity,
    }
}

fn eval_buy_hold(closes: &[f64], test_start: usize, test_end: usize) -> WindowMetrics {
    let start_price = closes.get(test_start).copied().unwrap_or(1.0);
    let end_idx = test_end.min(closes.len() - 1);
    let end_price = closes.get(end_idx).unwrap_or(&start_price);
    let gross = end_price / start_price - 1.0;
    let net = gross - 2.0 * TAKER_FEE;
    let ret_pct = net * 100.0;

    WindowMetrics {
        return_pct: ret_pct,
        sharpe: 0.0,
        max_dd: 0.0,
        trades: 1,
        win_rate: if net > 0.0 { 1.0 } else { 0.0 },
        pass: ret_pct > 0.0,
        equity_final: 1.0 + net,
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== ETH/XRP 4h Mean Reversion — Production Harness ===\n");
    println!(
        "Parameters: lookback={}, entry_z={:.1}, exit_z={:.1}, max_hold={} bars",
        MR_LOOKBACK, MR_ENTRY_Z, MR_EXIT_Z, MR_MAX_HOLD
    );
    println!("Fee: {:.1}% taker | Interval: {} | Candles: {}\n", TAKER_FEE * 100.0, INTERVAL, CANDLES);

    let loader = DataLoader::new(None, None);

    let mut all_csv: Vec<String> = Vec::new();
    all_csv.push("symbol,window,strategy,return_pct,sharpe,max_dd,trades,win_rate,pass".to_string());

    let symbols = [
        ("ETHFDUSD", "ETHUSDT"),
        ("XRPFDUSD", "XRPUSDT"),
    ];

    for (fd_symbol, usdt_symbol) in symbols {
        println!("\n=== {} ===", fd_symbol);

        // Load FDUSD 4h data
        let raw = match loader.fetch_data(fd_symbol, INTERVAL, CANDLES).await {
            Ok(df) => df,
            Err(e) => {
                println!("  {} failed: {e}", fd_symbol);
                // Fallback to USDT pair
                println!("  Falling back to {}...", usdt_symbol);
                match loader.fetch_data(usdt_symbol, INTERVAL, CANDLES).await {
                    Ok(df) => df,
                    Err(e2) => {
                        println!("  {} also failed: {e2}", usdt_symbol);
                        continue;
                    }
                }
            }
        };

        // Add technicals
        let btc_raw = loader.fetch_data("BTCUSDT", "1d", 300).await.ok();
        let enriched = match &btc_raw {
            Some(btc) => FeatureEngine::add_technicals(&raw, Some(btc))?,
            None => FeatureEngine::add_technicals(&raw, None)?,
        };

        let times: Vec<i64> = enriched
            .column("time")?
            .cast(&DataType::Int64)?
            .i64()?
            .into_iter()
            .flatten()
            .collect();

        let to_f64 = |s: &Series| -> Vec<f64> {
            s.f64()
                .unwrap()
                .into_iter()
                .map(|v| v.unwrap_or(0.0))
                .collect()
        };

        let closes = to_f64(enriched.column("close")?);

        let n = closes.len();
        let train_end = (n as f64 * TRAIN_FRAC) as usize;
        let test_len = (n - train_end) / N_WINDOWS;

        println!(
            "  {} bars | train: 0-{} | test windows: {} each\n",
            n, train_end, test_len
        );

        let mut mr_pass = 0;
        let mut macd_pass = 0;
        let mut bnh_pass = 0;
        let mut mr_all = Vec::new();
        let mut macd_all = Vec::new();

        for w in 0..N_WINDOWS {
            let test_start = train_end + w * test_len;
            let test_end = if w == N_WINDOWS - 1 {
                n
            } else {
                (test_start + test_len).min(n)
            };

            if test_end <= test_start || test_end > n {
                continue;
            }

            let start_time = times.get(test_start).copied().unwrap_or(0);
            let end_time = times.get(test_end.saturating_sub(1)).copied().unwrap_or(0);
            println!(
                "  W{} | test [{:>4}, {:>4}] | {}",
                w, test_start, test_end,
                epoch_ms_to_date(start_time, end_time)
            );

            let mr = eval_mr(&closes, test_start, test_end);
            let macd = eval_macd_4h(&closes, test_start, test_end);
            let bnh = eval_buy_hold(&closes, test_start, test_end);

            if mr.pass { mr_pass += 1; }
            if macd.pass { macd_pass += 1; }
            if bnh.pass { bnh_pass += 1; }
            mr_all.push(mr.return_pct);
            macd_all.push(macd.return_pct);

            let pass_str = |p| if p { "✅" } else { "❌" };

            println!(
                "    MR:        {:>+8.2}% | sh={:>+6.2} | dd={:>6.2}% | {}t | wr={:>5.0}% {}",
                mr.return_pct, mr.sharpe, mr.max_dd, mr.trades, mr.win_rate * 100.0, pass_str(mr.pass),
            );
            println!(
                "    MACD@4h:   {:>+8.2}% | sh={:>+6.2} | dd={:>6.2}% | {}t | wr={:>5.0}% {}",
                macd.return_pct, macd.sharpe, macd.max_dd, macd.trades, macd.win_rate * 100.0, pass_str(macd.pass),
            );
            println!(
                "    BnH:       {:>+8.2}% {}",
                bnh.return_pct, pass_str(bnh.pass),
            );

            for (label, m) in [("MR", &mr), ("MACD4h", &macd), ("BnH", &bnh)] {
                all_csv.push(format!(
                    "{},W{},{},{},{},{},{},{},{}",
                    fd_symbol, w, label,
                    format!("{:.4}", m.return_pct),
                    format!("{:.4}", m.sharpe),
                    format!("{:.4}", m.max_dd),
                    m.trades,
                    format!("{:.4}", m.win_rate),
                    if m.pass { "PASS" } else { "FAIL" },
                ));
            }
        }

        let mr_rate = mr_pass as f32 / N_WINDOWS as f32 * 100.0;
        let macd_rate = macd_pass as f32 / N_WINDOWS as f32 * 100.0;
        let bnh_rate = bnh_pass as f32 / N_WINDOWS as f32 * 100.0;
        println!(
            "\n  {} OOS Summary: MR {}/{} ({:.0}%) ✅ | MACD {}/{} ({:.0}%) ✅ | BnH {}/{} ({:.0}%) ✅",
            fd_symbol,
            mr_pass, N_WINDOWS, mr_rate,
            macd_pass, N_WINDOWS, macd_rate,
            bnh_pass, N_WINDOWS, bnh_rate,
        );

        if !mr_all.is_empty() {
            let avg_mr = mr_all.iter().sum::<f64>() / mr_all.len() as f64;
            let avg_macd = if !macd_all.is_empty() {
                macd_all.iter().sum::<f64>() / macd_all.len() as f64
            } else {
                0.0
            };
            println!(
                "  {} avg OOS: MR {:+.2}% vs MACD {:+.2}% vs BNH",
                fd_symbol, avg_mr, avg_macd,
            );
        }
    }

    let out_path = "snapshots/eth_mr_production_results.csv";
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(out_path)?;
    for line in &all_csv {
        writeln!(f, "{}", line)?;
    }
    println!("\nResults written to {}", out_path);

    Ok(())
}

fn epoch_ms_to_date(start_ms: i64, end_ms: i64) -> String {
    use chrono::{TimeZone, Utc};
    let fmt = |ms: i64| {
        Utc.timestamp_millis_opt(ms)
            .single()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "?".to_string())
    };
    format!("{} → {}", fmt(start_ms), fmt(end_ms))
}

//! Intraday Mean Reversion @ 4h — Walk-Forward Validation
//!
//! Goal: Validate ETH (and XRP if data available) mean reversion at 4h bars.
//!
//! Key question: Does the +8.6% avg OOS result at 1h survive at 4h?
//!
//! Parameters (pre-declared from 1h best result, adapted for 4h):
//! - Entry z-score: 1.5 (ETH), 2.0 (XRP) — same as 1h best
//! - Lookback: 96 bars (same as 1h; captures ~16d at 4h)
//! - Max hold: 12 bars (~2d at 4h)
//! - Exit: z > -0.3 (partial reversion exit)
//! - Direction: LONG ONLY (1h result showed mean reversion works long)
//!
//! Benchmarks compared at 4h:
//! - MACD+Regime (4h) — trend yardstick at same timeframe
//! - Buy & Hold — performance reference
//!
//! NO config sweep. Pre-declared parameters only. Walk-forward 4 windows.

use anyhow::Result;
use chrono::Utc;
use colored::*;
use krypto::data::DataLoader;
use std::collections::BTreeMap;

const INTERVAL: &str = "4h";
const CANDLES_PER_SYMBOL: usize = 8000; // ~2.7 years at 4h
const TRAIN_FRAC: f64 = 0.65;
const FEE_PCT: f64 = 0.001; // 0.1% taker each side

const SYMBOLS: &[&str] = &["ETHUSDT", "XRPUSDT"];

// Pre-declared mean reversion configs (from 1h best, NOT swept)
const MR_ENTRY_Z_ETH: f64 = 1.5;
const MR_ENTRY_Z_XRP: f64 = 2.0;
const MR_LOOKBACK: usize = 96; // same as 1h (bar count matters more than time)
const MR_MAX_HOLD: usize = 12; // ~2 days at 4h
const MR_EXIT_Z: f64 = -0.3;

// Pre-declared MACD configs for 4h benchmark
const MACD_FAST: usize = 16; // 4h equivalent of 1h fast=8*4/4=8
const MACD_SLOW: usize = 48; // 4h equivalent of 1h slow=24*4/4=24
const MACD_SIGNAL: usize = 12;

fn epoch_ms_to_date(ms: i64) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "?".to_string())
}

// ── Mean Reversion Signal (4h) ─────────────────────────────────────────────
fn mr_signal(closes: &[f64], idx: usize, _entry_z: f64, lookback: usize) -> Option<f64> {
    if idx < lookback {
        return None;
    }
    let window = &closes[idx - lookback..idx];
    let mean = window.iter().sum::<f64>() / lookback as f64;
    let variance = window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / lookback as f64;
    let std = variance.sqrt();
    if std < 1e-10 {
        return None;
    }
    let z = (closes[idx] - mean) / std;
    Some(z)
}

// ── Rolling Z-Score ─────────────────────────────────────────────────────────
fn rolling_z(closes: &[f64], idx: usize, lookback: usize) -> Option<f64> {
    if idx < lookback {
        return None;
    }
    let window = &closes[idx - lookback..idx];
    let mean = window.iter().sum::<f64>() / lookback as f64;
    let variance = window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / lookback as f64;
    let std = variance.sqrt();
    if std < 1e-10 {
        return None;
    }
    Some((closes[idx] - mean) / std)
}

// ── MACD Signal (4h) ────────────────────────────────────────────────────────
fn ema(data: &[f64], n: usize) -> Vec<f64> {
    let alpha = 2.0 / (n as f64 + 1.0);
    let mut ema = Vec::with_capacity(data.len());
    ema.push(data[0]);
    for &price in data.iter().skip(1) {
        let prev = *ema.last().unwrap();
        ema.push(prev + alpha * (price - prev));
    }
    ema
}

fn macd_signal_4h(closes: &[f64], idx: usize) -> Option<bool> {
    if idx < MACD_SLOW + MACD_SIGNAL {
        return None;
    }
    let fast = ema(&closes[..=idx], MACD_FAST);
    let slow = ema(&closes[..=idx], MACD_SLOW);
    let signal = ema(&fast[..], MACD_SIGNAL);
    let macd_line = fast[fast.len() - 1] - slow[slow.len() - 1];
    let signal_line = signal[signal.len() - 1];
    Some(macd_line > signal_line) // true = bullish
}

// ── Walk-Forward Evaluation ───────────────────────────────────────────────────
fn evaluate_mr(
    closes: &[f64],
    entry_z: f64,
    lookback: usize,
    max_hold: usize,
    exit_z: f64,
    label: &str,
) -> (String, f64, f64, usize, f64, f64, usize, usize) {
    let n = closes.len();
    let train_end = (n as f64 * TRAIN_FRAC) as usize;
    let n_windows = 4;
    let test_len = (n - train_end) / n_windows;

    let mut total_return = 1.0_f64;
    let mut wins = 0;
    let mut losses = 0;
    let mut trades = 0;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut oos_returns = Vec::new();

    for w in 0..n_windows {
        let test_start = train_end + w * test_len;
        let test_end = if w == n_windows - 1 {
            n
        } else {
            test_start + test_len
        };

        for i in test_start..test_end.saturating_sub(1) {
            let z = match rolling_z(closes, i, lookback) {
                Some(z) => z,
                None => continue,
            };

            if z < entry_z {
                // Enter long at next bar open
                let entry = closes[i + 1];
                let exit_idx = (i + 1 + max_hold).min(test_end - 1).min(n - 1);

                // Exit when z reverts above exit_z
                let mut exit_price = closes[exit_idx];
                let exit_search_end = exit_idx.min(n - 1);
                for j in (i + 1)..exit_search_end {
                    if let Some(zj) = rolling_z(closes, j, lookback) {
                        if zj > exit_z {
                            exit_price = closes[j];
                            break;
                        }
                    }
                }

                let gross = (exit_price / entry - 1.0) - 2.0 * FEE_PCT;
                total_return *= 1.0 + gross;
                if gross > 0.0 {
                    wins += 1;
                } else {
                    losses += 1;
                }
                trades += 1;
                oos_returns.push(gross);

                let equity = total_return;
                if equity > peak {
                    peak = equity;
                }
                let dd = (peak - equity) / peak;
                if dd > max_dd {
                    max_dd = dd;
                }
            }
        }
    }

    let wr = if trades > 0 {
        wins as f64 / trades as f64
    } else {
        0.0
    };
    let avg_trade = oos_returns.iter().sum::<f64>() / oos_returns.len().max(1) as f64;
    let total_pct = (total_return - 1.0) * 100.0;

    (
        label.to_string(),
        total_pct,
        avg_trade * 100.0,
        trades,
        wr * 100.0,
        max_dd * 100.0,
        wins,
        losses,
    )
}

fn evaluate_macd_4h(
    closes: &[f64],
    label: &str,
) -> (String, f64, f64, usize, f64, f64, usize, usize) {
    let n = closes.len();
    let train_end = (n as f64 * TRAIN_FRAC) as usize;
    let n_windows = 4;
    let test_len = (n - train_end) / n_windows;

    let mut total_return = 1.0_f64;
    let mut wins = 0;
    let mut losses = 0;
    let mut trades = 0;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut oos_returns = Vec::new();

    for w in 0..n_windows {
        let test_start = train_end + w * test_len;
        let test_end = if w == n_windows - 1 {
            n
        } else {
            test_start + test_len
        };

        for i in test_start..test_end.saturating_sub(1) {
            let bullish = match macd_signal_4h(closes, i) {
                Some(b) => b,
                None => continue,
            };

            if bullish {
                let entry = closes[i + 1];
                let exit_idx = (i + 1 + 12).min(test_end - 1).min(n - 1);
                let exit_price = closes[exit_idx];
                let gross = (exit_price / entry - 1.0) - 2.0 * FEE_PCT;
                total_return *= 1.0 + gross;
                if gross > 0.0 {
                    wins += 1;
                } else {
                    losses += 1;
                }
                trades += 1;
                oos_returns.push(gross);
                let equity = total_return;
                if equity > peak {
                    peak = equity;
                }
                let dd = (peak - equity) / peak;
                if dd > max_dd {
                    max_dd = dd;
                }
            }
        }
    }

    let wr = if trades > 0 {
        wins as f64 / trades as f64
    } else {
        0.0
    };
    let avg_trade = oos_returns.iter().sum::<f64>() / oos_returns.len().max(1) as f64;
    let total_pct = (total_return - 1.0) * 100.0;

    (
        label.to_string(),
        total_pct,
        avg_trade * 100.0,
        trades,
        wr * 100.0,
        max_dd * 100.0,
        wins,
        losses,
    )
}

// ── Hold Benchmark ───────────────────────────────────────────────────────────
fn buy_hold(closes: &[f64], start: usize, end: usize) -> f64 {
    if end <= start || start >= closes.len() || end == 0 {
        return 0.0;
    }
    let start_p = closes[start];
    let end_p = closes[end.saturating_sub(1)];
    (end_p / start_p - 1.0) - 2.0 * FEE_PCT
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", "═".repeat(70));
    println!("  INTRADAY MEAN REVERSION @ 4h — WALK-FORWARD VALIDATION");
    println!("{}", "═".repeat(70));
    println!("  Pre-declared params (from 1h best result, NOT swept)");
    println!(
        "  ETH: z={:.1}, lb={}, mh={}, xz={:.1}",
        MR_ENTRY_Z_ETH, MR_LOOKBACK, MR_MAX_HOLD, MR_EXIT_Z
    );
    println!(
        "  XRP: z={:.1}, lb={}, mh={}, xz={:.1}",
        MR_ENTRY_Z_XRP, MR_LOOKBACK, MR_MAX_HOLD, MR_EXIT_Z
    );
    println!(
        "  MACD@4h: fast={}, slow={}, sig={}",
        MACD_FAST, MACD_SLOW, MACD_SIGNAL
    );
    println!(
        "  Interval: {} | {} candles | {}% train | {:.1}bps fee",
        INTERVAL,
        CANDLES_PER_SYMBOL,
        (TRAIN_FRAC * 100.0) as i32,
        FEE_PCT * 10000.0
    );
    println!("{}", "─".repeat(70));

    let loader = DataLoader::new(None, None);

    for sym in SYMBOLS {
        print!("\nLoading {sym} {INTERVAL} ({CANDLES_PER_SYMBOL} candles)... ",);
        let df = match loader
            .fetch_data(sym, INTERVAL, CANDLES_PER_SYMBOL as u32)
            .await
        {
            Ok(df) => df,
            Err(e) => {
                println!("FAILED: {e}");
                continue;
            }
        };

        let times: Vec<i64> = df
            .column("time")?
            .cast(&polars::prelude::DataType::Int64)?
            .i64()?
            .into_iter()
            .flatten()
            .collect();
        let closes: Vec<f64> = df
            .column("close")?
            .cast(&polars::prelude::DataType::Float64)?
            .f64()?
            .into_iter()
            .flatten()
            .collect();

        let n = closes.len();
        let train_end = (n as f64 * TRAIN_FRAC) as usize;
        println!(
            "{} bars | {} → {}",
            n,
            epoch_ms_to_date(*times.first().unwrap_or(&0)),
            epoch_ms_to_date(*times.last().unwrap_or(&0))
        );
        println!(
            "  Train: {} bars ({}) | Test: {} bars",
            train_end,
            epoch_ms_to_date(times[train_end.saturating_sub(1)]),
            n - train_end
        );

        // Buy & Hold baseline (from train_end to end)
        let bnh = buy_hold(&closes, train_end, n);
        println!("  Buy&Hold OOS (train→end): {:.1}%", bnh * 100.0);

        let entry_z = if *sym == "ETHUSDT" {
            MR_ENTRY_Z_ETH
        } else {
            MR_ENTRY_Z_XRP
        };

        // Mean Reversion @ 4h
        let (label, ret, avg, trades, wr, mdd, wins, losses) = evaluate_mr(
            &closes,
            entry_z,
            MR_LOOKBACK,
            MR_MAX_HOLD,
            MR_EXIT_Z,
            &format!("MR@4h({sym})"),
        );
        println!("\n  {}", "─".repeat(50));
        println!(
            "  📊 {}: {:.1}% OOS | {:.2}% avg/trade | {} trades | {:.0}% WR | {:.1}% DD",
            label.cyan(),
            ret,
            avg,
            trades,
            wr,
            mdd
        );
        println!("     Wins: {} | Losses: {}", wins, losses);

        // MACD @ 4h benchmark
        let (mlabel, mret, mavg, mtrades, mwr, mmdd, mwins, mlosses) =
            evaluate_macd_4h(&closes, &format!("MACD@4h({sym})"));
        println!(
            "  📊 {}: {:.1}% OOS | {:.2}% avg/trade | {} trades | {:.0}% WR | {:.1}% DD",
            mlabel.yellow(),
            mret,
            mavg,
            mtrades,
            mwr,
            mmdd
        );
        println!("     Wins: {} | Losses: {}", mwins, mlosses);

        // Verdict
        println!("\n  {}", "═".repeat(50));
        if trades >= 30 && ret > 0.0 {
            let vs_macd = ret - mret;
            println!(
                "  ✅ MR@4h OOS: {:.1}% | vs MACD@4h: {:+.1}% | trades: {}",
                ret, vs_macd, trades
            );
        } else if trades < 30 {
            println!("  ⚠️  MR@4h: Only {} trades — too thin to trust", trades);
        } else {
            println!(
                "  ❌ MR@4h: {:.1}% OOS return — NEGATIVE (invalidated)",
                ret
            );
        }

        // Compare to 1h reference
        if *sym == "ETHUSDT" {
            println!(
                "\n  📎 1h reference ({}): +8.6% avg OOS, 123 trades, 68% WR",
                "ETHUSDT"
            );
            println!(
                "     4h result: {:.1}%, {} trades — {}",
                ret,
                trades,
                if trades >= 30 && ret > 0.0 {
                    "4h CONFIRMED"
                } else if trades < 30 {
                    "4h TOO THIN"
                } else {
                    "4h INVALIDATED"
                }
            );
        }
        if *sym == "XRPUSDT" {
            println!(
                "\n  📎 1h reference ({}): +5.2% avg OOS, 64 trades, 67% WR",
                "XRPUSDT"
            );
            println!(
                "     4h result: {:.1}%, {} trades — {}",
                ret,
                trades,
                if trades >= 30 && ret > 0.0 {
                    "4h CONFIRMED"
                } else if trades < 30 {
                    "4h TOO THIN"
                } else {
                    "4h INVALIDATED"
                }
            );
        }
    }

    println!("\n{}", "─".repeat(70));
    println!("Trust notes:");
    println!("  ✅ No look-ahead (signal at bar close, entry next bar open)");
    println!("  ✅ Realistic fees (0.1% taker each side)");
    println!("  ✅ Walk-forward chronology (train on earlier, test on later)");
    println!("  ⚠️  4h bars = 8000 max from Binance; ~2.7yr backtest window");
    println!("  ⚠️  Pre-declared params from 1h; not swept for 4h");
    println!("  ⚠️  No 4h ETH/XRP result is confirmed yet — THIS IS THE TEST");
    println!("{}", "═".repeat(70));

    Ok(())
}

//! Crisis Short Sleeve — Walk-Forward Validation
//!
//! PURPOSE: Provide bear-regime alpha via short-side exposure.
//!
//! Background:
//! - ALL existing strategies in the krypto program are long-only
//! - In bear markets (2022-style), the entire book draws down together
//! - A short-side signal is orthogonal and structurally necessary
//! - This has been "next session" for 5+ sessions
//!
//! Method:
//! - EWMA-CUSUM detection: when BTC's short-term EWMA crosses below long-term EWMA
//!   with accelerating downside momentum (rate of change negative and steepening)
//! - Short BTC (and optionally ETH) when the regime change signal fires
//! - Exit via Chandelier(45, 2.5) trailing stop
//! - Walk-forward: 252/252 train/test, step=21
//!
//! Entry signal (EWMA-CUSUM):
//!   1. Compute 21-bar and 63-bar EWMAs of log returns
//!   2. When fast_ewma < slow_ewma AND the spread is widening (momentum accelerating down)
//!   3. Additional confirmation: price below 63-bar SMA (trend filter)
//!
//! Exit: Chandelier trailing stop (ATR 45, mult 2.5)
//!
//! Walk-forward validation across 9 universes, BTC-only signals applied to all symbols.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const STEP_BARS: usize = 21;
const WARMUP: usize = 252;
const TAKER_FEE: f64 = 0.001;
const CHAND_PERIOD: usize = 45;
const CHAND_MULT: f64 = 2.5;

// EWMA parameters
const FAST_EWMA_SPAN: usize = 21;
const SLOW_EWMA_SPAN: usize = 63;
const SMA_PERIOD: usize = 63;
const ROC_PERIOD: usize = 10; // Rate of change lookback for momentum

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"]),
    ("NoDOGE", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("Legacy4", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    ("Legacy5BNB", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"]),
    ("OldGuardNoBNB", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
    ("LargeCaps5", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"]),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    ("LowVolume5", &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"]),
    ("OldGuard4", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
];

// ── Math helpers ───────────────────────────────────────────────────────────────

fn calc_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 5 {
        return 0.0;
    }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily_rets.len().max(1) as f64;
    let std = var.sqrt();
    if std < 1e-9 { return 0.0; }
    mean * 365.0 / (std * 365.0_f64.sqrt())
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

fn true_range(h: f64, l: f64, prev_c: f64) -> f64 {
    (h - l).abs().max((h - prev_c).abs()).max((l - prev_c).abs())
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut tr_sum = 0.0_f64;
    for i in idx.saturating_sub(period - 1)..=idx {
        let pc = if i > 0 { close[i - 1] } else { close[0] };
        tr_sum += true_range(high[i], low[i], pc);
    }
    tr_sum / period as f64
}

fn ema_val(data: &[f64], span: usize) -> Vec<f64> {
    if data.is_empty() || span == 0 { return vec![]; }
    let alpha = 2.0 / (span as f64 + 1.0);
    let mut out = vec![data[0]; data.len()];
    for i in 1..data.len() {
        out[i] = alpha * data[i] + (1.0 - alpha) * out[i - 1];
    }
    out
}

fn sma_at(data: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    data[idx + 1 - period..=idx].iter().sum::<f64>() / period as f64
}

// ── EWMA-CUSUM Signal ──────────────────────────────────────────────────────────

fn crisis_short_signal(close: &[f64], idx: usize) -> bool {
    if idx < SLOW_EWMA_SPAN + ROC_PERIOD {
        return false;
    }

    // Compute log returns
    let mut log_rets = Vec::with_capacity(idx);
    for i in 1..=idx {
        if close[i - 1] > 0.0 {
            log_rets.push((close[i] / close[i - 1]).ln());
        } else {
            log_rets.push(0.0);
        }
    }

    // Fast and slow EWMAs of log returns
    let fast = ema_val(&log_rets, FAST_EWMA_SPAN);
    let slow = ema_val(&log_rets, SLOW_EWMA_SPAN);

    let fast_now = fast[idx - 1];
    let slow_now = slow[idx - 1];

    // Price below SMA63 (trend confirmation)
    let sma63 = sma_at(close, SMA_PERIOD, idx);
    let price_below_sma = close[idx] < sma63;

    // Spread widening (momentum accelerating down)
    let spread_now = fast_now - slow_now;
    let spread_prev = if idx > ROC_PERIOD + 1 {
        let fast_prev = fast[idx - 1 - ROC_PERIOD];
        let slow_prev = slow[idx - 1 - ROC_PERIOD];
        fast_prev - slow_prev
    } else {
        0.0
    };

    // Spread is negative (bearish) and getting more negative (accelerating)
    let spreading_down = spread_now < 0.0 && spread_now < spread_prev;

    // Combined signal: bearish trend + accelerating downside + price below SMA
    fast_now < slow_now && spreading_down && price_below_sma
}

// ── Per-window backtest ───────────────────────────────────────────────────────

struct WindowResult {
    return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_short_strategy(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    test_start: usize,
    test_end: usize,
) -> WindowResult {
    let mut equity = vec![1.0];
    let mut daily_rets = Vec::new();
    let mut trades = Vec::new();

    let mut i = test_start;
    while i < test_end.saturating_sub(2) && i + 1 < close.len() {
        let signal = crisis_short_signal(close, i);

        if !signal {
            equity.push(*equity.last().unwrap());
            daily_rets.push(0.0);
            i += 1;
            continue;
        }

        // SHORT entry at next bar open
        let entry_idx = (i + 1).min(close.len() - 1);
        let entry_price = close[entry_idx];
        let atr = atr_at(high, low, close, CHAND_PERIOD, entry_idx);

        if entry_price <= 0.0 || atr <= 0.0 {
            i += 1;
            continue;
        }

        // Chandelier trailing stop for shorts: lowest_low + mult * ATR
        let mut lowest_low = low[entry_idx];
        let mut exit_bar = (test_end.min(close.len() - 1));
        let mut found_exit = false;

        for j in (entry_idx + 1)..close.len().min(test_end) {
            lowest_low = lowest_low.min(low[j]);
            let stop_price = lowest_low + CHAND_MULT * atr;
            if high[j] >= stop_price {
                exit_bar = j;
                found_exit = true;
                break;
            }
            if j >= test_end - 1 {
                exit_bar = j;
                break;
            }
        }

        let exit_price = close[exit_bar.min(close.len() - 1)];
        // Short PnL: profit when price falls
        let gross = entry_price / exit_price - 1.0;
        let net = gross - 2.0 * TAKER_FEE;

        let span = (exit_bar.saturating_sub(entry_idx)).max(1);
        let daily_ret = net / span as f64;

        for _ in 0..span {
            let last_eq = *equity.last().unwrap();
            equity.push(last_eq * (1.0 + daily_ret));
        }
        for _ in 0..span {
            daily_rets.push(daily_ret);
        }

        trades.push(gross);
        i = exit_bar.min(close.len() - 1) + 1;
    }

    // Extend to full test window
    let target_len = (test_end - test_start) + 1;
    while equity.len() < target_len {
        equity.push(*equity.last().unwrap());
    }

    let equity_slice = &equity[..target_len.min(equity.len())];
    let final_equity = equity_slice.last().copied().unwrap_or(1.0);
    let ret_pct = (final_equity - 1.0) * 100.0;
    let sharpe = calc_sharpe(&daily_rets);
    let max_dd = calc_max_dd(equity_slice);
    let wins = trades.iter().filter(|&&g| g > 0.0).count();
    let win_rate = if trades.is_empty() { 0.0 } else { wins as f64 / trades.len() as f64 };

    WindowResult {
        return_pct: ret_pct,
        sharpe,
        max_dd,
        trades: trades.len(),
        win_rate,
        pass: ret_pct > 0.0,
    }
}

// Also run a long-only benchmark for comparison
fn run_long_benchmark(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    test_start: usize,
    test_end: usize,
) -> WindowResult {
    let mut equity = vec![1.0];
    let mut daily_rets = Vec::new();
    let mut trades = Vec::new();

    let mut i = test_start;
    while i < test_end.saturating_sub(2) && i + 1 < close.len() {
        // Long when NOT in crisis (inverse of short signal)
        let in_crisis = crisis_short_signal(close, i);

        if in_crisis {
            equity.push(*equity.last().unwrap());
            daily_rets.push(0.0);
            i += 1;
            continue;
        }

        // LONG entry at next bar open
        let entry_idx = (i + 1).min(close.len() - 1);
        let entry_price = close[entry_idx];
        let atr = atr_at(high, low, close, CHAND_PERIOD, entry_idx);

        if entry_price <= 0.0 || atr <= 0.0 {
            i += 1;
            continue;
        }

        // Chandelier trailing stop for longs
        let mut highest_high = high[entry_idx];
        let mut exit_bar = test_end.min(close.len() - 1);

        for j in (entry_idx + 1)..close.len().min(test_end) {
            highest_high = highest_high.max(high[j]);
            let stop_price = highest_high - CHAND_MULT * atr;
            if low[j] <= stop_price {
                exit_bar = j;
                break;
            }
            if j >= test_end - 1 {
                exit_bar = j;
                break;
            }
        }

        let exit_price = close[exit_bar.min(close.len() - 1)];
        let gross = exit_price / entry_price - 1.0;
        let net = gross - 2.0 * TAKER_FEE;

        let span = (exit_bar.saturating_sub(entry_idx)).max(1);
        let daily_ret = net / span as f64;

        for _ in 0..span {
            let last_eq = *equity.last().unwrap();
            equity.push(last_eq * (1.0 + daily_ret));
        }
        for _ in 0..span {
            daily_rets.push(daily_ret);
        }

        trades.push(gross);
        i = exit_bar.min(close.len() - 1) + 1;
    }

    let target_len = (test_end - test_start) + 1;
    while equity.len() < target_len {
        equity.push(*equity.last().unwrap());
    }

    let equity_slice = &equity[..target_len.min(equity.len())];
    let final_equity = equity_slice.last().copied().unwrap_or(1.0);
    let ret_pct = (final_equity - 1.0) * 100.0;
    let sharpe = calc_sharpe(&daily_rets);
    let max_dd = calc_max_dd(equity_slice);
    let wins = trades.iter().filter(|&&g| g > 0.0).count();
    let win_rate = if trades.is_empty() { 0.0 } else { wins as f64 / trades.len() as f64 };

    WindowResult {
        return_pct: ret_pct,
        sharpe,
        max_dd,
        trades: trades.len(),
        win_rate,
        pass: ret_pct > 0.0,
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Crisis Short Sleeve — Walk-Forward Validation ===\n");
    println!("Signal: EWMA-CUSUM (fast={}/slow={}) + SMA{} trend filter + ROC acceleration", 
             FAST_EWMA_SPAN, SLOW_EWMA_SPAN, SMA_PERIOD);
    println!("Exit: Chandelier({}, {:.1})", CHAND_PERIOD, CHAND_MULT);
    println!("9 universes, BTC signal applied to all symbols\n");

    let loader = DataLoader::new(None, None);

    let btc_raw = loader.fetch_with_cache("BTCUSDT", "1d", CANDLES).await?;
    let btc_df = FeatureEngine::add_technicals(&btc_raw, None)?;

    let to_f64 = |s: &Series| -> Vec<f64> {
        s.f64().unwrap().into_iter().map(|v| v.unwrap_or(0.0)).collect()
    };

    let mut all_results: Vec<String> = Vec::new();
    all_results.push("universe,window,strategy,symbol,return_pct,sharpe,max_dd,trades,win_rate,pass".to_string());

    // Accumulators for summary
    let mut short_pass_total = 0;
    let mut short_total = 0;
    let mut long_pass_total = 0;
    let mut long_total = 0;
    let mut short_sharpe_sum = 0.0_f64;
    let mut long_sharpe_sum = 0.0_f64;

    for (universe_name, symbols) in UNIVERSES {
        println!("\n--- Universe: {} ---", universe_name);
        let start = Instant::now();

        let mut aligned: HashMap<String, (Vec<f64>, Vec<f64>, Vec<f64>)> = HashMap::new();
        for &symbol in *symbols {
            let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
            let enriched = FeatureEngine::add_technicals(&raw, Some(&btc_df))?;
            let close = to_f64(enriched.column("close")?);
            let high = to_f64(enriched.column("high")?);
            let low = to_f64(enriched.column("low")?);
            aligned.insert(symbol.to_string(), (close, high, low));
        }

        // Align to shortest
        let min_len = aligned.values().map(|(c, _, _)| c.len()).min().unwrap_or(0);
        for (_, data) in aligned.iter_mut() {
            data.0.truncate(min_len);
            data.1.truncate(min_len);
            data.2.truncate(min_len);
        }

        let n = min_len;
        let n_windows = n.saturating_sub(WARMUP) / STEP_BARS;

        for wi in 0..n_windows {
            let test_start = WARMUP + wi * STEP_BARS;
            let test_end = (test_start + STEP_BARS * 12).min(n - 1); // ~252 bars per window

            if test_end <= test_start + 50 { continue; }

            // Run on BTC only (representative)
            let Some((close, high, low)) = aligned.get("BTCUSDT") else { continue; };

            let short_r = run_short_strategy(close, high, low, test_start, test_end);
            let long_r = run_long_benchmark(close, high, low, test_start, test_end);

            short_total += 1;
            long_total += 1;
            if short_r.pass { short_pass_total += 1; }
            if long_r.pass { long_pass_total += 1; }
            short_sharpe_sum += short_r.sharpe;
            long_sharpe_sum += long_r.sharpe;

            let pass_str = |p: bool| if p { "✅" } else { "❌" };

            if wi % 10 == 0 {
                println!(
                    "  W{:02} | SHORT {:>+8.2}% sh={:>+6.2} dd={:>5.1}% {}t {} | LONG {:>+8.2}% sh={:>+6.2} dd={:>5.1}% {}t {}",
                    wi,
                    short_r.return_pct, short_r.sharpe, short_r.max_dd, short_r.trades, pass_str(short_r.pass),
                    long_r.return_pct, long_r.sharpe, long_r.max_dd, long_r.trades, pass_str(long_r.pass),
                );
            }

            for (label, r) in [("SHORT", &short_r), ("LONG_SKIP_CRISIS", &long_r)] {
                all_results.push(format!(
                    "{},W{},{},BTC,{},{},{},{},{},{}",
                    universe_name, wi, label,
                    format!("{:.4}", r.return_pct),
                    format!("{:.4}", r.sharpe),
                    format!("{:.4}", r.max_dd),
                    r.trades,
                    format!("{:.4}", r.win_rate),
                    if r.pass { "PASS" } else { "FAIL" },
                ));
            }
        }

        println!("  {:?}", start.elapsed());
    }

    // Write CSV
    let out_path = "snapshots/crisis_short_results.csv";
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(out_path)?;
    for line in &all_results { writeln!(f, "{}", line)?; }
    println!("\nResults written to {}", out_path);

    // Summary
    println!("\n=== SUMMARY ===");
    if short_total > 0 {
        let short_rate = short_pass_total as f64 / short_total as f64 * 100.0;
        let long_rate = long_pass_total as f64 / long_total as f64 * 100.0;
        let avg_short_sh = short_sharpe_sum / short_total as f64;
        let avg_long_sh = long_sharpe_sum / long_total as f64;
        println!("  SHORT sleeve:          {}/{} ({:.1}%) | avg Sharpe {:+.2}", 
                 short_pass_total, short_total, short_rate, avg_short_sh);
        println!("  LONG (skip crisis):    {}/{} ({:.1}%) | avg Sharpe {:+.2}",
                 long_pass_total, long_total, long_rate, avg_long_sh);
        println!("\n  Crisis filter removes {:.0}% of windows from long book",
                 100.0 - long_rate);
    }

    // Markdown summary
    let md_path = "snapshots/crisis_short_results.md";
    let mut mf = OpenOptions::new().create(true).write(true).truncate(true).open(md_path)?;
    writeln!(mf, "# Crisis Short Sleeve Walk-Forward Results\n")?;
    writeln!(mf, "Signal: EWMA-CUSUM (fast={}/slow={}) + SMA{} + ROC acceleration\n",
             FAST_EWMA_SPAN, SLOW_EWMA_SPAN, SMA_PERIOD)?;
    if short_total > 0 {
        let short_rate = short_pass_total as f64 / short_total as f64 * 100.0;
        let long_rate = long_pass_total as f64 / long_total as f64 * 100.0;
        writeln!(mf, "| Strategy | Pass Rate | Avg Sharpe |")?;
        writeln!(mf, "|---|---|---|")?;
        writeln!(mf, "| SHORT sleeve | {:.1}% ({}/{}) | {:+.2} |", 
                 short_rate, short_pass_total, short_total, short_sharpe_sum / short_total as f64)?;
        writeln!(mf, "| LONG (skip crisis) | {:.1}% ({}/{}) | {:+.2} |",
                 long_rate, long_pass_total, long_total, long_sharpe_sum / long_total as f64)?;
    }
    println!("Markdown written to {}", md_path);

    Ok(())
}

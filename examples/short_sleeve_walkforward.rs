//! Short-Side Sleeve — Vol-Threshold Walk-Forward
//!
//! PURPOSE: Test the PLAN-specified bear-market short signal.
//! Plan hypothesis: BTC 21d vol > 90th pct of 252d AND SMA21 < SMA200 → short.
//!
//! This is T51 from PLAN.md — 4+ weeks overdue.
//!
//! Signal fires ONLY in high-vol bear regimes (not choppy/crisis V-shape).
//! Exit: Turtle ATR trailing stop (same mechanism as long side).
//!
//! Walk-forward: 9 universes, 7 windows. Guardrail: ≥69.1% pass.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const STEP_BARS: usize = 63;
const WARMUP: usize = 300;
const TAKER_FEE: f64 = 0.001;

const ATR_PERIOD: usize = 24;
const ATR_MULT: f64 = 2.0;
const HOLD_MAX: usize = 12;

// Vol threshold params (PLAN hypothesis)
const VOL_LOOKBACK: usize = 252;
const VOL_PCT_THRESHOLD: f64 = 90.0; // 90th percentile
const SMA21_PERIOD: usize = 21;
const SMA200_PERIOD: usize = 200;

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
    if daily_rets.len() < 10 { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily_rets.len().max(1) as f64;
    let std = var.sqrt().max(1e-9);
    mean * 365.0_f64.sqrt() / std
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

fn pct_rank(value: f64, history: &[f64]) -> f64 {
    if history.is_empty() { return 50.0; }
    let below = history.iter().filter(|&&v| v < value).count() as f64;
    below / history.len() as f64 * 100.0
}

// ── Signal ──────────────────────────────────────────────────────────────────────

/// Returns true when bear-regime short signal fires.
/// Conditions: BTC 21d ATR > 90th pct of 252-bar history AND SMA21 < SMA200.
fn bear_short_signal(
    btc_close: &[f64],
    btc_high: &[f64],
    btc_low: &[f64],
    idx: usize,
) -> bool {
    if idx < VOL_LOOKBACK.max(SMA200_PERIOD) + 5 {
        return false;
    }

    // 21-bar ATR at idx
    let mut tr_sum = 0.0_f64;
    for i in idx.saturating_sub(ATR_PERIOD - 1)..=idx {
        let prev_c = if i > 0 { btc_close[i - 1] } else { btc_close[0] };
        let tr = (btc_high[i] - btc_low[i])
            .abs()
            .max((btc_high[i] - prev_c).abs())
            .max((btc_low[i] - prev_c).abs());
        tr_sum += tr;
    }
    let current_atr = tr_sum / ATR_PERIOD as f64;
    if current_atr <= 0.0 { return false; }

    // ATR history: gather 252 non-overlapping 21-bar ATRs ending at idx-1
    let mut atr_hist: Vec<f64> = Vec::with_capacity(VOL_LOOKBACK / 21);
    let mut j = idx.saturating_sub(VOL_LOOKBACK);
    while j + ATR_PERIOD <= idx.saturating_sub(1) {
        let mut sum = 0.0_f64;
        for k in j..j + ATR_PERIOD {
            let prev_c = if k > 0 { btc_close[k - 1] } else { btc_close[0] };
            let tr = (btc_high[k] - btc_low[k])
                .abs()
                .max((btc_high[k] - prev_c).abs())
                .max((btc_low[k] - prev_c).abs());
            sum += tr;
        }
        atr_hist.push(sum / ATR_PERIOD as f64);
        j += ATR_PERIOD;
    }
    if atr_hist.len() < 5 { return false; }

    // Vol rank: current ATR percentile in history
    let vol_pct = pct_rank(current_atr, &atr_hist);
    if vol_pct < VOL_PCT_THRESHOLD {
        return false; // not high-vol enough
    }

    // SMA21 < SMA200 (bear trend)
    let sum21: f64 = btc_close[idx + 1 - SMA21_PERIOD..=idx].iter().sum();
    let sma21 = sum21 / SMA21_PERIOD as f64;

    let sum200: f64 = btc_close[idx + 1 - SMA200_PERIOD..=idx].iter().sum();
    let sma200 = sum200 / SMA200_PERIOD as f64;

    sma21 < sma200
}

// ── Turtle ATR exit (for shorts — highest_high → stop below) ─────────────────

fn turtle_exit_short(
    entry_idx: usize,
    exit_idx: usize,
    high: &[f64],
    low: &[f64],
    close: &[f64],
) -> (usize, f64) {
    let entry_price = close[entry_idx];

    // Compute ATR at entry
    let mut tr_sum = 0.0_f64;
    for i in entry_idx.saturating_sub(ATR_PERIOD - 1)..=entry_idx {
        let prev_c = if i > 0 { close[i - 1] } else { close[0] };
        let tr = (high[i] - low[i])
            .abs()
            .max((high[i] - prev_c).abs())
            .max((low[i] - prev_c).abs());
        tr_sum += tr;
    }
    let atr = tr_sum / ATR_PERIOD as f64;

    let mut lowest_low = low[entry_idx];
    let mut stop_bar = exit_idx;

    for j in entry_idx + 1..=exit_idx {
        lowest_low = lowest_low.min(low[j]);
        let stop_price = lowest_low + ATR_MULT * atr;
        // For short: exit if price rallies to stop
        if high[j] >= stop_price {
            stop_bar = j;
            break;
        }
        if j == exit_idx {
            stop_bar = j;
        }
    }

    let exit_price = close[stop_bar];
    let gross = if entry_price > 0.0 { exit_price / entry_price - 1.0 } else { 0.0 };

    // Short: profit when price falls
    let net = gross - 2.0 * TAKER_FEE;
    (stop_bar, net)
}

// ── Per-window backtest ────────────────────────────────────────────────────────

struct WindowResult {
    return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_short_sleeve(
    btc_close: &[f64],
    btc_high: &[f64],
    btc_low: &[f64],
    symbol_close: &[f64],
    symbol_high: &[f64],
    symbol_low: &[f64],
    test_start: usize,
    test_end: usize,
) -> WindowResult {
    let mut equity = vec![1.0_f64];
    let mut daily_rets = Vec::new();
    let mut trades = Vec::new();
    let mut pos_open = false;
    let mut entry_bar = 0_usize;

    for i in test_start..test_end.saturating_sub(1) {
        if !pos_open {
            // Check signal
            if bear_short_signal(btc_close, btc_high, btc_low, i) {
                // Short entry next bar
                let entry_idx = (i + 1).min(symbol_close.len() - 1);
                if entry_idx >= test_end { break; }
                pos_open = true;
                entry_bar = entry_idx;
            } else {
                // No position — equity holds
                equity.push(*equity.last().unwrap());
                daily_rets.push(0.0);
            }
        } else {
            // In short position — check exit
            let hold_bars = i.saturating_sub(entry_bar);

            // Turtle ATR exit
            let (exit_bar, pnl) = turtle_exit_short(entry_bar, i, symbol_high, symbol_low, symbol_close);

            if exit_bar != i || hold_bars >= HOLD_MAX {
                // Exit taken
                let span = (exit_bar.saturating_sub(entry_bar)).max(1);
                let daily_ret = pnl / span as f64;
                for _ in 0..span {
                    let last_eq = *equity.last().unwrap();
                    equity.push(last_eq * (1.0 + daily_ret));
                }
                for _ in 0..span {
                    daily_rets.push(daily_ret);
                }
                trades.push(pnl);
                pos_open = false;
            } else {
                // Continue holding
                let last_eq = *equity.last().unwrap();
                equity.push(last_eq);
                daily_rets.push(0.0);
            }
        }
    }

    // Close any open position at test_end
    if pos_open {
        let exit_idx = (test_end - 1).min(symbol_close.len() - 1);
        let span = (exit_idx.saturating_sub(entry_bar)).max(1);
        let pnl = symbol_close[exit_idx] / symbol_close[entry_bar] - 1.0 - 2.0 * TAKER_FEE;
        let daily_ret = pnl / span as f64;
        for _ in 0..span {
            let last_eq = *equity.last().unwrap();
            equity.push(last_eq * (1.0 + daily_ret));
        }
        for _ in 0..span {
            daily_rets.push(daily_ret);
        }
        trades.push(pnl);
    }

    // Pad to test_end
    while equity.len() < test_end - test_start + 1 {
        equity.push(*equity.last().unwrap());
    }

    let eq = &equity[..(test_end - test_start + 1).min(equity.len())];
    let final_eq = eq.last().copied().unwrap_or(1.0);
    let ret_pct = (final_eq - 1.0) * 100.0;
    let sharpe = calc_sharpe(&daily_rets);
    let max_dd = calc_max_dd(eq);
    let wins = trades.iter().filter(|&&p| p > 0.0).count();
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
    println!("=== Short-Side Sleeve — Vol-Threshold Walk-Forward (T51) ===\n");
    println!("Signal: 21d ATR > 90th pct of 252-bar history AND SMA21 < SMA200");
    println!("Exit: Turtle ATR({}, {}) | Max hold: {} bars\n", ATR_PERIOD, ATR_MULT, HOLD_MAX);

    let loader = DataLoader::new(None, None);

    let btc_raw = loader.fetch_with_cache("BTCUSDT", "1d", CANDLES).await?;
    let btc_enr = FeatureEngine::add_technicals(&btc_raw, None)?;

    let to_f64 = |s: &Series| -> Vec<f64> {
        s.f64().unwrap().into_iter().map(|v| v.unwrap_or(0.0)).collect()
    };

    let btc_close = to_f64(btc_enr.column("close")?);
    let btc_high = to_f64(btc_enr.column("high")?);
    let btc_low = to_f64(btc_enr.column("low")?);

    let mut all_results: Vec<String> = Vec::new();
    all_results.push("universe,window,return_pct,sharpe,max_dd,trades,win_rate,pass".to_string());

    let mut total_pass = 0;
    let mut total_windows = 0;
    let mut sharpe_sum = 0.0_f64;
    let mut ret_sum = 0.0_f64;
    let mut trade_sum = 0;

    let mut per_universe: HashMap<&str, (usize, usize, f64, f64, usize)> = HashMap::new();

    for (universe_name, symbols) in UNIVERSES {
        println!("--- Universe: {} ---", universe_name);
        let start = Instant::now();

        // Align all symbols to BTC
        let mut aligned: HashMap<String, (Vec<f64>, Vec<f64>, Vec<f64>)> = HashMap::new();
        for &symbol in *symbols {
            let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
            let enriched = FeatureEngine::add_technicals(&raw, Some(&btc_enr))?;
            let close = to_f64(enriched.column("close")?);
            let high = to_f64(enriched.column("high")?);
            let low = to_f64(enriched.column("low")?);
            aligned.insert(symbol.to_string(), (close, high, low));
        }

        let min_len = aligned.values().map(|(c, _, _)| c.len()).min().unwrap_or(0);

        let n_windows = (min_len.saturating_sub(WARMUP) / STEP_BARS).max(1);

        let mut uni_pass = 0;
        let mut uni_total = 0;
        let mut uni_sharpe = 0.0_f64;
        let mut uni_ret = 0.0_f64;
        let mut uni_trades = 0_usize;

        for wi in 0..n_windows {
            let test_start = WARMUP + wi * STEP_BARS;
            let test_end = (test_start + STEP_BARS * 5).min(min_len - 1);

            if test_end <= test_start + 50 { continue; }

            // Run on BTC as the regime signal, applied to BTC itself
            let Some((sym_close, sym_high, sym_low)) = aligned.get("BTCUSDT") else { continue; };

            let r = run_short_sleeve(
                &btc_close, &btc_high, &btc_low,
                sym_close, sym_high, sym_low,
                test_start, test_end,
            );

            total_pass += if r.pass { 1 } else { 0 };
            total_windows += 1;
            sharpe_sum += r.sharpe;
            ret_sum += r.return_pct;
            trade_sum += r.trades;
            uni_pass += if r.pass { 1 } else { 0 };
            uni_total += 1;
            uni_sharpe += r.sharpe;
            uni_ret += r.return_pct;
            uni_trades += r.trades;

            let pass_str = if r.pass { "✅" } else { "❌" };
            if wi % 10 == 0 || wi < 3 {
                println!(
                    "  W{:02} | RET {:>+8.1}% SH {:>+6.2} DD {:>5.1}% {}t {}",
                    wi, r.return_pct, r.sharpe, r.max_dd, r.trades, pass_str
                );
            }

            all_results.push(format!(
                "{},W{},{},{},{},{},{},{}",
                universe_name, wi,
                format!("{:.4}", r.return_pct),
                format!("{:.4}", r.sharpe),
                format!("{:.4}", r.max_dd),
                r.trades,
                format!("{:.4}", r.win_rate),
                if r.pass { "PASS" } else { "FAIL" },
            ));
        }

        let uni_rate = if uni_total > 0 { uni_pass as f64 / uni_total as f64 * 100.0 } else { 0.0 };
        println!(
            "  {} windows | {} pass ({:.0}%) | avg Sharpe {:+.2} | avg ret {:+.1}%",
            uni_total, uni_pass, uni_rate, uni_sharpe / uni_total.max(1) as f64, uni_ret / uni_total.max(1) as f64
        );
        println!("  Elapsed: {:?}\n", start.elapsed());

        per_universe.insert(universe_name, (uni_pass, uni_total, uni_sharpe, uni_ret, uni_trades));
    }

    // Write CSV
    let out_path = "snapshots/short_sleeve_results.csv";
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(out_path)?;
    for line in &all_results { writeln!(f, "{}", line)?; }

    // Summary
    let pass_rate = if total_windows > 0 { total_pass as f64 / total_windows as f64 * 100.0 } else { 0.0 };
    let avg_sharpe = sharpe_sum / total_windows.max(1) as f64;
    let avg_ret = ret_sum / total_windows.max(1) as f64;

    println!("\n=== FINAL SUMMARY ===");
    println!("  Overall: {}/{} pass ({:.1}%) | avg Sharpe {:+.2} | avg return {:+.1}% | {} trades",
             total_pass, total_windows, pass_rate, avg_sharpe, avg_ret, trade_sum);

    let guardrail = 69.1;
    let verdict = if pass_rate >= guardrail { "PASS → HOF candidate" } else { "FAIL → GRAVEYARD" };
    println!("  Guardrail (≥69.1%): {:.1}% → {}", pass_rate, verdict);

    // Per-universe table
    println!("\n  Per-universe:");
    println!("  {:20} {:>8} {:>10} {:>10} {:>8}", "Universe", "Pass/Total", "PassRate", "AvgSharpe", "Trades");
    println!("  {}", "-".repeat(60));
    for (name, (p, t, sh, _re, tr)) in &per_universe {
        let rate = if *t > 0 { *p as f64 / *t as f64 * 100.0 } else { 0.0 };
        println!("  {:20} {:>4}/{:>4} {:>9.0}% {:>+10.2} {:>8}", name, p, t, rate, sh / (*t).max(1) as f64, tr);
    }

    // Write markdown
    let md_path = "snapshots/short_sleeve_results.md";
    let mut mf = OpenOptions::new().create(true).write(true).truncate(true).open(md_path)?;
    writeln!(mf, "# Short-Side Sleeve Walk-Forward Results (T51)\n")?;
    writeln!(mf, "Signal: 21d ATR > 90th pct of 252-bar ATR history AND SMA21 < SMA200\n")?;
    writeln!(mf, "Exit: Turtle ATR({}, {}) | Guardrail: ≥69.1%\n", ATR_PERIOD, ATR_MULT)?;
    writeln!(mf, "## Global Summary\n")?;
    writeln!(mf, "| Metric | Value |")?;
    writeln!(mf, "|---|---|")?;
    writeln!(mf, "| Pass Rate | {:.1}% ({}/{}) |", pass_rate, total_pass, total_windows)?;
    writeln!(mf, "| Avg Sharpe | {:+.2} |", avg_sharpe)?;
    writeln!(mf, "| Avg Return | {:+.1}% |", avg_ret)?;
    writeln!(mf, "| Total Trades | {} |", trade_sum)?;
    writeln!(mf, "| Guardrail | ≥69.1% → {} |", if pass_rate >= guardrail { "PASS" } else { "FAIL" })?;
    writeln!(mf, "\n## Per-Universe\n")?;
    writeln!(mf, "| Universe | Pass/Total | Rate | Avg Sharpe |")?;
    writeln!(mf, "|---|---|---|---|")?;
    for (name, (p, t, sh, _, _)) in &per_universe {
        let rate = if *t > 0 { *p as f64 / *t as f64 * 100.0 } else { 0.0 };
        writeln!(mf, "| {} | {}/{} | {:.0}% | {:+.2} |", name, p, t, rate, sh / (*t).max(1) as f64)?;
    }

    println!("\nResults: {}", out_path);
    println!("Markdown: {}", md_path);

    Ok(())
}
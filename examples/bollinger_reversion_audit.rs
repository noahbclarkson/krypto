//! BollingerReversion Post-Fix OOS Audit — 9-Universe Walk-Forward
//!
//! PURPOSE: Definitively audit BollingerReversion with post-fix code.
//! HOF still shows pre-fix DOGE Sharpe 19.01. The RSI hyperopt showed
//! negative aggregate OOS Sharpe. Random-entry beats the signal 24-35%.
//!
//! QUESTIONS:
//!   1. Does BollingerReversion survive rigorous 9-universe walk-forward?
//!   2. Is the signal adding value beyond the ATR×0.30 stop?
//!   3. Should it stay in HOF or be downgraded to GRAVEYARD?
//!
//! CONFIGS TESTED:
//!   - HOF_ORIGINAL: bb=30, std=2.5, rsi=20 (original HOF settings, pre-fix)
//!   - POST_FIX:     bb=20, std=2.0, rsi=35 (current deployed, post-hyperopt)
//!   - RANDOM_ENTRY: random entry + ATR×0.30 stop (signal-value baseline)
//!
//! METHOD: Walk-forward 252/252, per-symbol, aggregate by universe
//! FEE: 0.1% taker each side
//! HOLD: max 21 bars (matches equity curve harness)
//!
//! Usage:
//!   cargo run --profile sweep --example bollinger_reversion_audit 2>&1

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

// ── Constants ──────────────────────────────────────────────────────────────

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 21;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30;
const ATR_PERIOD: usize = 14;
const MIN_TRADES: usize = 3;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

const CSV_OUT: &str = "snapshots/bollinger_reversion_audit.csv";
const MD_OUT: &str = "snapshots/bollinger_reversion_audit.md";

// ── Config structs ─────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct BrConfig {
    label: &'static str,
    bb_period: usize,
    bb_std: f64,
    rsi_filter: f64,
    random_entry: bool,
}

const CONFIGS: &[BrConfig] = &[
    BrConfig { label: "HOF_ORIG", bb_period: 30, bb_std: 2.5, rsi_filter: 20.0, random_entry: false },
    BrConfig { label: "POST_FIX", bb_period: 20, bb_std: 2.0, rsi_filter: 35.0, random_entry: false },
    BrConfig { label: "RANDOM",   bb_period: 20, bb_std: 2.0, rsi_filter: 35.0, random_entry: true },
];

// ── Data ───────────────────────────────────────────────────────────────────

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

// ── Indicator helpers ──────────────────────────────────────────────────────

fn compute_atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let n = close.len();
    let mut atr = vec![0.0f64; n];
    if n < period + 1 { return atr; }

    let mut sum_tr = 0.0;
    for i in 0..=period.min(n - 1) {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let tr = if i == 0 {
            h - l
        } else {
            let c0 = close.get(i - 1).copied().unwrap_or(0.0);
            (h - l).max((h - c0).abs()).max((l - c0).abs())
        };
        sum_tr += tr;
    }
    atr[period] = sum_tr / period as f64;

    for i in (period + 1)..n {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i - 1).copied().unwrap_or(0.0);
        let tr = (h - l).max((h - c0).abs()).max((l - c0).abs());
        atr[i] = (atr[i - 1] * (period as f64 - 1.0) + tr) / period as f64;
    }
    atr
}

fn compute_rsi(close: &[f64], period: usize) -> Vec<f64> {
    let n = close.len();
    let mut rsi = vec![50.0f64; n];
    if n < period + 1 { return rsi; }

    let mut avg_gain = 0.0f64;
    let mut avg_loss = 0.0f64;

    for i in 1..=period {
        let delta = close.get(i).copied().unwrap_or(0.0) - close.get(i - 1).copied().unwrap_or(0.0);
        avg_gain += delta.max(0.0);
        avg_loss += (-delta).max(0.0);
    }
    avg_gain /= period as f64;
    avg_loss /= period as f64;

    for i in (period + 1)..n {
        let delta = close.get(i).copied().unwrap_or(0.0) - close.get(i - 1).copied().unwrap_or(0.0);
        avg_gain = (avg_gain * (period as f64 - 1.0) + delta.max(0.0)) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + (-delta).max(0.0)) / period as f64;
        if avg_loss == 0.0 {
            rsi[i] = 100.0;
        } else {
            let rs = avg_gain / avg_loss;
            rsi[i] = 100.0 - (100.0 / (1.0 + rs));
        }
    }
    rsi
}

fn compute_bb_lower(close: &[f64], period: usize, std_mult: f64) -> Vec<f64> {
    let n = close.len();
    let mut lower = vec![f64::NAN; n];

    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut count = 0usize;

    for i in 0..n {
        let price = close.get(i).copied().unwrap_or(0.0);
        sum += price;
        sum_sq += price * price;
        count += 1;

        if count > period {
            sum -= close.get(i - period).copied().unwrap_or(0.0);
            sum_sq -= close.get(i - period).copied().unwrap_or(0.0) * close.get(i - period).copied().unwrap_or(0.0);
            count -= 1;
        }

        if count == period {
            let mean = sum / period as f64;
            let var = (sum_sq / period as f64) - (mean * mean);
            let std = var.max(0.0).sqrt();
            lower[i] = mean - std * std_mult;
        }
    }
    lower
}

// ── Metrics ────────────────────────────────────────────────────────────────

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd < 1e-10 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        if peak > 0.0 {
            let dd = (peak - e) / peak;
            if dd > max_dd { max_dd = dd; }
        }
    }
    max_dd * 100.0
}

// ── Per-symbol walk-forward result ─────────────────────────────────────────

#[derive(Clone)]
struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

/// Run one symbol through walk-forward windows for one config.
/// Returns per-window results.
fn run_symbol_wf(
    sd: &SymData,
    config: BrConfig,
) -> Vec<(usize, WfResult)> {
    let n = sd.close.len();
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    if total_windows == 0 { return vec![]; }

    // Precompute indicators on full series (no look-ahead: signals use data up to bar only)
    let bb_lower = compute_bb_lower(&sd.close, config.bb_period, config.bb_std);
    let rsi = compute_rsi(&sd.close, 14);
    let atr = compute_atr(&sd.high, &sd.low, &sd.close, ATR_PERIOD);

    let mut results = Vec::new();

    for wi in 0..total_windows {
        let test_start = TRAIN_BARS + wi * TEST_BARS;
        let test_end = (test_start + TEST_BARS).min(n);
        if test_end.saturating_sub(test_start) < 5 { continue; }

        let mut equity = 1.0_f64;
        let mut equity_curve = vec![1.0_f64];
        let mut wins = 0usize;
        let mut total_trades = 0usize;
        let mut daily_rets = Vec::new();

        let mut bar = test_start;

        while bar + 1 < test_end {
            // ── Signal generation ──
            // NO look-ahead: use bar-1 signal to enter at bar's close
            // (matches backtest engine fix: signals[i-1] executes at closes[i])
            let signal_bar = bar.saturating_sub(1);
            let mut go_long = false;

            if config.random_entry {
                // Random entry: use a deterministic pseudo-random based on bar index
                // This avoids introducing `rand` dependency while still being "random"
                // Simple LCG: always produces same sequence for reproducibility
                let hash = (bar.wrapping_mul(2654435761) ^ 0x12345678) as u64;
                let frac = (hash & 0xFFFFFF) as f64 / 0xFFFFFF as f64;
                // Enter on ~15% of bars (matches typical Bollinger entry frequency)
                go_long = frac < 0.15;
            } else {
                // BollingerReversion signal: close < BB_lower AND RSI < filter
                let c = sd.close.get(signal_bar).copied().unwrap_or(0.0);
                let bl = bb_lower.get(signal_bar).copied().unwrap_or(f64::NAN);
                let r = rsi.get(signal_bar).copied().unwrap_or(50.0);

                if bl.is_finite() && c < bl && r < config.rsi_filter {
                    go_long = true;
                }
            }

            if !go_long {
                equity_curve.push(equity);
                bar += 1;
                continue;
            }

            // ── Enter long at current bar's close ──
            let entry_px = sd.close[bar];
            let entry = entry_px * (1.0 - TAKER_FEE); // pay taker on entry
            let stop_pct = if bar < atr.len() && atr[bar] > 0.0 && entry_px > 0.0 {
                (atr[bar] * ATR_MULT / entry_px).clamp(0.005, 0.30)
            } else {
                0.03 // fallback ~3%
            };

            // ── Hold with ATR×0.30 trailing stop ──
            let mut lowest = entry_px;
            let mut exit_bar = (bar + HOLD_MAX).min(test_end.saturating_sub(1));
            let mut stopped = false;

            for b in (bar + 1)..exit_bar.min(n.saturating_sub(1)) {
                let px = sd.close.get(b).copied().unwrap_or(entry_px);
                if px < lowest { lowest = px; }

                // Trailing stop: if price drops > stop_pct from entry
                let drop_pct = (entry_px - px) / entry_px;
                if drop_pct > stop_pct {
                    exit_bar = b;
                    stopped = true;
                    break;
                }

                // Also check: price recovered to BB_mean? (natural Bollinger exit)
                // Use simple mean reversion: if close > entry_px (profitable), check if
                // it crossed back above BB midline
                let mean_bb = {
                    let mut s = 0.0;
                    let mut cnt = 0;
                    for j in (b.saturating_sub(config.bb_period - 1))..=b {
                        if let Some(&p) = sd.close.get(j) { s += p; cnt += 1; }
                    }
                    if cnt > 0 { s / cnt as f64 } else { entry_px }
                };
                if px > mean_bb {
                    exit_bar = b;
                    stopped = false; // natural exit, not stopped out
                    break;
                }
            }

            // ── Record trade ──
            let exit_px = sd.close.get(exit_bar).copied().unwrap_or(entry_px);
            let exit_val = exit_px * (1.0 - TAKER_FEE); // pay taker on exit
            let gross_ret = exit_val / entry - 1.0;
            let bars_held = (exit_bar as i64 - bar as i64).max(1) as usize;

            wins += if gross_ret > 0.0 { 1 } else { 0 };
            total_trades += 1;
            equity *= 1.0 + gross_ret;

            let avg_daily = gross_ret / bars_held as f64;
            for _ in 0..bars_held {
                daily_rets.push(avg_daily);
            }

            equity_curve.push(equity);
            bar = exit_bar + 1;
        }

        let ret = (equity - 1.0) * 100.0;
        let sharpe = annualised_sharpe(&daily_rets);
        let max_dd = max_dd_from(&equity_curve);
        let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
        let pass = total_trades >= MIN_TRADES && ret > 0.0;

        results.push((wi, WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }));
    }

    results
}

// ── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("╔══════════════════════════════════════════════════════════════════╗");
    eprintln!("║  BOLLINGERREVERSION POST-FIX OOS AUDIT — 9-UNIVERSE WF         ║");
    eprintln!("║  HOF_ORIG: bb=30 std=2.5 rsi=20  |  POST_FIX: bb=20 std=2.0 rsi=35  ║");
    eprintln!("║  RANDOM: ATR×0.30 stop only (signal-value baseline)            ║");
    eprintln!("╚══════════════════════════════════════════════════════════════════╝\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // ── Run all configs × all universes × all symbols ──────────────────────

    let mut csv_lines = vec![
        "config,universe,symbol,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()
    ];

    // Aggregate: config → universe → (pass_count, total_windows, avg_ret, avg_sharpe, trades)
    let mut agg: HashMap<&str, HashMap<&str, (usize, usize, f64, f64, usize)>> = HashMap::new();

    for config in CONFIGS {
        eprintln!("═══ Config: {} ═══", config.label);

        for &(uni_label, symbols) in UNIVERSES {
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(*s));
            if !all_loaded {
                eprintln!("  {:>18} SKIPPED (missing data)", uni_label);
                continue;
            }

            let mut uni_pass = 0usize;
            let mut uni_total = 0usize;
            let mut uni_ret_sum = 0.0f64;
            let mut uni_sharpe_sum = 0.0f64;
            let mut uni_trades = 0usize;

            for &sym in symbols {
                let sd = sym_data_map.get(sym).unwrap();
                let wf_results = run_symbol_wf(sd, *config);

                for (wi, r) in &wf_results {
                    let thin = if r.trades < MIN_TRADES { "THIN" } else { "OK" };
                    let result = if r.pass { "PASS" } else { "FAIL" };

                    csv_lines.push(format!(
                        "{},{},{},W{:02},{:.2},{:.4},{:.2},{},{:.2},{}",
                        config.label, uni_label, sym, wi,
                        r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                    ));

                    if r.pass { uni_pass += 1; }
                    uni_total += 1;
                    uni_ret_sum += r.ret;
                    uni_sharpe_sum += r.sharpe;
                    uni_trades += r.trades;
                }
            }

            if uni_total > 0 {
                let avg_ret = uni_ret_sum / uni_total as f64;
                let avg_sharpe = uni_sharpe_sum / uni_total as f64;
                let pass_pct = uni_pass as f64 / uni_total as f64 * 100.0;
                eprintln!(
                    "  {:>18} | {}/{} pass ({:.0}%) | avg {:+.1}% | avg Sharpe {:.2} | {} trades",
                    uni_label, uni_pass, uni_total, pass_pct, avg_ret, avg_sharpe, uni_trades
                );

                agg.entry(config.label)
                    .or_insert_with(HashMap::new)
                    .insert(uni_label, (uni_pass, uni_total, avg_ret, avg_sharpe, uni_trades));
            }
        }
        eprintln!();
    }

    // ── Write CSV ──────────────────────────────────────────────────────────
    {
        let mut f = File::create(CSV_OUT)?;
        for line in &csv_lines { writeln!(f, "{}", line)?; }
    }

    // ── Global summary per config ──────────────────────────────────────────
    eprintln!("╔══════════════════════════════════════════════════════════════════╗");
    eprintln!("║  GLOBAL SUMMARY                                                 ║");
    eprintln!("╚══════════════════════════════════════════════════════════════════╝");

    let mut global_summary = Vec::new();

    for config in CONFIGS {
        if let Some(uni_map) = agg.get(config.label) {
            let total_pass: usize = uni_map.values().map(|(p,_,_,_,_)| *p).sum();
            let total_windows: usize = uni_map.values().map(|(_,t,_,_,_)| *t).sum();
            let total_trades: usize = uni_map.values().map(|(_,_,_,_,tr)| *tr).sum();
            let avg_sharpe: f64 = {
                let (sum, cnt) = uni_map.values()
                    .map(|(_,_,_,sh,_)| (*sh, 1usize))
                    .fold((0.0f64, 0usize), |(s, c), (sh, n)| (s + sh, c + n));
                sum / cnt.max(1) as f64
            };
            let avg_ret: f64 = {
                let (sum, cnt) = uni_map.values()
                    .map(|(_,_,r,_,_)| (*r, 1usize))
                    .fold((0.0f64, 0usize), |(s, c), (r, n)| (s + r, c + n));
                sum / cnt.max(1) as f64
            };
            let pass_pct = total_pass as f64 / total_windows.max(1) as f64 * 100.0;

            eprintln!(
                "  {:>10} | {}/{} pass ({:.0}%) | avg {:+.1}% | avg Sharpe {:.2} | {} trades",
                config.label, total_pass, total_windows, pass_pct, avg_ret, avg_sharpe, total_trades
            );

            global_summary.push((config.label, total_pass, total_windows, pass_pct, avg_ret, avg_sharpe, total_trades));
        }
    }

    // ── Per-config per-universe comparison table ───────────────────────────
    eprintln!("\n╔══════════════════════════════════════════════════════════════════╗");
    eprintln!("║  PER-UNIVERSE COMPARISON                                        ║");
    eprintln!("╚══════════════════════════════════════════════════════════════════╝");
    eprintln!("{:>18} │ {:>14} │ {:>14} │ {:>14}", "Universe", "HOF_ORIG", "POST_FIX", "RANDOM");

    for &(uni_label, _) in UNIVERSES {
        let mut row = Vec::new();
        for config in CONFIGS {
            if let Some(uni_map) = agg.get(config.label) {
                if let Some((pass, total, _, _, _)) = uni_map.get(uni_label) {
                    row.push(format!("{}/{}", pass, total));
                } else {
                    row.push("—".to_string());
                }
            } else {
                row.push("—".to_string());
            }
        }
        eprintln!("{:>18} │ {:>14} │ {:>14} │ {:>14}", uni_label, row[0], row[1], row[2]);
    }

    // ── Write MD report ────────────────────────────────────────────────────
    {
        let mut md = File::create(MD_OUT)?;
        writeln!(md, "# BollingerReversion Post-Fix OOS Audit — 9-Universe Walk-Forward")?;
        writeln!(md, "")?;
        writeln!(md, "**Date:** 2026-04-11")?;
        writeln!(md, "**Purpose:** Definitively audit BollingerReversion post-fix.")?;
        writeln!(md, "")?;
        writeln!(md, "## Configs Tested")?;
        writeln!(md, "")?;
        writeln!(md, "| Config | BB Period | BB Std | RSI Filter | Random Entry |")?;
        writeln!(md, "|--------|-----------|--------|------------|--------------|")?;
        writeln!(md, "| HOF_ORIG | 30 | 2.5 | 20 | No |")?;
        writeln!(md, "| POST_FIX | 20 | 2.0 | 35 | No |")?;
        writeln!(md, "| RANDOM | — | — | — | Yes (15% freq) |")?;
        writeln!(md, "")?;

        writeln!(md, "## Global Summary")?;
        writeln!(md, "")?;
        writeln!(md, "| Config | Pass | Total | Pass% | Avg Ret | Avg Sharpe | Trades |")?;
        writeln!(md, "|--------|------|-------|-------|---------|------------|--------|")?;
        for (label, pass, total, pct, ret, sh, trades) in &global_summary {
            writeln!(md, "| {} | {} | {} | {:.0}% | {:+.1}% | {:.2} | {} |",
                label, pass, total, pct, ret, sh, trades)?;
        }
        writeln!(md, "")?;

        writeln!(md, "## Per-Universe Comparison")?;
        writeln!(md, "")?;
        writeln!(md, "| Universe | HOF_ORIG Pass | POST_FIX Pass | RANDOM Pass |")?;
        writeln!(md, "|----------|---------------|---------------|-------------|")?;
        for &(uni_label, _) in UNIVERSES {
            let mut row = Vec::new();
            for config in CONFIGS {
                if let Some(uni_map) = agg.get(config.label) {
                    if let Some((pass, total, _, _, _)) = uni_map.get(uni_label) {
                        row.push(format!("{}/{}", pass, total));
                    } else {
                        row.push("—".to_string());
                    }
                } else {
                    row.push("—".to_string());
                }
            }
            writeln!(md, "| {} | {} | {} | {} |", uni_label, row[0], row[1], row[2])?;
        }
        writeln!(md, "")?;

        // ── Verdict ────────────────────────────────────────────────────────
        writeln!(md, "## Verdict")?;
        writeln!(md, "")?;
        writeln!(md, "**Key question:** Does the BollingerReversion signal add value beyond the ATR×0.30 stop?")?;
        writeln!(md, "")?;

        // Compare POST_FIX vs RANDOM
        if let (Some(br), Some(rnd)) = (global_summary.iter().find(|(l,_,_,_,_,_,_)| *l == "POST_FIX"),
                                          global_summary.iter().find(|(l,_,_,_,_,_,_)| *l == "RANDOM")) {
            let signal_beats_random = br.2 > 0 && (br.1 as f64 / br.2 as f64) > (rnd.1 as f64 / rnd.2 as f64);
            writeln!(md, "- POST_FIX pass rate: {:.0}% vs RANDOM pass rate: {:.0}%", br.3, rnd.3)?;
            writeln!(md, "- POST_FIX avg Sharpe: {:.2} vs RANDOM avg Sharpe: {:.2}", br.5, rnd.5)?;
            writeln!(md, "- Signal beats random: {}", if signal_beats_random { "YES ✅" } else { "NO ❌" })?;
        }
        writeln!(md, "")?;
        writeln!(md, "**Generated:** {:.1}s", t0.elapsed().as_secs_f64())?;
    }

    eprintln!("\nCSV: {}", CSV_OUT);
    eprintln!("MD:  {}", MD_OUT);
    eprintln!("Runtime: {:?}", t0.elapsed());

    Ok(())
}

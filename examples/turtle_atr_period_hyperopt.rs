//! =========================================================
//! HYPERPARAMETER OPTIMIZATION: Turtle ATR Exit Period
//! =========================================================
//!
//! TARGET: Turtle ATR period — the ATR lookback used for the Turtle's
//!         ATR-trailing stop exit. This is DISTINCT from Chandelier ATR.
//!
//! Background:
//!   - Turtle entry period (EP=21) was optimized 2026-04-10 ✓
//!   - Chandelier ATR period (P=28) was optimized 2026-04-11 ✓
//!   - The ORIGINAL Turtle system uses a SEPARATE ATR(20) trailing stop
//!     as its primary exit mechanism. Our code conflates Chandelier and Turtle.
//!   - This sweep tests whether a Turtle-specific ATR period (10-60)
//!     improves over Chandelier(28) alone, or whether using BOTH exits
//!     (Chandelier OR Turtle ATR fires first) improves robustness.
//!
//! Design:
//!   - Fixed: EP=21, CHAND_PERIOD=28, CHAND_MULT=2.0, CAP=3, HM=45, TAKER=0.1%
//!   - Sweep: TURTLE_ATR_PERIOD ∈ {10, 15, 20, 25, 28, 30, 35, 40, 50, 60} (10 values)
//!   - Two modes tested:
//!     (A) CHAND_ONLY: Chandelier(28, 2.0) as sole exit
//!     (B) DUAL_EXIT: Chandelier(28,2.0) OR Turtle_ATR(N,2.0) — exit fires on whichever triggers first
//!   - Walk-forward: 252 train / 252 test across all 9 universes
//!   - Metrics: pass rate, avg Sharpe, avg return, worst DD, equity curves

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;

// Sweep values
const ATR_PERIODS: &[usize] = &[10, 15, 20, 25, 28, 30, 35, 40, 50, 60];

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

const CSV_OUT: &str = "snapshots/turtle_atr_period_sweep.csv";
const EQUITY_OUT: &str = "snapshots/turtle_atr_period_equity.csv";
const SUMMARY_MD: &str = "snapshots/turtle_atr_period_sweep_summary.md";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        curr_close > max_close
    } else {
        false
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_curve: Vec<f64>,
}

/// Run simulation with DUAL exit: Chandelier(28, 2.0) OR Turtle_ATR(N, 2.0)
/// Exit fires on whichever triggers first.
fn run_sim_dual(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    turtle_atr_period: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank by dollar volume
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, EP, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Dual ATR trailing stop
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));

                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            // Chandelier ATR(28, 2.0)
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                            // Turtle ATR(N, 2.0)
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, turtle_atr_period, b);
                            let trail_turtle = highest_high_turtle - CHAND_MULT * atr_turtle;

                            // Exit fires on EITHER stop
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

fn run_sim_chand_only(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, EP, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high = highest_high.max(sd.high[b]);
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail = highest_high - CHAND_MULT * atr_val;
                            if sd.close[b] < trail {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Turtle ATR Period Hyperopt: DUAL_EXIT sweep ====");
    eprintln!("Baseline: CHAND_ONLY = Chandelier(28, 2.0) alone");
    eprintln!("Sweep: TURTLE_ATR_PERIOD ∈ {:?}", ATR_PERIODS);
    eprintln!("Mode: DUAL_EXIT (Chandelier OR Turtle fires first)\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
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

    // ── Global CSV header ────────────────────────────────────────────────────
    let mut csv_lines = vec!["atr_period,mode,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    let mut equity_lines = vec!["atr_period,mode,universe,window,step,equity".to_string()];
    let mut summary: Vec<(usize, String, usize, usize, f64, f64, f64, f64, usize)> = Vec::new();

    // ── Baseline: CHAND_ONLY ──────────────────────────────────────────────────
    eprintln!("===== MODE: CHAND_ONLY (baseline) =====");
    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { eprintln!("{:>20} SKIPPED", label); continue; }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        let mut uni_pass = 0usize;
        let mut uni_total = 0usize;
        let mut uni_sharpe_sum = 0.0_f64;
        let mut uni_ret_sum = 0.0_f64;
        let mut uni_dd_max = 0.0_f64;
        let mut uni_trades = 0usize;

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            let r = run_sim_chand_only(&sym_data_map, &symbols, test_start, test_end);

            uni_pass += if r.pass { 1 } else { 0 };
            uni_total += 1;
            uni_sharpe_sum += r.sharpe;
            uni_ret_sum += r.ret;
            uni_dd_max = uni_dd_max.max(r.max_dd);
            uni_trades += r.trades;

            csv_lines.push(format!(
                "{},CHAND_ONLY,{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                CHAND_PERIOD, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
            ));

            // Sample equity curve (every 10 steps to keep file manageable)
            for (step, &eq) in r.equity_curve.iter().enumerate() {
                if step % 10 == 0 {
                    equity_lines.push(format!("{},CHAND_ONLY,{},{},{},{:.6}", CHAND_PERIOD, label, wi, step, eq));
                }
            }
        }

        let avg_sh = uni_sharpe_sum / uni_total.max(1) as f64;
        let avg_ret = uni_ret_sum / uni_total.max(1) as f64;
        eprintln!("  {:>18} | {}/{} pass | sh={:+.2} ret={:+.1}% DD={:.1}% {}tr",
            label, uni_pass, uni_total, avg_sh, avg_ret, uni_dd_max, uni_trades);
        summary.push((CHAND_PERIOD, "CHAND_ONLY".to_string(), uni_pass, uni_total, avg_sh, avg_ret, uni_dd_max, 0.0, uni_trades));
    }

    // ── Sweep: DUAL_EXIT mode ─────────────────────────────────────────────────
    for &turtle_period in ATR_PERIODS {
        if turtle_period == CHAND_PERIOD { continue; } // skip duplicate of baseline

        eprintln!("\n===== MODE: DUAL_EXIT | TURTLE_ATR={} =====", turtle_period);

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            let mut uni_pass = 0usize;
            let mut uni_total = 0usize;
            let mut uni_sharpe_sum = 0.0_f64;
            let mut uni_ret_sum = 0.0_f64;
            let mut uni_dd_max = 0.0_f64;
            let mut uni_trades = 0usize;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim_dual(&sym_data_map, &symbols, test_start, test_end, turtle_period);

                uni_pass += if r.pass { 1 } else { 0 };
                uni_total += 1;
                uni_sharpe_sum += r.sharpe;
                uni_ret_sum += r.ret;
                uni_dd_max = uni_dd_max.max(r.max_dd);
                uni_trades += r.trades;

                csv_lines.push(format!(
                    "{},DUAL_EXIT,{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    turtle_period, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                ));

                for (step, &eq) in r.equity_curve.iter().enumerate() {
                    if step % 10 == 0 {
                        equity_lines.push(format!("{},DUAL_EXIT,{},{},{},{:.6}", turtle_period, label, wi, step, eq));
                    }
                }
            }

            let avg_sh = uni_sharpe_sum / uni_total.max(1) as f64;
            let avg_ret = uni_ret_sum / uni_total.max(1) as f64;
            eprintln!("  {:>18} | {}/{} pass | sh={:+.2} ret={:+.1}% DD={:.1}% {}tr",
                label, uni_pass, uni_total, avg_sh, avg_ret, uni_dd_max, uni_trades);
            summary.push((turtle_period, "DUAL_EXIT".to_string(), uni_pass, uni_total, avg_sh, avg_ret, uni_dd_max, 0.0, uni_trades));
        }
    }

    // ── Write CSV ─────────────────────────────────────────────────────────────
    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    let mut ef = File::create(EQUITY_OUT)?;
    for line in &equity_lines { writeln!(ef, "{}", line)?; }

    // ── Compute global summaries ──────────────────────────────────────────────
    let mut global_summary: Vec<(usize, String, usize, usize, f64, f64, f64, usize)> = Vec::new();
    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { continue; }

        for &turtle_period in ATR_PERIODS {
            let mode = if turtle_period == CHAND_PERIOD { "CHAND_ONLY" } else { "DUAL_EXIT" };
            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

            let mut g_pass = 0usize;
            let mut g_total = 0usize;
            let mut g_sharpe_sum = 0.0_f64;
            let mut g_ret_sum = 0.0_f64;
            let mut g_dd_max = 0.0_f64;
            let mut g_trades = 0usize;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = if turtle_period == CHAND_PERIOD {
                    run_sim_chand_only(&sym_data_map, &symbols, test_start, test_end)
                } else {
                    run_sim_dual(&sym_data_map, &symbols, test_start, test_end, turtle_period)
                };

                g_pass += if r.pass { 1 } else { 0 };
                g_total += 1;
                g_sharpe_sum += r.sharpe;
                g_ret_sum += r.ret;
                g_dd_max = g_dd_max.max(r.max_dd);
                g_trades += r.trades;
            }

            global_summary.push((turtle_period, mode.to_string(), g_pass, g_total,
                g_sharpe_sum / g_total.max(1) as f64, g_ret_sum / g_total.max(1) as f64, g_dd_max, g_trades));
        }
    }

    // ── Ranking by avg Sharpe across all universes ─────────────────────────────
    global_summary.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

    eprintln!("\n==== GLOBAL RANKING (by avg Sharpe across universes) ====");
    for (period, mode, g_pass, g_total, avg_sh, avg_ret, worst_dd, g_trades) in &global_summary {
        let pass_pct = *g_pass as f64 / (*g_total as f64).max(1.0_f64) * 100.0;
        eprintln!("  ATR={:>3} {} | {}/{} pass ({:.0}%) | sh={:+.3} ret={:+.1}% DD={:.1}% {}tr",
            *period, *mode, *g_pass, *g_total, pass_pct, *avg_sh, *avg_ret, *worst_dd, *g_trades);
    }

    // ── Write summary markdown ────────────────────────────────────────────────
    let mut md = File::create(SUMMARY_MD)?;
    writeln!(md, "# Turtle ATR Period Hyperopt — DUAL_EXIT Sweep")?;
    writeln!(md, "")?;
    writeln!(md, "## Configuration")?;
    writeln!(md, "- Strategy: Turtle breakout + Chandelier(28,2.0) OR Turtle_ATR(N,2.0) exit")?;
    writeln!(md, "- Baseline: CHAND_ONLY = Chandelier(28, 2.0) alone")?;
    writeln!(md, "- Sweep: ATR_PERIODS = {:?}", ATR_PERIODS)?;
    writeln!(md, "- Fixed: EP=21, CHAND_PERIOD=28, CHAND_MULT=2.0, CAP=3, HM=45, FEE=0.1%")?;
    writeln!(md, "- Walk-forward: {} train / {} test, {} universes", TRAIN_BARS, TEST_BARS, UNIVERSES.len())?;
    writeln!(md, "")?;
    writeln!(md, "## Global Ranking (by avg Sharpe)")?;
    writeln!(md, "| Rank | ATR Period | Mode | Pass | Avg Sharpe | Avg Ret | Worst DD | Trades |")?;
    writeln!(md, "|------|------------|------|------|-------------|---------|-----------|--------|")?;

    for (i, (period, mode, g_pass, g_total, avg_sh, avg_ret, worst_dd, g_trades)) in global_summary.iter().enumerate() {
        let pass_pct = *g_pass as f64 / (*g_total as f64).max(1.0_f64) * 100.0;
        writeln!(md, "| {} | {} | {} | {}/{} ({:.0}%) | {:+.3} | {:+.1}% | {:.1}% | {} |",
            i+1, *period, *mode, *g_pass, *g_total, pass_pct, *avg_sh, *avg_ret, *worst_dd, *g_trades)?;
    }

    eprintln!("\n==== DONE in {:?} ====", t0.elapsed());
    eprintln!("CSV: {}", CSV_OUT);
    eprintln!("Equity: {}", EQUITY_OUT);
    eprintln!("Summary: {}", SUMMARY_MD);

    Ok(())
}

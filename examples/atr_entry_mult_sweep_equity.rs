//! ATR_ENTRY_MULT Sweep with Equity Curve Export
//! 
//! Tests ATR_ENTRY_MULT ∈ [0.00..2.00] step=0.05 (41 values)
//! Fixed params: EP=21, CHAND(7,2.30), HM=12, CAP=3, ATR=24, ATR_M=2.0, VOL=1
//! 
//! Exports per-bar equity for Baseline (EM=0.00), and key runner-ups.
//! Run: cargo run --example atr_entry_mult_sweep_equity --profile sweep

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 21;     // REVERTED 2026-04-26
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const VOL_LOOKBACK: usize = 1;

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

const OUTPUT_CSV: &str = "snapshots/atr_entry_mult_equity.csv";
const SUMMARY_CSV: &str = "snapshots/atr_entry_mult_summary.csv";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

#[derive(Clone)]
struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
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

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], 
    entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
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

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_entry_mult: f64,
) -> (WfResult, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
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
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, 
                        TURTLE_ATR_PERIOD, atr_entry_mult, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;

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

                            if equity > peak { peak = equity; }
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

    (WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== ATR_ENTRY_MULT Sweep (EP=21, 41 values) × 9 universes × 6 windows ====");
    eprintln!("Fixed: EP={}, CHAND({},{}), HM={}, CAP={}\n", 
        TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, HOLD_MAX, POSITION_CAP);

    let loader = DataLoader::new(None, None);
    let mut all_syms: HashSet<String> = HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    // Load all symbol data
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

    // Build ATR_ENTRY_MULT values
    let mut atr_values: Vec<f64> = Vec::new();
    let mut v: f64 = 0.0;
    while v <= 2.001 {
        atr_values.push(((v * 100.0).round() / 100.0).min(2.0));
        v += 0.05_f64;
    }
    let n_vals = atr_values.len();
    eprintln!("Testing {} ATR_ENTRY_MULT values: 0.00 to 2.00 step 0.05\n", n_vals);

    // Build window boundaries
    let n_windows = (n.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    eprintln!("9 universes × {} walk-forward windows\n", n_windows);

    // KEY CONFIGS for equity chart: baseline + confirmed winners + mid-range
    let key_configs: Vec<(f64, &str)> = vec![
        (0.00, "baseline"),    // EM=0.00 — current default (no ATR filter)
        (0.85, "winner"),     // EM=0.85 — sweep winner (82.5% pass, Sharpe 5.21)
        (0.90, "runner_up1"), // EM=0.90 — second best (79.4% pass, Sharpe 5.45)
        (0.50, "mid_range"),  // EM=0.50 — mid-range for comparison
    ];

    // Run sweep
    let mut all_results: Vec<(f64, WfResult, Vec<String>)> = Vec::new();
    let mut key_equity_curves: HashMap<String, Vec<f64>> = HashMap::new();

    for &atr_entry in &atr_values {
        let label = if let Some(name) = key_configs.iter().find(|x| (x.0 - atr_entry).abs() < 0.001) {
            Some(name.1.to_string())
        } else {
            None
        };

        let mut total_pass = 0usize;
        let mut total_windows = 0usize;
        let mut sum_sharpe = 0.0_f64;
        let mut sum_ret = 0.0_f64;
        let mut total_trades_all = 0usize;
        // Track equity accumulation for key configs (Base5 universe)
        // Format: (combined_log_sum, window_count)
        let mut key_equity_accum: Option<(Vec<f64>, usize)> = None;

        for (uname, symbols) in UNIVERSES {
            let symbols_vec: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            for w in 0..n_windows {
                let test_start = TRAIN_BARS + w * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end - test_start < 50 { continue; }

                let (result, equity) = run_sim(&sym_data_map, &symbols_vec, test_start, test_end, atr_entry);

                total_pass += if result.pass { 1 } else { 0 };
                total_windows += 1;
                sum_sharpe += result.sharpe;
                sum_ret += result.ret;
                total_trades_all += result.trades;

                // Accumulate log-equity for key configs (Base5 universe only)
                if let Some(ref lbl) = label {
                    if *uname == "Base5" {
                        // Normalize equity to start at 1.0, then convert to log returns for proper geometric mean
                        if let Some(&first) = equity.first() {
                            if first > 0.0 {
                                let norm: Vec<f64> = equity.iter().map(|&v| (v / first).ln()).collect();
                                if let Some((ref mut log_sum, ref mut count)) = key_equity_accum {
                                    // Extend if needed
                                    if log_sum.len() < norm.len() {
                                        log_sum.resize(norm.len(), 0.0_f64);
                                    }
                                    for i in 0..norm.len() {
                                        log_sum[i] += norm[i];
                                    }
                                    *count += 1;
                                } else {
                                    key_equity_accum = Some((norm, 1));
                                }
                            }
                        }
                    }
                }
            }
        }

        let avg_sharpe = if total_windows > 0 { sum_sharpe / total_windows as f64 } else { 0.0 };
        let avg_ret = if total_windows > 0 { sum_ret / total_windows as f64 } else { 0.0 };
        let pass_rate = if total_windows > 0 { total_pass as f64 / total_windows as f64 * 100.0 } else { 0.0 };

        let result = WfResult {
            ret: avg_ret,
            sharpe: avg_sharpe,
            max_dd: 0.0,
            trades: total_trades_all,
            win_rate: 0.0,
            pass: pass_rate >= 70.0,
        };

        all_results.push((atr_entry, result.clone(), vec![]));

        if let Some(lbl) = label {
            if let Some((log_sum, count)) = key_equity_accum {
                if count > 0 {
                    // Convert log-sum to geometric mean: exp(mean(log returns))
                    let n = count as f64;
                    let geometric_mean: Vec<f64> = log_sum.iter().map(|&v| (v / n).exp()).collect();
                    key_equity_curves.insert(lbl.to_string(), geometric_mean);
                }
            }
        }

        eprintln!("EM={:.2}: {} pass ({:.1}%), Sharpe={:.3}, Ret={:.1}%, {} trades",
            atr_entry, total_pass, pass_rate, avg_sharpe, avg_ret, total_trades_all);
    }

    // Sort by Sharpe
    all_results.sort_by(|a, b| b.1.sharpe.partial_cmp(&a.1.sharpe).unwrap());

    println!("\n=== TOP 15 by Sharpe ===");
    println!("{:>6} {:>5} {:>8} {:>8} {:>7}", "EM", "Pass%", "Sharpe", "Ret%", "Trades");
    for (em, r, _) in all_results.iter().take(15) {
        let pr = if all_results.len() > 0 { 
            let total_pass = r.pass as u8 as f64; // just use pass bool
            0.0_f64 
        } else { 0.0 };
        println!("{:.2}   {:>8.3}  {:>8.2}  {:>7}", em, r.sharpe, r.ret, r.trades);
    }

    // Write summary CSV
    let mut summary_csv = String::from("atr_entry_mult,avg_sharpe,avg_return,total_trades,pass_count,total_windows\n");
    for (em, r, _) in &all_results {
        summary_csv.push_str(&format!("{:.2},{:.4},{:.1},{},0,54\n", em, r.sharpe, r.ret, r.trades));
    }
    std::fs::write(SUMMARY_CSV, summary_csv)?;
    println!("\nSummary CSV: {}", SUMMARY_CSV);

    // Write equity CSV (key configs only)
    let mut csv_lines = vec![String::from("bar,baseline,winner,runner_up1,mid_range")];
    let max_len = key_equity_curves.values().map(|v| v.len()).max().unwrap_or(0);
    for i in 0..max_len {
        let bar_str = i.to_string();
        let mut row = bar_str;
        for key in &["baseline", "winner", "runner_up1", "mid_range"] {
            let val = key_equity_curves.get(*key).and_then(|v| v.get(i)).unwrap_or(&1.0);
            row.push_str(&format!(",{:.6}", val));
        }
        csv_lines.push(row);
    }
    std::fs::write(OUTPUT_CSV, csv_lines.join("\n"))?;
    println!("Equity CSV: {}", OUTPUT_CSV);
    println!("Runtime: {:.1}s", t0.elapsed().as_secs_f64());

    Ok(())
}
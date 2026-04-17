//! Turtle VOL_LOOKBACK Extended Hyperopt: EMA vs SMA, Range 1-100
//!
//! Tests EMA smoothing (unexplored) vs SMA (tested 1-60 in vol_lookback_dense.rs).
//! Uses CURRENT frozen params: CHAND(20, 2.15), ATR(24, 2.0)
//! Phase 1: NoDOGE 6-window sweep (VL=1-100, both methods)
//! Phase 2: 9-universe validation of winner + runner-ups

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
// Current frozen params (updated after hyperopt-2026-04-16)
const CHAND_PERIOD: usize = 20;
const CHAND_MULT: f64 = 2.15;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

// Production universe for primary sweep
const NODOGE_UNIVERSE: (&str, &[&str]) = (
    "NoDOGE",
    &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"],
);

// 9-universe for validation (using slice-of-slices for const compat)
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

const CSV_OUT: &str = "snapshots/vol_ema_extended_hyperopt.csv";
const MD_OUT: &str = "snapshots/vol_ema_extended_hyperopt.md";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

/// Rolling SMA of price/volume series at index idx
fn rolling_avg_sma(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

/// Rolling EMA at index idx (alpha = 2/(span+1))
fn rolling_avg_ema(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx == 0 || vals.is_empty() { return *vals.get(idx).unwrap_or(&0.0); }
    if idx < window {
        return rolling_avg_sma(vals, idx + 1, idx);
    }
    let alpha = 2.0 / (window as f64 + 1.0);
    let one_minus_alpha = 1.0 - alpha;
    let mut ema = rolling_avg_sma(vals, window, window - 1);
    for i in window..=idx {
        ema = alpha * vals[i] + one_minus_alpha * ema;
    }
    ema
}

/// Dollar volume score at bar using specified method
fn dv_score(sd: &SymData, bar: usize, window: usize, use_ema: bool) -> f64 {
    if bar >= sd.close.len() { return 0.0; }
    let vol_val = if use_ema {
        rolling_avg_ema(&sd.vol, window, bar)
    } else {
        rolling_avg_sma(&sd.vol, window, bar)
    };
    let price = sd.close[bar];
    let dv = vol_val * price;
    if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = *high.get(i).unwrap_or(&0.0);
        let l = *low.get(i).unwrap_or(&0.0);
        let c0 = *close.get(i.saturating_sub(1)).unwrap_or(&0.0);
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
    close.get(idx).map(|&c| c > max_close).unwrap_or(false)
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from_equity(equity_curve: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity_curve {
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

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    vol_lookback: usize,
    use_ema: bool,
) -> WfResult {
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
                let dv = dv_score(sd, bar, vol_lookback, use_ema);
                scores.push((sym.as_str(), dv));
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
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;
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
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }
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
    let max_dd = max_dd_from_equity(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

/// Aggregate stats per (vl, use_ema): (sharpe_sum, pass_count, total_count)
type VlStats = (f64, usize, usize);

fn run_nodoge_sweep(sym_data: &HashMap<String, SymData>) -> HashMap<(usize, bool), VlStats> {
    let symbols: Vec<String> = NODOGE_UNIVERSE.1.iter().map(|s| s.to_string()).collect();
    let n = sym_data.get("BTCUSDT").map(|sd| sd.close.len()).unwrap_or(0);
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

    let mut stats: HashMap<(usize, bool), VlStats> = HashMap::new();
    for vl in 1..=100 {
        stats.insert((vl, false), (0.0, 0, 0));
        stats.insert((vl, true),  (0.0, 0, 0));
    }

    for wi in 0..total_windows {
        let test_start = TRAIN_BARS + wi * TEST_BARS;
        let test_end = (test_start + TEST_BARS).min(n);
        if test_end - test_start < 5 { continue; }

        for vl in 1..=100 {
            let r_sma = run_sim(sym_data, &symbols, test_start, test_end, vl, false);
            let (ref mut sum_s, ref mut pass_s, ref mut total_s) = stats.get_mut(&(vl, false)).unwrap();
            *sum_s += r_sma.sharpe;
            *pass_s += if r_sma.pass { 1 } else { 0 };
            *total_s += 1;

            let r_ema = run_sim(sym_data, &symbols, test_start, test_end, vl, true);
            let (ref mut sum_e, ref mut pass_e, ref mut total_e) = stats.get_mut(&(vl, true)).unwrap();
            *sum_e += r_ema.sharpe;
            *pass_e += if r_ema.pass { 1 } else { 0 };
            *total_e += 1;
        }
    }
    stats
}

fn print_phase1_results(stats: &HashMap<(usize, bool), VlStats>) {
    eprintln!("\n{:>4} {:>4} | {:>7} | {:>8}", "VL", "Method", "PassRate", "AvgSharpe");
    eprintln!("{}", "-".repeat(30));
    for vl in (1..=100).step_by(5) {
        for &ema in &[false, true] {
            let (sum_s, pass_c, total_c) = stats.get(&(vl, ema)).unwrap();
            let avg_sh = if *total_c > 0 { *sum_s / *total_c as f64 } else { 0.0 };
            let pr = if *total_c > 0 { *pass_c as f64 / *total_c as f64 * 100.0 } else { 0.0 };
            eprint!("{:>4} {:>4} | {:>6.1}% | {:>8.4}", vl, if ema { "EMA" } else { "SMA" }, pr, avg_sh);
        }
        eprintln!();
    }
}

fn find_top_configs(stats: &HashMap<(usize, bool), VlStats>) -> Vec<(usize, bool, f64)> {
    let mut all: Vec<(usize, bool, f64)> = Vec::new();
    for vl in 1..=100 {
        for &ema in &[false, true] {
            let (sum_s, pass_c, total_c) = stats.get(&(vl, ema)).unwrap();
            let avg_sh = if *total_c > 0 { *sum_s / *total_c as f64 } else { 0.0 };
            all.push((vl, ema, avg_sh));
        }
    }
    all.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    all
}

fn run_9way_validation(
    sym_data_map: &HashMap<String, SymData>,
    configs: &[(usize, bool)],
    n: usize,
) -> Vec<(String, usize, bool, usize, usize, f64, f64)> {
    // returns: (universe, vl, ema, pass_count, total_count, avg_sharpe, avg_ret)
    let mut results = Vec::new();

    for &(label, syms) in UNIVERSES {
        let symbols: Vec<String> = syms.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        for &(vl, ema) in configs {
            let mut total_sharpe = 0.0_f64;
            let mut total_pass = 0usize;
            let mut total_count = 0usize;
            let mut total_ret = 0.0_f64;

            for wi in 0..total_windows {
                let test_start = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end - test_start < 5 { continue; }
                let r = run_sim(sym_data_map, &symbols, test_start, test_end, vl, ema);
                total_sharpe += r.sharpe;
                total_pass += if r.pass { 1 } else { 0 };
                total_count += 1;
                total_ret += r.ret;
            }

            let avg_sh = if total_count > 0 { total_sharpe / total_count as f64 } else { 0.0 };
            let avg_ret = if total_count > 0 { total_ret / total_count as f64 } else { 0.0 };
            eprintln!("{:>20} VL={:>3} {:>3} | {}/{} ({:>5.1}%) | sh={:>6.3} | ret={:>+7.1}%",
                label, vl, if ema { "EMA" } else { "SMA" },
                total_pass, total_count,
                if total_count > 0 { total_pass as f64 / total_count as f64 * 100.0 } else { 0.0 },
                avg_sh, avg_ret);
            results.push((label.to_string(), vl, ema, total_pass, total_count, avg_sh, avg_ret));
        }
    }
    results
}

fn run_full_equity(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    n: usize,
    vl: usize,
    use_ema: bool,
) -> (WfResult, usize) {
    // Run all windows sequentially and concatenate equity curves
    let mut all_equity = vec![1.0_f64];
    let mut total_sharpe_sum = 0.0_f64;
    let mut total_pass = 0usize;
    let mut total_count = 0usize;
    let mut total_ret = 0.0_f64;

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    for wi in 0..total_windows {
        let test_start = TRAIN_BARS + wi * TEST_BARS;
        let test_end = (test_start + TEST_BARS).min(n);
        if test_end - test_start < 5 { continue; }
        let r = run_sim(sym_data, symbols, test_start, test_end, vl, use_ema);
        total_sharpe_sum += r.sharpe;
        total_pass += if r.pass { 1 } else { 0 };
        total_count += 1;
        total_ret += r.ret;
        // Concatenate equity (skip first 1.0 to avoid duplicate peak)
        for eq in r.equity_curve.into_iter().skip(1) {
            all_equity.push(eq);
        }
    }

    let avg_sh = if total_count > 0 { total_sharpe_sum / total_count as f64 } else { 0.0 };
    let avg_ret = if total_count > 0 { total_ret / total_count as f64 } else { 0.0 };
    let win_rate = 0.0; // not meaningful for concatenated
    let max_dd = max_dd_from_equity(&all_equity);
    let ret = (all_equity.last().unwrap_or(&1.0) - 1.0) * 100.0;

    (WfResult { ret, sharpe: avg_sh, max_dd, trades: 0, win_rate, pass: total_pass == total_count, equity_curve: all_equity }, total_count)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    // ── Load data ──
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for &(_, syms) in UNIVERSES { for s in syms { all_syms.insert(s.to_string()); } }
    for &s in NODOGE_UNIVERSE.1 { all_syms.insert(s.to_string()); }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
        }
    }
    let n = min_len.min(2800);

    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = n.min(df.height());
            sym_data_map.insert(sym.clone(), SymData {
                close: df.column("close")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect(),
                high:  df.column("high")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect(),
                low:   df.column("low")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect(),
                vol:   df.column("volume")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect(),
            });
        }
    }
    eprintln!("Loaded {} syms, {} bars\n", sym_data_map.len(), n);

    // ── Phase 1: NoDOGE dense sweep (VL=1-100, SMA+EMA) ──
    eprintln!("=== PHASE 1: NoDOGE 6-window sweep ===");
    let stats = run_nodoge_sweep(&sym_data_map);
    print_phase1_results(&stats);

    // Find top-3 configs
    let all_configs = find_top_configs(&stats);
    let top3: Vec<(usize, bool)> = all_configs.iter().take(3).map(|&(vl, ema, _)| (vl, ema)).collect();

    eprintln!("\nTOP-3 CONFIGS (NoDOGE avg Sharpe):");
    for (i, &(vl, ema)) in top3.iter().enumerate() {
        let (_, pass_c, total_c) = stats.get(&(vl, ema)).unwrap();
        let avg_sh = if *total_c > 0 { stats.get(&(vl, ema)).unwrap().0 / *total_c as f64 } else { 0.0 };
        eprintln!("  #{}= VL={}, {} (avg Sharpe={:.4})", i+1, vl, if ema { "EMA" } else { "SMA" }, avg_sh);
    }

    // ── Phase 2: 9-universe validation of top configs ──
    eprintln!("\n=== PHASE 2: 9-universe validation ===");
    let val_results = run_9way_validation(&sym_data_map, &top3, n);

    // Aggregate 9-way stats per config
    let mut cfg9way_sums: HashMap<(usize, bool), (f64, usize, usize, f64)> = HashMap::new();
    // (sharpe_sum, pass_sum, total_sum, ret_sum)
    for (label, vl, ema, pass_c, total_c, avg_sh, avg_ret) in &val_results {
        let key = (*vl, *ema);
        let entry = cfg9way_sums.entry(key).or_insert((0.0_f64, 0_usize, 0_usize, 0.0_f64));
        entry.0 += avg_sh;
        entry.1 += *pass_c;
        entry.2 += *total_c;
        entry.3 += avg_ret;
        let _ = label;
    }

    eprintln!("\n9-WAY SUMMARY:");
    let mut summary: Vec<((usize, bool), f64, f64, usize, usize)> = Vec::new();
    for (&(vl, ema), (sh_sum, pass_sum, total_sum, ret_sum)) in &cfg9way_sums {
        let n_uni = 9;
        let avg_sh_all = *sh_sum / n_uni as f64;
        let avg_pr = *pass_sum as f64 / *total_sum as f64 * 100.0;
        let avg_ret_all = *ret_sum / n_uni as f64;
        summary.push(((vl, ema), avg_sh_all, avg_ret_all, *pass_sum, *total_sum));
        eprintln!("  VL={} {} | 9-way avg Sharpe={:.4} | avg Ret={:+.1}% | pass {}/{} ({:.0}%)",
            vl, if ema { "EMA" } else { "SMA" }, avg_sh_all, avg_ret_all, *pass_sum, *total_sum, avg_pr);
    }
    summary.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Winner is the one with highest 9-way avg Sharpe (most robust across universes)
    let (best_vl, best_ema) = summary.first().map(|x| x.0).unwrap_or((1, false));
    let (_, best_sh_9w, best_ret_9w, best_pass_sum, best_total_sum) = summary.first().unwrap();
    eprintln!("\nWINNER (most robust across 9 universes): VL={}, {} (9-way Sharpe={:.4}, Ret={:+.1}%, pass {}/{})",
        best_vl, if best_ema { "EMA" } else { "SMA" }, best_sh_9w, best_ret_9w, best_pass_sum, best_total_sum);

    // ── Export equity curves for charting ──
    let nodoge_syms: Vec<String> = NODOGE_UNIVERSE.1.iter().map(|s| s.to_string()).collect();

    // Baseline: VL=1, SMA (original hardcoded)
    let (baseline_eq, _) = run_full_equity(&sym_data_map, &nodoge_syms, n, 1, false);
    {
        let mut f = File::create(CSV_OUT.replace(".csv", "_baseline.csv"))?;
        writeln!(f, "day,equity")?;
        for (i, &eq) in baseline_eq.equity_curve.iter().enumerate() {
            writeln!(f, "{},{}", i, eq)?;
        }
    }

    // Winner equity
    let (winner_eq, _) = run_full_equity(&sym_data_map, &nodoge_syms, n, best_vl, best_ema);
    {
        let f = File::create(CSV_OUT.replace(".csv", "_winner.csv"))?;
        let mut f = f;
        writeln!(f, "day,equity")?;
        for (i, &eq) in winner_eq.equity_curve.iter().enumerate() {
            writeln!(f, "{},{}", i, eq)?;
        }
    }

    // Runner-up 1
    let ru1 @ (ru1_vl, ru1_ema) = summary.get(1).map(|x| x.0).unwrap_or((2, false));
    let (ru1_eq, _) = run_full_equity(&sym_data_map, &nodoge_syms, n, ru1_vl, ru1_ema);
    {
        let f = File::create(CSV_OUT.replace(".csv", "_runnerup1.csv"))?;
        let mut f = f;
        writeln!(f, "day,equity")?;
        for (i, &eq) in ru1_eq.equity_curve.iter().enumerate() {
            writeln!(f, "{},{}", i, eq)?;
        }
    }

    // Runner-up 2
    let ru2 @ (ru2_vl, ru2_ema) = summary.get(2).map(|x| x.0).unwrap_or((3, false));
    let (ru2_eq, _) = run_full_equity(&sym_data_map, &nodoge_syms, n, ru2_vl, ru2_ema);
    {
        let f = File::create(CSV_OUT.replace(".csv", "_runnerup2.csv"))?;
        let mut f = f;
        writeln!(f, "day,equity")?;
        for (i, &eq) in ru2_eq.equity_curve.iter().enumerate() {
            writeln!(f, "{},{}", i, eq)?;
        }
    }

    eprintln!("\nEquity curves exported.");
    eprintln!("Baseline (VL=1 SMA): {} days, final equity={:.2}x", baseline_eq.equity_curve.len(), baseline_eq.equity_curve.last().unwrap_or(&1.0));
    eprintln!("Winner (VL={} {}): {} days, final equity={:.2}x", best_vl, if best_ema { "EMA" } else { "SMA" }, winner_eq.equity_curve.len(), winner_eq.equity_curve.last().unwrap_or(&1.0));
    eprintln!("Runnerup1 (VL={} {}): {} days, final equity={:.2}x", ru1_vl, if ru1_ema { "EMA" } else { "SMA" }, ru1_eq.equity_curve.len(), ru1_eq.equity_curve.last().unwrap_or(&1.0));
    eprintln!("Runnerup2 (VL={} {}): {} days, final equity={:.2}x", ru2_vl, if ru2_ema { "EMA" } else { "SMA" }, ru2_eq.equity_curve.len(), ru2_eq.equity_curve.last().unwrap_or(&1.0));

    // ── Write CSV summary ──
    {
        let mut f = File::create(CSV_OUT)?;
        writeln!(f, "vl,method,avg_sharpe_nodoge,pass_rate_nodoge,total_windows,9way_avg_sharpe,9way_avg_ret,9way_pass_sum,9way_total_sum")?;
        for &((vl, ema), avg_sh_9w, avg_ret_9w, pass_sum, total_sum) in &summary {
            let (sh_sum, pass_c, total_c) = stats.get(&(vl, ema)).unwrap();
            let avg_sh_nodoge = if *total_c > 0 { *sh_sum / *total_c as f64 } else { 0.0 };
            let pr_nodoge = if *total_c > 0 { *pass_c as f64 / *total_c as f64 * 100.0 } else { 0.0 };
            writeln!(f, "{},{},{:.4},{:.1},{},{:.4},{:.1},{},{}", vl, if ema { "EMA" } else { "SMA" }, avg_sh_nodoge, pr_nodoge, total_c, avg_sh_9w, avg_ret_9w, pass_sum, total_sum)?;
        }
    }

    // ── Write MD report ──
    {
        let mut f = File::create(MD_OUT)?;
        writeln!(f, "# VOL_LOOKBACK Extended Hyperopt (EMA vs SMA, 1-100)")?;
        writeln!(f, "")?;
        writeln!(f, "**Date:** 2026-04-17")?;
        writeln!(f, "**Strategy:** Turtle+Chandelier (frozen: EP=21, CHAND=20, ATR=24, CAP=3, HM=45)")?;
        writeln!(f, "**Phase 1:** NoDOGE 6-window sweep, VL=1-100, both SMA and EMA (200 configs)")?;
        writeln!(f, "**Phase 2:** 9-universe validation of top-3 configs")?;
        writeln!(f, "")?;
        writeln!(f, "## Winner (Most Robust Across 9 Universes)")?;
        writeln!(f, "")?;
        writeln!(f, "- **VL={}, {}** — 9-way avg Sharpe={:.4}, avg Ret={:+.1}%, pass {}/{}",
            best_vl, if best_ema { "EMA" } else { "SMA" }, best_sh_9w, best_ret_9w, best_pass_sum, best_total_sum)?;
        writeln!(f, "- **Baseline VL=1 SMA** — NoDOGE avg Sharpe={:.4}",
            if let (s, p, t) = stats.get(&(1, false)).unwrap() { if *t > 0 { *s / *t as f64 } else { 0.0 } } else { 0.0 })?;
        writeln!(f, "## Phase 2: 9-Universe Validation")?;
        writeln!(f, "## Phase 2: 9-Universe Validation")?;
        writeln!(f, "")?;
        writeln!(f, "| Universe | VL | Method | Pass | Avg Sharpe | Avg Ret |")?;
        writeln!(f, "|---|---|---|---|---|---|")?;
        for (lbl, vl, ema, ps, tc, sh, rt) in &val_results {
            writeln!(f, "| {} | {} | {} | {}/{} | {:.4} | {:+.1}% |", lbl, vl, if *ema { "EMA" } else { "SMA" }, ps, tc, sh, rt)?;
        }
        writeln!(f, "")?;
        writeln!(f, "## Charts")?;
        writeln!(f, "- `charts/vol_ema_comparison.png` — equity curve comparison")?;
    }

    eprintln!("\nRuntime: {:?}", t0.elapsed());
    eprintln!("CSV: {}", CSV_OUT);
    eprintln!("MD: {}", MD_OUT);
    Ok(())
}

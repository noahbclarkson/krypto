//! REGIME_ATR Hyperopt — Turtle-Only ATR Rank Filter
//!
//! PURPOSE: Find optimal REGIME_ATR_PERIOD and REGIME_LOOKBACK for the BTC ATR rank
//! conditional entry filter. The prior sweep (turtle_only_atr_rank_sweep.rs) used
//! hardcoded REGIME_ATR_PERIOD=21, REGIME_LOOKBACK=252 with no validation.
//!
//! This harness sweeps:
//!   REGIME_ATR_PERIOD: [5..=60 step 1]  (56 values)
//!   REGIME_LOOKBACK:   [21, 42, 63, 126, 252, 504]  (6 values)
//!   ATR_RANK_THRESHOLD: [0, 5, 10, 15, 20, 25, 30, 40]  (8 values)
//!
//! Strategy: Turtle breakout (EP=21, TurtleATR(24,2.0), HM=12, CAP=3)
//! Exit: Turtle ATR ONLY (matching live bot)
//! Fee: correct entry*(1+fee), exit*(1-fee)
//!
//! Production params: EP=21, TURTLE_ATR(24,2.0), HM=12, CAP=3, VL=8

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Instant;
use tokio::main;

const CANDLES: u32 = 3000;

// Production params (Turtle-only, matching live bot)
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 12;
const ATR_ENTRY_MULT: f64 = 0.00;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 8;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

// Regime filter thresholds to sweep (including baseline T=0)
const THRESHOLDS: &[u32] = &[0, 5, 10, 15, 20, 25, 30, 40];

// REGIME_ATR_PERIOD sweep [5..=60 step 1]
const ATR_PERIODS: &[usize] = &[
    5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35,
    36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50,
    51, 52, 53, 54, 55, 56, 57, 58, 59, 60,
];

// REGIME_LOOKBACK sweep
const LOOKBACKS: &[usize] = &[21, 42, 63, 126, 252, 504];

// 9 standard universes
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

const CSV_OUT: &str = "snapshots/regime_atr_hyperopt.csv";
const SUMMARY_OUT: &str = "snapshots/regime_atr_hyperopt_summary.csv";
const EQUITY_DIR: &str = "snapshots/regime_atr_equity/";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(h: &[f64], l: &[f64], c: &[f64], p: usize, idx: usize) -> f64 {
    if idx < p { return 0.0; }
    let mut trs = Vec::with_capacity(p);
    for i in (idx + 1 - p)..=idx {
        let hi = *h.get(i).unwrap_or(&0.0);
        let lo = *l.get(i).unwrap_or(&0.0);
        let c0 = *c.get(i.saturating_sub(1)).unwrap_or(&0.0);
        trs.push((hi - lo).max((hi - c0).abs()).max((lo - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / p as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(c: &[f64], h: &[f64], _l: &[f64], ep: usize, ap: usize, am: f64, idx: usize) -> bool {
    if idx < ep + 1 { return false; }
    let start = idx + 1 - ep;
    let mut mx = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&cv) = c.get(i) { mx = mx.max(cv); }
    }
    if let Some(&curr) = c.get(idx) {
        let breakout = curr > mx;
        if breakout && am > 0.0 {
            let atr_val = atr_at(h, &[], c, ap, idx);
            return curr >= mx + am * atr_val;
        }
        breakout
    } else { false }
}

fn turtle_exit(low: f64, entry: f64, atr: f64, mult: f64, worst_low: f64, bars: usize, hold_max: usize) -> bool {
    if bars >= hold_max { return true; }
    let stop = worst_low - mult * atr;
    low < stop
}

/// BTC ATR percentile at index idx (0..=100).
/// Uses regime_atr_period and regime_lookback for the percentile calculation.
fn btc_atr_pct(btc: &SymData, atr_p: usize, lookback: usize, idx: usize) -> f64 {
    if idx < atr_p.max(lookback) + 1 { return 50.0; }
    let curr_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_p, idx);
    let curr_close = *btc.close.get(idx).unwrap_or(&1.0);
    if curr_close <= 0.0 || curr_atr <= 0.0 { return 50.0; }
    let curr_pct = curr_atr / curr_close;
    let start = idx.saturating_sub(lookback);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        if let Some(&c) = btc.close.get(i) {
            if c > 0.0 {
                let hist_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_p, i);
                if hist_atr / c < curr_pct { below += 1; }
                total += 1;
            }
        }
    }
    if total == 0 { return 50.0; }
    (below as f64 / total as f64) * 100.0
}

fn annualised_sharpe(rets: &[f64]) -> f64 {
    if rets.len() < 2 { return 0.0; }
    let mn: f64 = rets.iter().sum::<f64>() / rets.len() as f64;
    let sd = (rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / rets.len() as f64).sqrt();
    if sd <= 1e-12 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct SimResult {
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
    btc: &SymData,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_p: usize,
    lookback: usize,
    threshold: u32,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut equity_curve = vec![1.0_f64];

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Regime filter gate
        let regime_ok = threshold == 0
            || btc_atr_pct(btc, atr_p, lookback, bar) >= threshold as f64;

        // Dollar-volume ranking
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = *sd.close.get(bar).unwrap_or(&0.0);
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

        if !regime_ok || equity <= 0.0 {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Check each top symbol for entry
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len().saturating_sub(1) { continue; }
                if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                    // Entry: buy at close, pay fee
                    let entry_px = *sd.close.get(bar).unwrap_or(&0.0);
                    if entry_px <= 0.0 { continue; }
                    let filled = entry_px * (1.0 + TAKER_FEE);

                    // Simulate hold
                    let mut held = 0;
                    let mut worst_low = *sd.low.get(bar).unwrap_or(&entry_px);
                    let mut atr_val = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar);
                    let mut exit_px = entry_px;
                    let mut exited = false;

                    for t in (bar + 1)..test_end {
                        held += 1;
                        if let Some(&hi) = sd.high.get(t) { worst_low = worst_low.min(*sd.low.get(t).unwrap_or(&hi)); }
                        atr_val = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, t);
                        let curr_low = *sd.low.get(t).unwrap_or(&entry_px);
                        let curr_close = *sd.close.get(t).unwrap_or(&entry_px);
                        if turtle_exit(curr_low, filled, atr_val, TURTLE_ATR_MULT, worst_low, held, HOLD_MAX) {
                            exit_px = curr_close;
                            exited = true;
                            bar = t; // advance bar cursor to avoid double-entry
                            break;
                        }
                    }

                    if !exited {
                        if let Some(&cp) = sd.close.get(test_end.saturating_sub(1)) {
                            exit_px = cp;
                            bar = test_end - 1;
                        }
                    }

                    let gross_ret = (exit_px - filled) / filled;
                    let net_ret = gross_ret - (TAKER_FEE * 2.0);
                    equity *= 1.0 + net_ret;
                    total_trades += 1;
                    if gross_ret > 0.0 { wins += 1; }
                    entered = true;
                    break; // one entry per bar per universe
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 } else { 0.0 };
    // Compute Sharpe from actual equity curve daily returns
    let mut daily_rets_for_sharpe = Vec::new();
    for i in 1..equity_curve.len() {
        if equity_curve[i-1] > 0.0 && equity_curve[i] > 0.0 {
            daily_rets_for_sharpe.push((equity_curve[i] - equity_curve[i-1]) / equity_curve[i-1]);
        }
    }
    SimResult { ret, sharpe: annualised_sharpe(&daily_rets_for_sharpe), max_dd: max_dd(&equity_curve), trades: total_trades, win_rate, pass, equity_curve }
}

fn run_wf(
    sym_data: &HashMap<String, SymData>,
    btc: &SymData,
    symbols: &[String],
    atr_p: usize,
    lookback: usize,
    threshold: u32,
    name: &str,
) -> (usize, usize, f64, f64, f64, usize, f64, Vec<f64>) {
    let train = 252;
    let test = 252;

    let mut all_results: Vec<SimResult> = Vec::new();
    let min_len = symbols.iter().filter_map(|s| sym_data.get(s).map(|sd| sd.close.len())).min().unwrap_or(0);
    let max_start = min_len.saturating_sub(test + train + 60);

    let mut window_starts: Vec<usize> = Vec::new();
    let mut s = 0;
    while s + train + test <= max_start {
        window_starts.push(s);
        s += test;
    }

    let mut total_pass = 0usize;
    for &start in &window_starts {
        let result = run_sim(sym_data, btc, symbols, start + train, start + train + test, atr_p, lookback, threshold);
        if result.pass { total_pass += 1; }
        all_results.push(result);
    }

    let n = all_results.len();
    let passes = all_results.iter().filter(|r| r.pass).count();
    let sum_ret = all_results.iter().map(|r| r.ret).sum::<f64>() / n.max(1) as f64;
    let sum_sharpe = all_results.iter().map(|r| r.sharpe).sum::<f64>() / n.max(1) as f64;
    let sum_dd = all_results.iter().map(|r| r.max_dd).sum::<f64>() / n.max(1) as f64;
    let sum_trades = all_results.iter().map(|r| r.trades).sum::<usize>() / n.max(1);
    let win_rate = all_results.iter().map(|r| r.win_rate).sum::<f64>() / n.max(1) as f64;
    let pass_pct = passes as f64 / n.max(1) as f64 * 100.0;

    eprintln!(
        "  AP={:>3} LB={:>3} T={:>3} | {:<16} | {}/{} ({}%) | Sharpe {:.3} | Ret {:.1}% | {} trades",
        atr_p, lookback, threshold, name, passes, n, pass_pct as i32, sum_sharpe, sum_ret, sum_trades
    );

    (passes, n, sum_sharpe, sum_ret, sum_dd, sum_trades, win_rate, all_results.first().map(|r| r.equity_curve.clone()).unwrap_or_default())
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    // Create equity output directory
    std::fs::create_dir_all(EQUITY_DIR)?;

    let loader = DataLoader::new(None, None);
    let mut all_syms_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms_set.insert(s.to_string()); }
    }

    // Load data
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms_set.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let h = df.height();
                min_len = min_len.min(h);
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARN: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in all_syms_set.iter() {
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

    // BTC data for regime filter
    let btc_df = loader.fetch_with_cache("BTCUSDT", "1d", CANDLES).await?;
    let btc_n = btc_df.height().min(n);
    macro_rules! btc_col {
        ($name:expr) => {{
            let chunked = btc_df.column($name)?.f64()?;
            chunked.into_iter().filter_map(|x| x).take(btc_n).collect::<Vec<_>>()
        }};
    }
    let btc = SymData {
        close: btc_col!("close"),
        high:  btc_col!("high"),
        low:   btc_col!("low"),
        vol:   btc_col!("volume"),
    };

    eprintln!("==== REGIME_ATR Hyperopt ====");
    eprintln!("ATR_PERIODS: {} values [5..=60 step 1]", ATR_PERIODS.len());
    eprintln!("LOOKBACKS: {:?} ({} values)", LOOKBACKS, LOOKBACKS.len());
    eprintln!("THRESHOLDS: {:?} ({} values)", THRESHOLDS, THRESHOLDS.len());
    eprintln!("Strategy: Turtle-only exit (matching live bot)\n");

    // Results storage: (atr_p, lookback, threshold, pass, total, sharpe, ret, dd, trades, win_rate)
    let mut all_results: Vec<String> = vec![
        "atr_period,lookback,threshold,pass,total,pass_pct,sharpe,ret_pct,dd_pct,trades,win_rate".to_string()
    ];

    // Best config tracking
    let mut best_sharpe = f64::NEG_INFINITY;
    let mut best_config = String::new();

    for &atr_p in ATR_PERIODS {
        for &lb in LOOKBACKS {
            for &t in THRESHOLDS {
                let mut g_pass = 0usize;
                let mut g_total = 0usize;
                let mut g_sharpe = 0.0_f64;
                let mut g_ret = 0.0_f64;
                let mut g_dd = 0.0_f64;
                let mut g_trades = 0usize;
                let mut g_win_rate = 0.0_f64;
                let mut count = 0usize;
                let mut sample_equity: Option<Vec<f64>> = None;

                for &(uname, symbols) in UNIVERSES {
                    let symbols_v: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
                    let all_loaded = symbols_v.iter().all(|s| sym_data_map.contains_key(s));
                    if !all_loaded { continue; }

                    let (pass, total, sharpe, ret, dd, trades, win_rate, equity) =
                        run_wf(&sym_data_map, &btc, &symbols_v, atr_p, lb, t, uname);

                    g_pass += pass;
                    g_total += total;
                    g_sharpe += sharpe;
                    g_ret += ret;
                    g_dd += dd;
                    g_trades += trades;
                    g_win_rate += win_rate;
                    count += 1;

                    if sample_equity.is_none() {
                        sample_equity = Some(equity);
                    }
                }

                if count > 0 {
                    let avg_sharpe = g_sharpe / count as f64;
                    let avg_ret = g_ret / count as f64;
                    let avg_dd = g_dd / count as f64;
                    let avg_trades = g_trades / count;
                    let avg_win_rate = g_win_rate / count as f64;
                    let pass_pct = g_pass as f64 / g_total.max(1) as f64 * 100.0;

                    all_results.push(format!("{},{},{},{},{},{:.1},{:.3},{:.1},{:.1},{},{:.3}",
                        atr_p, lb, t, g_pass, g_total, pass_pct, avg_sharpe, avg_ret, avg_dd, avg_trades, avg_win_rate
                    ));

                    // Track best
                    if avg_sharpe > best_sharpe {
                        best_sharpe = avg_sharpe;
                        best_config = format!("AP={},LB={},T={}", atr_p, lb, t);
                    }

                    // Write equity CSV for interesting configs (baseline T=0, winner T=5, and some runner-ups)
                    if t == 0 || t == 5 || (atr_p == 21 && lb == 252 && t == 0) {
                        if let Some(eq) = sample_equity {
                            let fname = format!("{}equity_AP{:03}_LB{:03}_T{:03}.csv", EQUITY_DIR, atr_p, lb, t);
                            let mut f = File::create(&fname)?;
                            writeln!(f, "step,equity")?;
                            for (i, &e) in eq.iter().enumerate() {
                                writeln!(f, "{},{:.6}", i, e)?;
                            }
                        }
                    }
                }
            }
        }
    }

    // Write main results
    let mut f = File::create(CSV_OUT)?;
    for line in &all_results { writeln!(f, "{}", line)?; }

    // Write summary: aggregate by atr_period+lookback (averaged across thresholds) and by threshold alone
    let mut summary: Vec<String> = vec![
        "atr_period,lookback,threshold,pass,total,pass_pct,sharpe,ret_pct,dd_pct,trades,win_rate,config_key".to_string()
    ];

    // Re-parse for summary (all lines already written to CSV)
    // Find best per atr_period+lookback across thresholds
    let mut best_by_ap_lb: HashMap<(usize, usize), (f64, u32)> = HashMap::new();
    for line in all_results.iter().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 11 { continue; }
        let ap: usize = parts[0].parse().unwrap_or(0);
        let lb: usize = parts[1].parse().unwrap_or(0);
        let t: u32 = parts[2].parse().unwrap_or(0);
        let sharpe: f64 = parts[6].parse().unwrap_or(0.0);
        let pass: usize = parts[3].parse().unwrap_or(0);
        let total: usize = parts[4].parse().unwrap_or(1);

        let entry = best_by_ap_lb.entry((ap, lb)).or_insert((f64::NEG_INFINITY, 999));
        if sharpe > entry.0 {
            *entry = (sharpe, t);
        }
    }

    // Write full detail summary
    for line in &all_results {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 11 { continue; }
        let ap: usize = parts[0].parse().unwrap_or(0);
        let lb: usize = parts[1].parse().unwrap_or(0);
        let t: u32 = parts[2].parse().unwrap_or(0);
        let key = format!("AP{:03}_LB{:03}_T{:03}", ap, lb, t);
        summary.push(format!("{},{},{},{},{},{},{:.3},{:.1},{:.1},{},{:.3},{}",
            ap, lb, t, parts[3], parts[4], parts[5], parts[6], parts[7], parts[8], parts[9], parts[10], key));
    }

    let mut g = File::create(SUMMARY_OUT)?;
    for line in &summary { writeln!(g, "{}", line)?; }

    eprintln!("\n\n==== Best Configurations by ATR_PERIOD ====");
    // Show top 10 by Sharpe (at T=5, the winning threshold)
    let mut ranked: Vec<(f64, String)> = Vec::new();
    for line in all_results.iter().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 11 { continue; }
        let t: u32 = parts[2].parse().unwrap_or(0);
        if t != 5 { continue; } // focus on T=5 winner
        let sharpe: f64 = parts[6].parse().unwrap_or(0.0);
        let config = format!("AP={},LB={}", parts[0], parts[1]);
        ranked.push((sharpe, config));
    }
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (i, (sharpe, cfg)) in ranked.iter().take(20).enumerate() {
        eprintln!("  #{:>2}: Sharpe {:.3} | {}", i+1, sharpe, cfg);
    }

    eprintln!("\nOverall best: {} (Sharpe {:.3})", best_config, best_sharpe);
    eprintln!("\nWrote: {}", CSV_OUT);
    eprintln!("Wrote: {} ({} lines)", SUMMARY_OUT, summary.len());
    eprintln!("Wrote equity CSVs to: {}", EQUITY_DIR);
    eprintln!("Elapsed: {:.1}s", t0.elapsed().as_secs_f64());

    Ok(())
}

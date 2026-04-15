//! HOLD_MAX Hyperopt: Full sweep of maximum hold duration for Turtle+Chandelier
//!
//! HOLD_MAX controls the maximum number of bars a trade can be held before
//! force-closing (Chandelier trailing stop usually fires before this limit).
//!
//! Current default: 60 (ARBITRARY — never tested)
//! Sweep range: 10 to 200, step 5 (39 values)
//! Across all 9 universes, walk-forward 252/252
//!
//! All other params held at validated values:
//!   TURTLE_ENTRY=21, CHAND_PERIOD=28, CHAND_MULT=2.0, POSITION_CAP=3

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
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;

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

const SWEEP_START: usize = 10;
const SWEEP_END: usize = 200;
const SWEEP_STEP: usize = 5;

const CSV_OUT: &str = "snapshots/hold_max_sweep.csv";
const EQUITY_CSV_OUT: &str = "snapshots/hold_max_equity_curves.csv";

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

fn turtle_signal(close: &[f64], _high: &[f64], entry_period: usize, idx: usize) -> bool {
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

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hold_max: usize,
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
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let mut exit_bar = (entry_bar_next + hold_max).min(n.saturating_sub(1));
                        for b in entry_bar_next..exit_bar.min(n.saturating_sub(1)) {
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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== HOLD_MAX Hyperopt: Sweep {}-{} step {} ====", SWEEP_START, SWEEP_END, SWEEP_STEP);
    eprintln!("EP={}, Chandelier({}, {}), CAP={}", TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, POSITION_CAP);

    // Build sweep values
    let hold_values: Vec<usize> = (SWEEP_START..=SWEEP_END)
        .step_by(SWEEP_STEP)
        .collect();
    eprintln!("Testing {} HOLD_MAX values: {:?}", hold_values.len(), hold_values);

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

    // === PHASE 1: Full sweep ===
    // Record: hold_max, universe, window, ret, sharpe, max_dd, trades, win_rate, pass
    let mut sweep_csv = vec!["hold_max,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    // Aggregate per (hold_max, universe): pass_count, total_windows, avg_sharpe, avg_ret, avg_dd, total_trades
    let mut agg: HashMap<(usize, String), (usize, usize, f64, f64, f64, usize)> = HashMap::new();

    // Track equity curves for top candidates
    let mut equity_data: HashMap<(usize, String), Vec<Vec<f64>>> = HashMap::new(); // (hold_max, universe) -> list of equity curves per window

    // Find total windows for progress
    let total_combos = hold_values.len() * UNIVERSES.len();
    let mut combo_idx = 0;

    for &hold_max in &hold_values {
        for &(label, symbols) in UNIVERSES {
            combo_idx += 1;
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            if combo_idx % 20 == 0 || combo_idx == total_combos {
                eprintln!("  Progress: {}/{} combos (HOLD_MAX={})", combo_idx, total_combos, hold_max);
            }

            let mut uni_pass = 0usize;
            let mut uni_total = 0usize;
            let mut uni_sharpe = 0.0_f64;
            let mut uni_ret = 0.0_f64;
            let mut uni_dd = 0.0_f64;
            let mut uni_trades = 0usize;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);

                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, hold_max);

                sweep_csv.push(format!(
                    "{},{},{},{:.4},{:.6},{:.4},{},{:.2},{}",
                    hold_max, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                ));

                if r.pass { uni_pass += 1; }
                uni_total += 1;
                uni_sharpe += r.sharpe;
                uni_ret += r.ret;
                uni_dd += r.max_dd;
                uni_trades += r.trades;

                // Store equity curves for select values (baseline=60, + top 5 candidates found later)
                if hold_max == 60 || hold_max == 30 || hold_max == 40 || hold_max == 50 || hold_max == 70 || hold_max == 80 || hold_max == 100 || hold_max == 120 || hold_max == 150 || hold_max == 200 {
                    equity_data.entry((hold_max, label.to_string()))
                        .or_insert_with(Vec::new)
                        .push(r.equity_curve.clone());
                }
            }

            agg.insert((hold_max, label.to_string()), (uni_pass, uni_total, uni_sharpe, uni_ret, uni_dd, uni_trades));
        }
    }

    eprintln!("\nPhase 1 complete in {:?}", t0.elapsed());

    // === PHASE 2: Write sweep CSV ===
    {
        let mut f = File::create(CSV_OUT)?;
        for line in &sweep_csv { writeln!(f, "{}", line)?; }
        eprintln!("Sweep CSV: {}", CSV_OUT);
    }

    // === PHASE 3: Compute per-hold_max global stats ===
    let mut global_stats: Vec<(usize, f64, f64, f64, f64, usize, usize, usize)> = Vec::new();
    // (hold_max, avg_sharpe, avg_ret, avg_dd, pass_rate, total_trades, pass_windows, total_windows)

    for &hold_max in &hold_values {
        let mut total_pass = 0usize;
        let mut total_windows = 0usize;
        let mut total_sharpe = 0.0_f64;
        let mut total_ret = 0.0_f64;
        let mut total_dd = 0.0_f64;
        let mut total_trades = 0usize;
        let mut n_unis = 0usize;

        for &(label, _) in UNIVERSES {
            if let Some(&(pass, total, sharpe, ret, dd, trades)) = agg.get(&(hold_max, label.to_string())) {
                total_pass += pass;
                total_windows += total;
                total_sharpe += sharpe;
                total_ret += ret;
                total_dd += dd.max(0.0);
                total_trades += trades;
                n_unis += 1;
            }
        }

        if total_windows > 0 {
            let avg_sharpe = total_sharpe / total_windows as f64;
            let avg_ret = total_ret / total_windows as f64;
            let avg_dd = total_dd / total_windows as f64;
            let pass_rate = total_pass as f64 / total_windows as f64 * 100.0;
            global_stats.push((hold_max, avg_sharpe, avg_ret, avg_dd, pass_rate, total_trades, total_pass, total_windows));
        }
    }

    // Sort by Sharpe to find winners
    let mut by_sharpe = global_stats.clone();
    by_sharpe.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Sort by pass rate (robustness) to find robustness winner
    let mut by_pass = global_stats.clone();
    by_pass.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

    eprintln!("\n==== TOP 10 BY AVG SHARPE ====");
    eprintln!("{:>8} {:>10} {:>10} {:>10} {:>8} {:>6} {:>6}", "HOLD_MAX", "AvgSharpe", "AvgRet%", "AvgDD%", "PassRate", "Pass", "Total");
    for (i, s) in by_sharpe.iter().take(10).enumerate() {
        let marker = if s.0 == 60 { " [BASE]" } else { "" };
        eprintln!("{:>4}. HM={:>3} sh={:8.4} ret={:+9.2}% dd={:7.2}% pass={:5.1}% ({}/{}){}", 
            i+1, s.0, s.1, s.2, s.3, s.4, s.6, s.7, marker);
    }

    eprintln!("\n==== TOP 10 BY PASS RATE ====");
    for (i, s) in by_pass.iter().take(10).enumerate() {
        let marker = if s.0 == 60 { " [BASE]" } else { "" };
        eprintln!("{:>4}. HM={:>3} pass={:5.1}% sh={:8.4} ret={:+9.2}% dd={:7.2}% ({}/{}){}", 
            i+1, s.0, s.4, s.1, s.2, s.3, s.6, s.7, marker);
    }

    // Find baseline rank
    if let Some(baseline) = global_stats.iter().find(|s| s.0 == 60) {
        let sh_rank = by_sharpe.iter().position(|s| s.0 == 60).unwrap() + 1;
        let pass_rank = by_pass.iter().position(|s| s.0 == 60).unwrap() + 1;
        eprintln!("\nBASELINE (HOLD_MAX=60): rank #{} by Sharpe, #{} by pass rate", sh_rank, pass_rank);
        eprintln!("  Sharpe={}, Ret={:+.2}%, DD={:.2}%, Pass={:.1}% ({}/{})", 
            baseline.1, baseline.2, baseline.3, baseline.4, baseline.6, baseline.7);
    }

    // === PHASE 4: Export equity curves for top candidates ===
    // Identify top 5 Sharpe values
    let top5_sharpe: Vec<usize> = by_sharpe.iter().take(5).map(|s| s.0).collect();
    let top5_pass: Vec<usize> = by_pass.iter().take(5).map(|s| s.0).collect();
    let baseline_val = 60usize;

    // Merge unique candidates for equity export
    let mut candidates: Vec<usize> = vec![baseline_val];
    for &v in &top5_sharpe { if !candidates.contains(&v) { candidates.push(v); } }
    for &v in &top5_pass { if !candidates.contains(&v) { candidates.push(v); } }

    // If needed, re-run equity curves for missing candidates
    for &hold_max in &candidates {
        for &(label, symbols) in UNIVERSES {
            let key = (hold_max, label.to_string());
            if equity_data.contains_key(&key) { continue; }

            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            let mut curves = Vec::new();
            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }
                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, hold_max);
                curves.push(r.equity_curve);
            }
            equity_data.insert(key, curves);
        }
    }

    // Write equity curves CSV: step,hold_max,universe,equity
    {
        let mut f = File::create(EQUITY_CSV_OUT)?;
        writeln!(f, "step,hold_max,universe,equity")?;

        for &hold_max in &candidates {
            for &(label, _) in UNIVERSES {
                if let Some(curves) = equity_data.get(&(hold_max, label.to_string())) {
                    // Concatenate all windows into a single curve
                    let mut combined: Vec<f64> = Vec::new();
                    for curve in curves {
                        combined.extend_from_slice(&curve);
                    }
                    for (step, &eq) in combined.iter().enumerate() {
                        writeln!(f, "{},{},{},{:.6}", step, hold_max, label, eq)?;
                    }
                }
            }
        }
        eprintln!("Equity CSV: {}", EQUITY_CSV_OUT);
    }

    // === PHASE 5: Summary report ===
    let mut summary = File::create("snapshots/hold_max_hyperopt_summary.md")?;
    writeln!(summary, "# HOLD_MAX Hyperopt Summary")?;
    writeln!(summary, "")?;
    writeln!(summary, "Sweep: {} to {} step {} ({} values)", SWEEP_START, SWEEP_END, SWEEP_STEP, hold_values.len())?;
    writeln!(summary, "Universes: 9 | Walk-forward: 252/252 | Other params: EP=21, Chand(28,2.0), CAP=3")?;
    writeln!(summary, "")?;
    writeln!(summary, "## Top 10 by Avg Sharpe")?;
    writeln!(summary, "")?;
    writeln!(summary, "| Rank | HOLD_MAX | AvgSharpe | AvgRet% | AvgDD% | PassRate | Pass/Total |")?;
    writeln!(summary, "|---|---|---|---|---|---|---|")?;
    for (i, s) in by_sharpe.iter().take(10).enumerate() {
        let marker = if s.0 == 60 { " **[BASE]**" } else { "" };
        writeln!(summary, "| {} | {} | {:.4} | {:+.2}% | {:.2}% | {:.1}% | {}/{} |{}", 
            i+1, s.0, s.1, s.2, s.3, s.4, s.6, s.7, marker)?;
    }

    writeln!(summary, "")?;
    writeln!(summary, "## Top 10 by Pass Rate (Robustness)")?;
    writeln!(summary, "")?;
    writeln!(summary, "| Rank | HOLD_MAX | PassRate | AvgSharpe | AvgRet% | AvgDD% | Pass/Total |")?;
    writeln!(summary, "|---|---|---|---|---|---|---|")?;
    for (i, s) in by_pass.iter().take(10).enumerate() {
        let marker = if s.0 == 60 { " **[BASE]**" } else { "" };
        writeln!(summary, "| {} | {} | {:.1}% | {:.4} | {:+.2}% | {:.2}% | {}/{} |{}", 
            i+1, s.0, s.4, s.1, s.2, s.3, s.6, s.7, marker)?;
    }

    // Per-universe breakdown for top 5
    writeln!(summary, "")?;
    writeln!(summary, "## Per-Universe Breakdown (Top 5 Sharpe)")?;
    writeln!(summary, "")?;
    for &hold_max in &top5_sharpe {
        writeln!(summary, "### HOLD_MAX = {}", hold_max)?;
        writeln!(summary, "")?;
        writeln!(summary, "| Universe | Pass | Total | AvgSharpe | AvgRet% | AvgDD% | Trades |")?;
        writeln!(summary, "|---|---|---|---|---|---|---|")?;
        for &(label, _) in UNIVERSES {
            if let Some(&(pass, total, sharpe, ret, dd, trades)) = agg.get(&(hold_max, label.to_string())) {
                let avg_sh = sharpe / total.max(1) as f64;
                let avg_r = ret / total.max(1) as f64;
                let avg_d = dd / total.max(1) as f64;
                writeln!(summary, "| {} | {} | {} | {:.4} | {:+.2}% | {:.2}% | {} |", 
                    label, pass, total, avg_sh, avg_r, avg_d, trades)?;
            }
        }
        writeln!(summary, "")?;
    }

    // Baseline comparison
    if let Some(baseline) = global_stats.iter().find(|s| s.0 == 60) {
        if let Some(winner) = by_sharpe.first() {
            let sh_delta = (winner.1 - baseline.1) / baseline.1.abs().max(0.001) * 100.0;
            let ret_delta = winner.2 - baseline.2;
            let dd_delta = winner.3 - baseline.3;
            let pass_delta = winner.4 - baseline.4;

            writeln!(summary, "## Winner vs Baseline")?;
            writeln!(summary, "")?;
            writeln!(summary, "| Metric | Baseline (HM=60) | Winner (HM={}) | Delta |", winner.0)?;
            writeln!(summary, "|---|---|---|---|")?;
            writeln!(summary, "| Avg Sharpe | {:.4} | {:.4} | {:+.2}% |", baseline.1, winner.1, sh_delta)?;
            writeln!(summary, "| Avg Return | {:+.2}% | {:+.2}% | {:+.2}pp |", baseline.2, winner.2, ret_delta)?;
            writeln!(summary, "| Avg DD | {:.2}% | {:.2}% | {:+.2}pp |", baseline.3, winner.3, dd_delta)?;
            writeln!(summary, "| Pass Rate | {:.1}% | {:.1}% | {:+.1}pp |", baseline.4, winner.4, pass_delta)?;
            writeln!(summary, "| Trades | {} | {} | {:+} |", baseline.5, winner.5, winner.5 as i64 - baseline.5 as i64)?;
        }
    }

    writeln!(summary, "")?;
    writeln!(summary, "Runtime: {:?}", t0.elapsed())?;

    eprintln!("\nSummary: snapshots/hold_max_hyperopt_summary.md");
    eprintln!("Total runtime: {:?}", t0.elapsed());

    Ok(())
}

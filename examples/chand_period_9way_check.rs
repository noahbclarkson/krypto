//! CHAND_PERIOD Robustness Check — All 9 Universes, 3 Candidate Values
//!
//! Tests CP=17 (winner), CP=28 (baseline), CP=20 (control) across all 9 universes.
//! Walks forward with 252/252. Frozen: EP=21, ATR_P=24, ATR_M=2.0, CHAND_M=2.15, HM=45, CAP=3
//! Goal: confirm CP=17 is the most robust across all universes.

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
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const CHAND_MULT: f64 = 2.15;

const CANDIDATES: &[usize] = &[17, 20, 28]; // winner, control, baseline

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

const CSV_OUT: &str = "snapshots/chand_period_9way_check.csv";
const MD_OUT: &str = "snapshots/chand_period_9way_check.md";

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

#[derive(Clone)]
struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    chand_period: usize,
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(String, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.clone(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect();

        if top_syms.is_empty() { bar += 1; continue; }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, EP, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, chand_period, b);
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
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }
        if !entered { bar += 1; }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&[1.0_f64]); // inline: just track peak/equity during sim
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

// Re-compute equity-based max_dd
fn run_sim_with_equity(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    chand_period: usize,
    test_start: usize,
    test_end: usize,
) -> (WfResult, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(String, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.clone(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect();

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

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, chand_period, b);
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
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    let result = WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass };
    (result, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== CHAND_PERIOD Robustness Check — All 9 Universes ====");
    eprintln!("Candidates: {:?}\n", CANDIDATES);

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
    for sym in all_syms.iter() {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            let close_v: Vec<f64> = df.column("close")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let high_v: Vec<f64>  = df.column("high")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let low_v: Vec<f64>   = df.column("low")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let vol_v: Vec<f64>   = df.column("volume")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            sym_data_map.insert(sym.clone(), SymData { close: close_v, high: high_v, low: low_v, vol: vol_v });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let windows: Vec<(usize, usize)> = (0..total_windows)
        .map(|wi| {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            (test_start, test_end)
        })
        .filter(|(s, e)| e.saturating_sub(*s) >= 5)
        .collect();
    let n_windows = windows.len();
    eprintln!("Windows: {}\n", n_windows);

    // [universe][candidate] = vec of WfResult per window
    let mut results: HashMap<String, HashMap<usize, Vec<WfResult>>> = HashMap::new();
    let mut all_global_pass: HashMap<usize, usize> = HashMap::new();
    let mut all_global_trades: HashMap<usize, usize> = HashMap::new();
    let mut all_sum_sharpe: HashMap<usize, f64> = HashMap::new();
    let mut all_sum_ret: HashMap<usize, f64> = HashMap::new();
    let mut all_sum_dd: HashMap<usize, f64> = HashMap::new();

    for &cp in CANDIDATES {
        all_global_pass.insert(cp, 0);
        all_global_trades.insert(cp, 0);
        all_sum_sharpe.insert(cp, 0.0);
        all_sum_ret.insert(cp, 0.0);
        all_sum_dd.insert(cp, 0.0);
    }

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s.as_str()));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing data)", label);
            continue;
        }

        eprintln!("==== {:<18} ====", label);
        let mut uni_results: HashMap<usize, Vec<WfResult>> = HashMap::new();
        for &cp in CANDIDATES {
            uni_results.insert(cp, Vec::new());
        }

        for &(test_start, test_end) in &windows {
            for &cp in CANDIDATES {
                let (r, _eq) = run_sim_with_equity(&sym_data_map, &symbols, cp, test_start, test_end);
                uni_results.get_mut(&cp).unwrap().push(r.clone());
                *all_global_pass.get_mut(&cp).unwrap() += if r.pass { 1 } else { 0 };
                *all_global_trades.get_mut(&cp).unwrap() += r.trades;
                *all_sum_sharpe.get_mut(&cp).unwrap() += r.sharpe;
                *all_sum_ret.get_mut(&cp).unwrap() += r.ret;
                *all_sum_dd.get_mut(&cp).unwrap() += r.max_dd;
            }
        }

        // Print per-universe summary
        for &cp in CANDIDATES {
            let res = uni_results.get(&cp).unwrap();
            let pass = res.iter().filter(|r| r.pass).count();
            let avg_sh: f64 = res.iter().map(|r| r.sharpe).sum::<f64>() / n_windows as f64;
            let avg_ret: f64 = res.iter().map(|r| r.ret).sum::<f64>() / n_windows as f64;
            let avg_dd: f64 = res.iter().map(|r| r.max_dd).sum::<f64>() / n_windows as f64;
            let marker = if cp == 17 { " ← WINNER" } else if cp == 28 { " ← BASELINE" } else { "" };
            eprintln!("  CP={:02}: pass={:2}/{} ({:3.0}%), sh={:.2}, ret={:+.1}%, DD={:.1}%{}",
                     cp, pass, n_windows, pass as f64 / n_windows as f64 * 100.0,
                     avg_sh, avg_ret, avg_dd, marker);
        }
        eprintln!();

        results.insert(label.to_string(), uni_results);
    }

    // Global summary
    eprintln!("==== GLOBAL SUMMARY ====");
    let mut rows: Vec<(usize, usize, f64, f64, f64, usize)> = Vec::new();
    for &cp in CANDIDATES {
        let gp = *all_global_pass.get(&cp).unwrap();
        let gt = *all_global_trades.get(&cp).unwrap();
        let avg_sh = *all_sum_sharpe.get(&cp).unwrap() / n_windows as f64 / UNIVERSES.len() as f64 * UNIVERSES.len() as f64;
        let avg_sh2 = *all_sum_sharpe.get(&cp).unwrap() / (n_windows * UNIVERSES.len()) as f64;
        let avg_ret2 = *all_sum_ret.get(&cp).unwrap() / (n_windows * UNIVERSES.len()) as f64;
        let avg_dd2 = *all_sum_dd.get(&cp).unwrap() / (n_windows * UNIVERSES.len()) as f64;
        rows.push((cp, gp, avg_sh2, avg_ret2, avg_dd2, gt));
        let pass_pct = gp as f64 / (n_windows * UNIVERSES.len()) as f64 * 100.0;
        let marker = if cp == 17 { " ← WINNER" } else if cp == 28 { " ← BASELINE" } else { "" };
        eprintln!("  CP={:02}: global pass={:3}/{} ({:3.0}%), avg_sh={:.2}, avg_ret={:+.1}%, avg_dd={:.1}%, t={}{}",
                 cp, gp, n_windows * UNIVERSES.len(), pass_pct, avg_sh2, avg_ret2, avg_dd2, gt, marker);
    }

    // Write CSV
    let mut f = File::create(CSV_OUT)?;
    writeln!(f, "candidate,global_pass,global_total,pass_pct,avg_sharpe,avg_return,avg_max_dd,total_trades")?;
    for &(cp, gp, sh, ret, dd, trades) in &rows {
        let total = n_windows * UNIVERSES.len();
        let pp = gp as f64 / total as f64 * 100.0;
        writeln!(f, "{},{},{},{:.2},{:.4},{:.2},{:.2},{}", cp, gp, total, pp, sh, ret, dd, trades)?;
    }

    // Write MD
    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# CHAND_PERIOD Robustness Check — All 9 Universes")?;
    writeln!(md, "")?;
    writeln!(md, "Candidates: {:?}", CANDIDATES)?;
    writeln!(md, "Frozen: EP={}, ATR_P={}, ATR_M={}, CHAND_M={}, HM={}, CAP={}", EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, CHAND_MULT, HOLD_MAX, POSITION_CAP)?;
    writeln!(md, "Universes: {}, Windows each: {}", UNIVERSES.len(), n_windows)?;
    writeln!(md, "")?;
    writeln!(md, "| Candidate | Global Pass | Pass% | Avg Sharpe | Avg Return | Avg DD | Trades |")?;
    writeln!(md, "|---|---|---|---|---|---|---|")?;
    for &(cp, gp, sh, ret, dd, trades) in &rows {
        let total = n_windows * UNIVERSES.len();
        let pp = gp as f64 / total as f64 * 100.0;
        let note = if cp == 17 { "**WINNER**" } else if cp == 28 { "BASELINE" } else { "" };
        writeln!(md, "| {} {} | {}/{} | {:3.1}% | {:.2} | {:+.1}% | {:.1}% | {} |", cp, note, gp, total, pp, sh, ret, dd, trades)?;
    }
    writeln!(md, "")?;
    let best = rows.iter().max_by_key(|r| r.1).map(|r| r.0).unwrap_or(28);
    writeln!(md, "**Most robust: CHAND_PERIOD={}** (highest global pass count)", best)?;

    eprintln!("\nRuntime: {:?}", t0.elapsed());
    eprintln!("CSV: {}", CSV_OUT);
    eprintln!("MD: {}", MD_OUT);

    Ok(())
}
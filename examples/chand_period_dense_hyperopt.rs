//! =========================================================
//! HYPERPARAMETER OPTENSIFICATION: CHAND_PERIOD Dense Sweep
//! =========================================================
//!
//! Background:
//!   CHAND_PERIOD controls the ATR lookback for the Chandelier trailing stop.
//!   Prior sweep (2026-04-21): CP∈[5..60 step 2] × 9 universes × 54 windows
//!   → CP=7 won (+6.9% Sharpe vs CP=11 baseline)
//!
//!   GAP: Step=2 sweep could miss the true optimum at an odd value.
//!   GAP: CP=7 was NEVER validated against pre-2021 data.
//!
//! Mission:
//!   1. Dense sweep: CP∈[5..60] step=1 (56 values — NOT step=2)
//!   2. 9-universe walk-forward validation (54 windows)
//!   3. Pre-2021 stress test for top candidates
//!   4. Export equity curves for baseline + winners to CSV
//!   5. Python chart: comparison_chart.png
//!
//! Usage:
//!   cargo run --release --example chand_period_dense_hyperopt --profile sweep

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

// ── Constants ─────────────────────────────────────────────────────────────────
const CANDLES: u32 = 3000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.0004;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const CHAND_MULT: f64 = 2.25;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.85;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 1;

// ── CHAND_PERIOD sweep: dense step=1 ─────────────────────────────────────────
const CHAND_PERIOD_MIN: usize = 5;
const CHAND_PERIOD_MAX: usize = 60;
const CHAND_PERIOD_STEP: usize = 1;

// ── Output CSVs ────────────────────────────────────────────────────────────────
const SUMMARY_CSV: &str = "snapshots/chand_period_dense_summary.csv";
const EQUITY_CSV: &str = "snapshots/chand_period_dense_equity.csv";

// ── Universes (USDT pairs — full 2018-2026 history) ────────────────────────────────
const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","BNBUSDT","SOLUSDT","XRPUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT"]),
    ("Legacy4",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LowVolume5",   &["LTCUSDT","EOSUSDT","BCHUSDT","LINKUSDT","ATOMUSDT"]),
    ("Blend5",       &["BTCUSDT","ETHUSDT","SOLUSDT","ADAUSDT","DOTUSDT"]),
    ("AltCap5",      &["SOLUSDT","AVAXUSDT","MATICUSDT","DOTUSDT","LINKUSDT"]),
];

// ── ATR helper ─────────────────────────────────────────────────────────────────
fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal(
    close: &[f64], high: &[f64], low: &[f64],
    entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize,
) -> bool {
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
    } else { false }
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

// ── SymData ────────────────────────────────────────────────────────────────────
#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

impl SymData {
    fn new(close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64>) -> Self {
        Self { close, high, low, vol }
    }
}

// ── Result struct ──────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
struct WfResult {
    cp: usize,
    universe: String,
    window: usize,
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    equity_final: f64,
    pass: bool,
}

// ── Run one walk-forward window ───────────────────────────────────────────────
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    cp: usize,
    universe_name: &str,
    window_idx: usize,
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut daily_rets = Vec::new();
    let mut wins = 0;
    let mut total_trades = 0usize;
    let mut bar = test_start;

    while bar < test_end {
        // Dollar-volume ranking
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for (sym, sd) in sym_data {
            if bar >= sd.close.len() { continue; }
            let rol_vol = if bar >= VOL_LOOKBACK {
                sd.vol[bar.saturating_sub(VOL_LOOKBACK)..=bar].iter().sum::<f64>() / VOL_LOOKBACK as f64
            } else { sd.vol[bar] };
            let price = sd.close.get(bar).copied().unwrap_or(0.0);
            let dv = rol_vol * price;
            scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP)
            .map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1; continue;
        }

        // Turtle breakout entry
        let mut entered = false;
        'symloop: for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, EP, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[bar];
                        let mut lowest_low_turtle = sd.low[bar];
                        let max_bar = (n - 1).min(bar + 200);
                        let mut exit_bar = max_bar;

                        // Dual exit search
                        for b in (entry_bar_next..=max_bar).rev() {
                            if b >= n { continue; }
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, cp, b);
                            if atr_chand > 0.0 {
                                let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                                let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                                if atr_turtle > 0.0 {
                                    let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                                    let stop = trail_chand.max(trail_turtle);
                                    if sd.low[b] <= stop {
                                        exit_bar = b;
                                        break;
                                    }
                                }
                            }
                            if sd.high[b] > highest_high_chand { highest_high_chand = sd.high[b]; }
                            if sd.low[b] < lowest_low_turtle { lowest_low_turtle = sd.low[b]; }
                        }

                        if exit_bar >= bar + HOLD_MAX { exit_bar = (bar + HOLD_MAX).min(n - 1); }

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
                            break 'symloop;
                        }
                    }
                }
            }
        }
        if !entered { equity_curve.push(equity); bar += 1; }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { cp, universe: universe_name.to_string(), window: window_idx,
               ret, sharpe, max_dd, trades: total_trades, win_rate, equity_final: equity, pass }
}

// ── Equity curve runner for Base5 universe ─────────────────────────────────────
fn run_equity_curve(
    sym_data: &HashMap<String, SymData>,
    cp: usize,
    start_bar: usize,
) -> Vec<f64> {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let universe_len = sym_data.values().map(|sd| sd.close.len()).min().unwrap_or(0);
    let mut bar = start_bar;

    while bar < universe_len {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for (sym, sd) in sym_data {
            if bar >= sd.close.len() { continue; }
            let rol_vol = if bar >= VOL_LOOKBACK {
                sd.vol[bar.saturating_sub(VOL_LOOKBACK)..=bar].iter().sum::<f64>() / VOL_LOOKBACK as f64
            } else { sd.vol[bar] };
            let price = sd.close.get(bar).copied().unwrap_or(0.0);
            let dv = rol_vol * price;
            scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP)
            .map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1; continue;
        }

        let mut entered = false;
        'symloop2: for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, EP, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[bar];
                        let mut lowest_low_turtle = sd.low[bar];
                        let max_bar = (n - 1).min(bar + 200);
                        let mut exit_bar = max_bar;

                        for b in (entry_bar_next..=max_bar).rev() {
                            if b >= n { continue; }
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, cp, b);
                            if atr_chand > 0.0 {
                                let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                                let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                                if atr_turtle > 0.0 {
                                    let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                                    let stop = trail_chand.max(trail_turtle);
                                    if sd.low[b] <= stop {
                                        exit_bar = b; break;
                                    }
                                }
                            }
                            if sd.high[b] > highest_high_chand { highest_high_chand = sd.high[b]; }
                            if sd.low[b] < lowest_low_turtle { lowest_low_turtle = sd.low[b]; }
                        }
                        if exit_bar >= bar + HOLD_MAX { exit_bar = (bar + HOLD_MAX).min(n - 1); }
                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            equity *= exit / entry;
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break 'symloop2;
                        }
                    }
                }
            }
        }
        if !entered { equity_curve.push(equity); bar += 1; }
    }
    equity_curve
}

// ── Main ───────────────────────────────────────────────────────────────────────
#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("==== CHAND_PERIOD Dense Sweep: CP∈[{}..{}] step={} ====", 
             CHAND_PERIOD_MIN, CHAND_PERIOD_MAX, CHAND_PERIOD_STEP);
    println!("   Fixed: EP={}, CM={}, ATR_P={}, AM={}, HM={}, EM={}",
             EP, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, ATR_ENTRY_MULT);
    println!("   Universes: 9 | Windows: 6 | Train: {} bars | Test: {} bars\n", TRAIN_BARS, TEST_BARS);

    // Load data
    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw_cache.insert(sym.clone(), df); }
            Err(e) => eprintln!("  WARNING: {} load failed: {}", sym, e),
        }
    }
    let n = min_len.min(2800);

    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            let close: Vec<f64> = df.column("close")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let high: Vec<f64> = df.column("high")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let low:  Vec<f64> = df.column("low")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let vol:  Vec<f64> = df.column("volume")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            sym_data_map.insert(sym.clone(), SymData::new(close, high, low, vol));
        }
    }

    // Build sweep list
    let cps: Vec<usize> = (CHAND_PERIOD_MIN..=CHAND_PERIOD_MAX+1)
        .filter(|x| (*x as isize - CHAND_PERIOD_MIN as isize) % CHAND_PERIOD_STEP as isize == 0)
        .collect();
    println!("Sweeping {} CHAND_PERIOD values (CP {}..{})\n", cps.len(), 
             cps.first().unwrap_or(&5), cps.last().unwrap_or(&60));

    // Run walk-forward sweep
    let mut all_results: Vec<WfResult> = Vec::new();
    for &cp in &cps {
        for (universe_name, syms) in UNIVERSES {
            let mut sym_data: HashMap<String, SymData> = HashMap::new();
            for &s in *syms {
                if let Some(sd) = sym_data_map.get(s) {
                    sym_data.insert(s.to_string(), sd.clone());
                }
            }
            let universe_len = sym_data.values().map(|sd| sd.close.len()).min().unwrap_or(0);

            for wi in 0..6 {
                let train_start = wi * TRAIN_BARS;
                let test_start = train_start + TRAIN_BARS;
                let test_end = (test_start + TEST_BARS).min(universe_len).min(n);
                if test_end <= test_start { continue; }
                let res = run_sim(&sym_data, cp, universe_name, wi, test_start, test_end);
                all_results.push(res);
            }
        }
    }

    // Aggregate by CP
    let mut by_cp: HashMap<usize, Vec<&WfResult>> = HashMap::new();
    for r in &all_results { by_cp.entry(r.cp).or_default().push(r); }

    // Write summary CSV
    let mut summary_lines = vec!["cp,universe,window,ret_pct,sharpe,max_dd_pct,trades,win_rate_pct,equity_final,pass".to_string()];
    for r in &all_results {
        summary_lines.push(format!("{},{},{},{:.2},{:.4},{:.2},{},{:.2},{:.6},{}",
            r.cp, r.universe, r.window, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.equity_final, r.pass));
    }
    let mut f = File::create(SUMMARY_CSV)?;
    for line in &summary_lines { writeln!(f, "{}", line)?; }
    println!("Wrote: {}\n", SUMMARY_CSV);

    // Compute CP stats
    let mut cp_stats: Vec<_> = by_cp.iter().map(|(cp, results)| {
        let pass_cnt = results.iter().filter(|r| r.pass).count();
        let total = results.len();
        let avg_sharpe = results.iter().map(|r| r.sharpe).sum::<f64>() / total as f64;
        let avg_ret   = results.iter().map(|r| r.ret).sum::<f64>() / total as f64;
        let avg_dd    = results.iter().map(|r| r.max_dd).sum::<f64>() / total as f64;
        let avg_equity= results.iter().map(|r| r.equity_final).sum::<f64>() / total as f64;
        (*cp, pass_cnt, total, pass_cnt as f64 / total as f64 * 100.0, avg_sharpe, avg_ret, avg_dd, avg_equity)
    }).collect();
    cp_stats.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap()); // by Sharpe desc

    println!("{}", "-".repeat(74));
    println!("  CP ranking by avg Sharpe (desc):");
    println!("{:>4} | {:>5} | {:>5} | {:>7} | {:>8} | {:>8} | {:>10}", 
             "CP", "Pass%", "Trds", "Sharpe", "AvgRet%", "AvgDD%", "AvgEquity");
    println!("{}", "-".repeat(65));
    for (cp, pass_cnt, _total, pass_pct, avg_sh, avg_ret, avg_dd, avg_eq) in &cp_stats {
        println!("{:>4} | {:>4.1}% | {:>5} | {:>7.3} | {:>8.1} | {:>7.1} | {:>10.3}",
                 cp, pass_pct, pass_cnt, avg_sh, avg_ret, avg_dd, avg_eq);
    }
    println!("{}", "-".repeat(74));

    // Export equity curves for top 5 + baseline (CP=7)
    let top_cps: Vec<usize> = cp_stats.iter().take(5).map(|(cp,_,_,_,_,_,_,_)| *cp).collect();
    let baseline_cp = 7usize;
    if !top_cps.contains(&baseline_cp) {
        eprintln!("WARNING: baseline CP={} not in top 5 — skipping equity export", baseline_cp);
    }

    // Build Base5 data
    let base5_syms = UNIVERSES[0].1;
    let mut base5_data: HashMap<String, SymData> = HashMap::new();
    for &s in base5_syms {
        if let Some(sd) = sym_data_map.get(s) {
            base5_data.insert(s.to_string(), sd.clone());
        }
    }

    println!("\nExporting equity curves for top CPs: {:?} (baseline CP={})", top_cps, baseline_cp);
    let mut equity_lines = vec!["cp,bar,equity_mult".to_string()];
    for &cp in &top_cps {
        let curve = run_equity_curve(&base5_data, cp, TRAIN_BARS);
        for (bi, &eq) in curve.iter().enumerate() {
            equity_lines.push(format!("{},{},{:.8}", cp, bi, eq));
        }
        let final_eq = curve.last().unwrap_or(&1.0);
        println!("  CP={:>2}: {} bars, final equity {:.4}", cp, curve.len(), final_eq);
    }

    let mut f = File::create(EQUITY_CSV)?;
    for line in &equity_lines { writeln!(f, "{}", line)?; }
    println!("Wrote: {}\n", EQUITY_CSV);

    // Summary
    let winner = cp_stats.first().map(|first| first.0).unwrap_or(7);
    let baseline_s = cp_stats.iter().find(|(cp,_,_,_,_,_,_,_)| *cp == baseline_cp)
        .map(|(_,_,_,_,sh,_,_,_)| *sh).unwrap_or(0.0);
    let winner_s   = cp_stats.first().map(|(_,_,_,_,sh,_,_,_)| *sh).unwrap_or(0.0);
    let delta_pct = if baseline_s > 0.0 { (winner_s - baseline_s) / baseline_s * 100.0 } else { 0.0 };

    println!("WINNER: CP={} (avg Sharpe {:.4}, baseline CP={} Sharpe {:.4}, delta {:+.2})", 
             winner, winner_s, baseline_cp, baseline_s, delta_pct);
    println!("    Top 5 by Sharpe: {:?}", top_cps);
    println!("    Total window-runs: {}", all_results.len());
    println!("    Elapsed: {:.1}s", t0.elapsed().as_secs_f64());

    Ok(())
}
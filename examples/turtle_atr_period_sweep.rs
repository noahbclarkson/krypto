//! Turtle ATR Period Hyperopt — Turtle-Only Exit Walk-Forward
//!
//! HYPOTHESIS: TURTLE_ATR_PERIOD=24 was validated in DUAL-EXIT mode (Chandelier fires first
//! 100% of the time). The live bot uses Turtle-ONLY exit. Is ATR_P=24 optimal for Turtle-only?
//!
//! Sweep: ATR_P ∈ [5..100 step 5] = 20 values × 9 universes × 6 windows
//! Production params: EP=21, ATR_M=2.0, HM=12, CAP=3, ATR_ENTRY_MULT=0.00
//! Selection: robustness-first (pass rate > positive universes > avg Sharpe)

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
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 90; // hyperopt 2026-04-30: updated from 8 to 90 (100-value sweep). See memory/hyperopt-2026-04-30-vol-lookback.md

const ATR_PERIODS: &[usize] = &[
    5, 10, 15, 20, 24, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100
];

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

const SUMMARY_CSV: &str = "snapshots/turtle_atr_period_sweep_summary.csv";
const EQUITY_CSV: &str = "snapshots/turtle_atr_period_sweep_equity.csv";
const DETAIL_CSV: &str = "snapshots/turtle_atr_period_sweep_detail.csv";
const SELECTED_CSV: &str = "snapshots/turtle_atr_period_sweep_selected.csv";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

#[derive(Clone)]
struct WfWindow {
    test_start: usize,
    test_end: usize,
}

#[derive(Clone)]
struct UniverseSetup {
    name: String,
    symbols: Vec<String>,
    windows: Vec<WfWindow>,
    sym_data: HashMap<String, SymData>,
}

#[derive(Clone, Copy)]
struct WfMetrics {
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
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { vals[idx] } else { vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64 }
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
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

fn max_dd_from(equity: f64) -> f64 {
    // Called with final equity only; use a simple approximation
    let peak = 1.0_f64;
    ((peak - equity) / peak).max(0.0) * 100.0
}

/// Run Turtle-only simulation for a single window
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_period: usize,
) -> WfMetrics {
    let mut equity = 1.0_f64;
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
                let price = sd.close[bar];
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();
        if top_syms.is_empty() { bar += 1; continue; }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, atr_period, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar {
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, atr_period, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_turtle { exit_bar = b; break; }
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
    let max_dd = max_dd_from(equity);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    WfMetrics { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

// ─── equity curve for one ATR period, aggregated across all windows ───────────
fn compute_aggregate_equity(
    uni_setups: &[UniverseSetup],
    atr_period: usize,
) -> Vec<f64> {
    // Aggregate daily returns across all windows (compounded)
    // For simplicity: compound per-window equity curves, then average at each step
    let mut all_eqs: Vec<Vec<f64>> = Vec::new();
    for us in uni_setups {
        for w in &us.windows {
            let _metrics = run_sim(&us.sym_data, &us.symbols, w.test_start, w.test_end, atr_period);
            // Re-run to get actual equity curve
            let eq = simulate_equity_curve(&us.sym_data, &us.symbols, w.test_start, w.test_end, atr_period);
            all_eqs.push(eq);
        }
    }
    // Find max length
    let max_len = all_eqs.iter().map(|e| e.len()).max().unwrap_or(1).max(1);
    // Pad shorter curves with last value
    let mut result = Vec::with_capacity(max_len);
    for step in 0..max_len {
        let mut prod = 1.0_f64;
        for eq in &all_eqs {
            let v = eq.get(step).copied().unwrap_or(*eq.last().unwrap_or(&1.0));
            prod *= v;
        }
        // Geometric mean across windows
        let n = all_eqs.len() as f64;
        result.push(if n > 0.0 { prod.powf(1.0 / n) } else { 1.0 });
    }
    result
}

fn simulate_equity_curve(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_period: usize,
) -> Vec<f64> {
    let mut equity = 1.0_f64;
    let mut eq_curve = vec![1.0_f64];
    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close[bar];
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();
        if top_syms.is_empty() { bar += 1; eq_curve.push(equity); continue; }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, atr_period, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar {
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, atr_period, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_turtle { exit_bar = b; break; }
                        }
                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                            equity *= 1.0 + gross_ret;
                            for _ in 0..bars_held { eq_curve.push(equity); }
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }
        if !entered { bar += 1; eq_curve.push(equity); }
    }
    eq_curve
}

// ─── main ─────────────────────────────────────────────────────────────────────
#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Turtle ATR Period Sweep — Turtle-Only Exit ====\n");
    eprintln!("Sweeping ATR_P ∈ {:?}", ATR_PERIODS);

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    eprintln!("Loaded {} symbols, {} bars\n", raw_cache.len(), n);

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
                close: col_vec!("close"), high: col_vec!("high"),
                low: col_vec!("low"), vol: col_vec!("volume"),
            });
        }
    }

    // Pre-compute universe setups
    let mut uni_setups: Vec<UniverseSetup> = Vec::new();
    for &(uname, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        let sym_data: HashMap<String, SymData> = symbols.iter()
            .filter_map(|s| sym_data_map.get(s).map(|sd| (s.to_string(), SymData {
                close: sd.close.clone(), high: sd.high.clone(),
                low: sd.low.clone(), vol: sd.vol.clone(),
            })))
            .collect();

        let windows: Vec<WfWindow> = (0..total_windows).filter_map(|wi| {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { None } else { Some(WfWindow { test_start, test_end }) }
        }).collect();

        uni_setups.push(UniverseSetup { name: uname.to_string(), symbols, windows, sym_data });
    }

    let n_atr = ATR_PERIODS.len();
    let n_uni = uni_setups.len();
    eprintln!("Running {} ATR periods × {} universes × {} windows...\n", n_atr, n_uni, 6);

    #[derive(Clone)]
    struct SummaryEntry {
        atr_period: usize,
        metrics: WfMetrics,
        total_pass: usize,
        total_windows: usize,
        pos_unis: usize,
    }

    let mut all_summary: Vec<SummaryEntry> = Vec::with_capacity(n_atr);
    let mut detail_rows = vec!("atr_period,universe,window,ret,sharpe,max_dd,trades,win_rate,pass".to_string());

    for &atr_p in ATR_PERIODS {
        let t1 = Instant::now();
        let mut total_pass = 0usize;
        let mut total_windows = 0usize;
        let mut sum_sharpe = 0.0_f64;
        let mut sum_ret = 0.0_f64;
        let mut sum_dd = 0.0_f64;
        let mut sum_trades = 0usize;
        let mut pos_unis = 0usize;

        for us in &uni_setups {
            let mut u_positive = false;
            for (wi, w) in us.windows.iter().enumerate() {
                let m = run_sim(&us.sym_data, &us.symbols, w.test_start, w.test_end, atr_p);
                detail_rows.push(format!(
                    "{},{},W{:02},{:.2},{:.4},{:.2},{},{:.1},{}",
                    atr_p, us.name, wi, m.ret, m.sharpe, m.max_dd, m.trades, m.win_rate, m.pass
                ));
                total_pass += if m.pass { 1 } else { 0 };
                total_windows += 1;
                sum_sharpe += m.sharpe;
                sum_ret += m.ret;
                sum_dd += m.max_dd;
                sum_trades += m.trades;
                if m.ret > 0.0 { u_positive = true; }
            }
            if u_positive { pos_unis += 1; }
        }

        let avg_sharpe = if total_windows > 0 { sum_sharpe / total_windows as f64 } else { 0.0 };
        let avg_ret = if total_windows > 0 { sum_ret / total_windows as f64 } else { 0.0 };
        let avg_dd = if total_windows > 0 { sum_dd / total_windows as f64 } else { 0.0 };

        all_summary.push(SummaryEntry {
            atr_period: atr_p,
            metrics: WfMetrics { ret: avg_ret, sharpe: avg_sharpe, max_dd: avg_dd, trades: sum_trades, win_rate: 0.0, pass: total_pass >= total_windows * 70 / 100 },
            total_pass,
            total_windows,
            pos_unis,
        });

        eprintln!(
            "ATR_P={:3}: {:2}/{:2} pass ({:5.1}%), Sharpe={:.3}, Ret={:+7.1}%, DD={:5.1}%, {} trades [{:.1}s]",
            atr_p, total_pass, total_windows,
            if total_windows > 0 { total_pass as f64 / total_windows as f64 * 100.0 } else { 0.0 },
            avg_sharpe, avg_ret, avg_dd, sum_trades, t1.elapsed().as_secs_f64()
        );
    }

    // Sort by robustness: avg_sharpe desc (but use pass rate + pos_unis as tiebreaker)
    all_summary.sort_by(|a, b| {
        let ar = if a.total_windows > 0 { a.total_pass as f64 / a.total_windows as f64 } else { 0.0 };
        let br = if b.total_windows > 0 { b.total_pass as f64 / b.total_windows as f64 } else { 0.0 };
        // Primary: pass rate desc
        let cmp1 = br.partial_cmp(&ar).unwrap();
        if cmp1 != std::cmp::Ordering::Equal { return cmp1; }
        // Secondary: pos_unis desc
        let cmp2 = b.pos_unis.cmp(&a.pos_unis);
        if cmp2 != std::cmp::Ordering::Equal { return cmp2; }
        // Tertiary: avg_sharpe desc
        b.metrics.sharpe.partial_cmp(&a.metrics.sharpe).unwrap()
    });

    eprintln!("\n==== Robustness Ranking (pass_rate | pos_unis | Sharpe) ====");
    eprintln!("{:>6} | {:>5} | {:>5} | {:>9} | {:>8} | {:>7} | {:>6}", "ATR_P", "Pass", "PosU", "AvgSharpe", "AvgRet%", "AvgDD%", "Trades");
    eprintln!("{}", "-".repeat(65));
    for s in &all_summary {
        let pr = if s.total_windows > 0 { s.total_pass as f64 / s.total_windows as f64 * 100.0 } else { 0.0 };
        eprintln!("{:>6} | {:>5.0}% | {:>5} | {:>9.3} | {:>+8.1} | {:>7.1} | {:>6}",
            s.atr_period, pr, s.pos_unis, s.metrics.sharpe, s.metrics.ret, s.metrics.max_dd, s.metrics.trades);
    }

    // Write summary CSV
    {
        let mut f = File::create(SUMMARY_CSV)?;
        writeln!(f, "atr_period,pass_rate,total_pass,total_windows,pos_unis,avg_sharpe,avg_ret,avg_dd,total_trades")?;
        for s in &all_summary {
            let pr = if s.total_windows > 0 { s.total_pass as f64 / s.total_windows as f64 * 100.0 } else { 0.0 };
            writeln!(f, "{},{:.2},{:.0},{:.0},{},{:.4},{:.2},{:.2},{}",
                s.atr_period, pr, s.total_pass, s.total_windows, s.pos_unis,
                s.metrics.sharpe, s.metrics.ret, s.metrics.max_dd, s.metrics.trades)?;
        }
        eprintln!("\nWrote: {}", SUMMARY_CSV);
    }

    // Write detail CSV
    {
        let mut f = File::create(DETAIL_CSV)?;
        for row in &detail_rows { writeln!(f, "{}", row)?; }
        eprintln!("Wrote: {}", DETAIL_CSV);
    }

    // Write equity curves for top 4 + baseline(24)
    {
        let top_n = 4;
        let baseline_atr = 24;
        let mut selected: Vec<usize> = all_summary.iter().take(top_n).map(|s| s.atr_period).collect();
        if !selected.contains(&baseline_atr) {
            if selected.len() >= top_n { selected.pop(); }
            selected.push(baseline_atr);
        }
        // Reorder selected to match all_summary ranking (already sorted)
        let selected_set: std::collections::HashSet<usize> = selected.iter().cloned().collect();
        let mut ordered: Vec<usize> = all_summary.iter()
            .map(|s| s.atr_period)
            .filter(|&ap| selected_set.contains(&ap))
            .collect();
        selected.clear();
        selected.extend(ordered);

        let mut eq_rows = vec!("atr_period,step,equity".to_string());
        for &atr_p in &selected {
            let eq_curve = compute_aggregate_equity(&uni_setups, atr_p);
            for (step, &eq) in eq_curve.iter().enumerate() {
                eq_rows.push(format!("{},{},{:.6}", atr_p, step, eq));
            }
        }
        let mut f = File::create(EQUITY_CSV)?;
        for row in &eq_rows { writeln!(f, "{}", row)?; }
        eprintln!("Wrote equity curves: {} ({} rows)", EQUITY_CSV, eq_rows.len());

        // Also write selected flat CSV for the chart script
        let mut flat_rows = vec!("atr_period,universe,window,final_equity,final_ret_pct,final_dd_pct,sharpe,trades,pass".to_string());
        for &atr_p in &selected {
            for us in &uni_setups {
                for (wi, w) in us.windows.iter().enumerate() {
                    let m = run_sim(&us.sym_data, &us.symbols, w.test_start, w.test_end, atr_p);
                    let final_eq = 1.0 + m.ret / 100.0;
                    flat_rows.push(format!("{},{},W{:02},{:.4},{:.2},{:.2},{:.4},{},{}",
                        atr_p, us.name, wi, final_eq, m.ret, m.max_dd, m.sharpe, m.trades, m.pass));
                }
            }
        }
        let mut f = File::create(SELECTED_CSV)?;
        for row in &flat_rows { writeln!(f, "{}", row)?; }
        eprintln!("Wrote selected: {}", SELECTED_CSV);
    }

    eprintln!("\nTotal elapsed: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

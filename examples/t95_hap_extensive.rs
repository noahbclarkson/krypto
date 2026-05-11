//! T95: HEDGE_ATR_PCT Extensive Hyperopt
//!
//! Purpose: audit and optimize HEDGE_ATR_PCT — the percentile threshold that
//! controls when the USDT hedge overlay activates.
//!
//! Background:
//! - T67 (2026-05-05) found IDENTICAL results for all HEDGE_ATR_PCT values on the
//!   WALK-FORWARD RESEARCH harness. WRONG CONCLUSION: T67 tested on dual Chandelier+Turtle
//!   research harness, not on the exact-live bot path (Turtle-only exit). The two systems
//!   differ mechanically. HEDGE_ATR_PCT could be significant on exact-live path.
//! - HEDGE_ATR_PCT: hedge activates when BTC hedge ATR > HAP-th percentile of
//!   HEDGE_LOOKBACK-day true-range history.
//! - Current hardcoded default: 0.45 (45th pct). No documented justification.
//! - This sweep: full range [0.01..0.99] step 0.01 (99 values) × 6 walk-forward
//!   windows × Base5 on the exact-live bot path.
//!
//! Outputs:
//! - snapshots/t95_hap_sweep.csv      — all (hap, window, sharpe, return, dd, trades)
//! - snapshots/t95_hap_summary.csv    — aggregated by HAP (pass_rate, avg_sharpe, etc.)
//! - snapshots/t95_hap_equity.csv     — equity curves for selected HAP values
//! - charts/comparison_chart.png      — equity curve visualization

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::live::config::{
    LiveConfig, ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_ATR_PERIOD,
    HEDGE_LOOKBACK, HEDGE_SIZE_MULT, HOLD_MAX, POSITION_CAP, REGIME_ATR_PERIOD,
    REGIME_LOOKBACK, TURTLE_ATR_MULT, TURTLE_ATR_PERIOD, TURTLE_EP, VOL_LOOKBACK,
};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;
const BASE_SYMBOLS: [&str; 6] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    dates: Vec<String>,
}

struct Bar { close: f64, high: f64, low: f64 }

struct PositionState {
    entry_bar: usize,
    entry_exec: f64,
    size: f64,
    highest_high: f64,
    bars_held: usize,
    atr_buf: VecDeque<f64>,
}

fn tr(_c: f64, h: f64, l: f64, pc: f64) -> f64 {
    (h - l).max((h - pc).abs()).max((l - pc).abs())
}

fn seed_atr(sd: &SymData, entry_idx: usize) -> VecDeque<f64> {
    let mut buf = VecDeque::with_capacity(TURTLE_ATR_PERIOD);
    for i in entry_idx.saturating_sub(TURTLE_ATR_PERIOD)..entry_idx {
        let pc = if i == 0 { sd.close[i] } else { sd.close[i - 1] };
        buf.push_back(tr(sd.close[i], sd.high[i], sd.low[i], pc));
    }
    buf
}

fn max_close(sd: &SymData, idx: usize) -> f64 {
    let start = idx.saturating_sub(TURTLE_EP);
    let mut m = f64::NEG_INFINITY;
    for i in start..idx {
        if sd.close[i] > m { m = sd.close[i]; }
    }
    m
}

fn btc_atr_pct(btc: &[Bar], idx: usize) -> f64 {
    let n = btc.len();
    if n <= REGIME_ATR_PERIOD.max(REGIME_LOOKBACK) + 1 || idx >= n { return 50.0; }
    // Current ATR/close ratio
    let mut sum = 0.0_f64;
    for i in (idx + 1 - REGIME_ATR_PERIOD)..=idx {
        let pc = if i == 0 { btc[i].close } else { btc[i - 1].close };
        sum += tr(btc[i].close, btc[i].high, btc[i].low, pc);
    }
    let curr_atr = sum / REGIME_ATR_PERIOD as f64;
    let curr_close = btc[idx].close;
    if curr_atr <= 0.0 || curr_close <= 0.0 { return 50.0; }
    let curr_ratio = curr_atr / curr_close;

    let start = idx.saturating_sub(REGIME_LOOKBACK);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let close = btc[i].close;
        if close <= 0.0 { continue; }
        let mut s = 0.0_f64;
        for j in (i + 1 - REGIME_ATR_PERIOD)..=i {
            let pc = if j == 0 { btc[j].close } else { btc[j - 1].close };
            s += tr(btc[j].close, btc[j].high, btc[j].low, pc);
        }
        let atr = s / REGIME_ATR_PERIOD as f64;
        if atr / close < curr_ratio { below += 1; }
        total += 1;
    }
    if total == 0 { 50.0 } else { below as f64 / total as f64 * 100.0 }
}

fn hedge_bars(btc: &[Bar]) -> (f64, Vec<f64>) {
    let n = btc.len();
    // Current hedge ATR
    let mut trs = Vec::with_capacity(HEDGE_ATR_PERIOD);
    for i in n.saturating_sub(HEDGE_ATR_PERIOD)..n {
        let b = &btc[i];
        let pc = if i == 0 { b.close } else { btc[i - 1].close };
        trs.push(tr(b.close, b.high, b.low, pc));
    }
    let hedge_atr = trs.iter().sum::<f64>() / HEDGE_ATR_PERIOD as f64;

    // Sorted TR history for percentile lookup
    let mut hist = Vec::with_capacity(HEDGE_LOOKBACK);
    for j in 1..=HEDGE_LOOKBACK {
        let idx_j = n.saturating_sub(j);
        if idx_j == 0 { break; }
        let bj = &btc[idx_j];
        let pcj = if idx_j == 0 { bj.close } else { btc[idx_j - 1].close };
        hist.push(tr(bj.close, bj.high, bj.low, pcj));
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (hedge_atr, hist)
}

fn is_hedge(hap: f64, hedge_atr: f64, hist: &[f64]) -> bool {
    if hist.is_empty() { return false; }
    let idx = (hap * hist.len() as f64) as usize;
    let idx = idx.min(hist.len() - 1);
    hedge_atr > hist[idx]
}

fn annualised_sharpe(eq: &[f64]) -> f64 {
    if eq.len() < 2 { return 0.0; }
    let mut rets = Vec::with_capacity(eq.len() - 1);
    for i in 1..eq.len() {
        if eq[i - 1] > 0.0 && eq[i] > 0.0 {
            rets.push(eq[i] / eq[i - 1] - 1.0);
        }
    }
    if rets.is_empty() { return 0.0; }
    let mean: f64 = rets.iter().sum::<f64>() / rets.len() as f64;
    let var: f64 = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    let std = var.sqrt();
    if std == 0.0 { return 0.0; }
    let s = mean / std * (365.0_f64).sqrt();
    if !s.is_finite() { 0.0 } else { s }
}

fn max_dd(eq: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut mdd = 0.0;
    for &e in eq {
        if e > peak { peak = e; }
        let d = (peak - e) / peak * 100.0;
        if d > mdd { mdd = d; }
    }
    mdd
}

fn year_from_date(date: &str) -> i32 {
    date.get(0..4).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0)
}

fn align_data(raw: &HashMap<String, SymData>) -> HashMap<String, SymData> {
    let common_dates = {
        let btc = raw.get("BTCUSDT").expect("BTCUSDT loaded");
        let mut d = btc.dates.clone();
        d.retain(|d| raw.values().all(|sd| sd.dates.iter().any(|x| x == d)));
        d.sort();
        d.dedup();
        d
    };
    let mut data: HashMap<String, SymData> = HashMap::new();
    for (sym, sd) in raw {
        let idx_map: HashMap<String, usize> = sd.dates.iter().enumerate().map(|(i, d)| (d.clone(), i)).collect();
        let mut close = Vec::with_capacity(common_dates.len());
        let mut high = Vec::with_capacity(common_dates.len());
        let mut low = Vec::with_capacity(common_dates.len());
        let mut dates = Vec::with_capacity(common_dates.len());
        for d in &common_dates {
            if let Some(&i) = idx_map.get(d) {
                close.push(sd.close[i]);
                high.push(sd.high[i]);
                low.push(sd.low[i]);
                dates.push(d.clone());
            }
        }
        data.insert(sym.clone(), SymData { close, high, low, dates });
    }
    data
}

struct WindowResult {
    sharpe: f64,
    ret: f64,
    dd: f64,
    trades: usize,
    equity: Vec<f64>,
}

/// Run exact-live simulation for one HAP value over one test period.
/// Returns: (sharpe, ret_pct, max_dd, trades, equity_curve, hedge_entries)
fn run_sim(
    data: &HashMap<String, SymData>,
    btc: &[Bar],
    hap: f64,
    test_start: usize,
    test_end: usize,
    fee: f64,
) -> WindowResult {
    let symbols: Vec<String> = BASE_SYMBOLS.iter().map(|s| s.to_string()).collect();
    let mut realized = 1.0_f64;
    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut equity_curve: Vec<f64> = Vec::with_capacity(test_end - test_start);
    let mut trades = 0usize;

    // Pre-compute hedge ATR and history (static for this run — same for all bars).
    let (hedge_atr, hist) = hedge_bars(btc);

    for idx in test_start..test_end {
        // ---- Process existing positions: exit checks + mark-to-market ----
        for sym in &symbols {
            let sd = match data.get(sym) { Some(s) => s, None => continue };

            if let Some(mut pos) = positions.remove(sym) {
                if idx > pos.entry_bar {
                    if sd.high[idx] > pos.highest_high { pos.highest_high = sd.high[idx]; }
                    pos.bars_held += 1;
                    let prev_close = sd.close[idx];
                    let t = tr(sd.close[idx], sd.high[idx], sd.low[idx], prev_close);
                    pos.atr_buf.push_back(t);
                    if pos.atr_buf.len() > TURTLE_ATR_PERIOD { pos.atr_buf.pop_front(); }

                    let mut exit_reason: Option<String> = None;
                    if pos.bars_held >= HOLD_MAX {
                        exit_reason = Some("HOLD_MAX".to_string());
                    } else if pos.atr_buf.len() >= TURTLE_ATR_PERIOD {
                        let atr = pos.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                        let stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                        if atr > 0.0 && sd.low[idx] <= stop {
                            exit_reason = Some("TURTLE_ATR".to_string());
                        }
                    }

                    if let Some(_reason) = exit_reason {
                        let exit_exec = sd.close[idx] * (1.0 - fee);
                        let pct_ret = exit_exec / pos.entry_exec - 1.0;
                        realized *= 1.0 + pos.size * pct_ret;
                        trades += 1;
                        continue;
                    }
                }
                positions.insert(sym.clone(), pos);
            }
        }

        // ---- Process flat positions: check for entry ----
        if positions.len() < POSITION_CAP {
            for sym in &symbols {
                let sd = match data.get(sym) { Some(s) => s, None => continue };

                // Skip if already has position.
                if positions.contains_key(sym) { continue; }

                // Turtle entry: current-inclusive EP window.
                if sd.close[idx] < max_close(sd, idx) { continue; }

                // ATR_RANK gate.
                let btc_pct = btc_atr_pct(btc, idx);
                if btc_pct < ATR_RANK_THRESHOLD { continue; }

                // Hedge overlay.
                let hedge = is_hedge(hap, hedge_atr, &hist);
                let mut size = 1.0 / POSITION_CAP as f64;
                if hedge { size *= HEDGE_SIZE_MULT; }

                positions.insert(sym.clone(), PositionState {
                    entry_bar: idx,
                    entry_exec: sd.close[idx] * (1.0 + fee),
                    size,
                    highest_high: sd.high[idx],
                    bars_held: 0,
                    atr_buf: seed_atr(sd, idx),
                });
            }
        }

        // Mark-to-market open positions.
        let mut mtm = realized;
        for (sym, pos) in &positions {
            if let Some(sd) = data.get(sym) {
                let pct_ret = (sd.close[idx] / pos.entry_exec - 1.0) * pos.size;
                mtm *= 1.0 + pct_ret;
            }
        }
        equity_curve.push(mtm);
    }

    // Liquidate final positions.
    let last_idx = test_end.saturating_sub(1);
    for (sym, pos) in positions.drain() {
        if let Some(sd) = data.get(&sym) {
            let exit_exec = sd.close[last_idx] * (1.0 - fee);
            let pct_ret = exit_exec / pos.entry_exec - 1.0;
            realized *= 1.0 + pos.size * pct_ret;
            trades += 1;
        }
    }
    // Update last equity point to final liquidation.
    if let Some(last) = equity_curve.last_mut() { *last = realized; }

    let ret = if equity_curve.first().map(|&e| e) == Some(0.0) || equity_curve.is_empty() {
        0.0
    } else {
        (equity_curve.last().unwrap() / equity_curve.first().unwrap() - 1.0) * 100.0
    };

    WindowResult {
        sharpe: annualised_sharpe(&equity_curve),
        ret,
        dd: max_dd(&equity_curve),
        trades,
        equity: equity_curve,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T95: HEDGE_ATR_PCT Extensive Hyperopt ===");
    let config = LiveConfig::default();
    let fee = config.fee_pct;
    println!("Fee: {:.2} bps/side | EP={}, ATR({},{}), HM={}, CAP={}",
        fee * 10_000.0, TURTLE_EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP);
    println!("HEDGE_ATR_P={}, HEDGE_LB={}, HEDGE_SIZE={}", HEDGE_ATR_PERIOD, HEDGE_LOOKBACK, HEDGE_SIZE_MULT);
    println!("Current HEDGE_ATR_PCT: {:.2} | ATR_RANK: AP={}, LB={}, T={:.1}",
        HEDGE_ATR_PCT, REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD);
    println!();

    // ── Load data ──────────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let mut raw_data: HashMap<String, SymData> = HashMap::new();
    for sym in BASE_SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let time_col = df.column("time").ok();
        let dates: Vec<String> = (0..close.len())
            .map(|i| time_col.and_then(|s| s.get(i).ok()).map(|v| v.to_string()).unwrap_or_default())
            .collect();
        raw_data.insert(sym.to_string(), SymData { close, high, low, dates });
    }

    let data = align_data(&raw_data);
    let btc_raw = data.get("BTCUSDT").expect("BTCUSDT loaded");
    let total_bars = btc_raw.close.len();

    // Build BTC bar slice for regime/hedge functions.
    let btc: Vec<Bar> = btc_raw.close.iter().enumerate()
        .map(|(i, &c)| Bar { close: c, high: btc_raw.high[i], low: btc_raw.low[i] })
        .collect();

    // ── Walk-forward windows ─────────────────────────────────────────────────
    let wf_step = 126;
    let mut wf_windows: Vec<(usize, usize, usize)> = Vec::new();
    let mut t = WARMUP_BARS + 252;
    while t + 252 <= total_bars {
        let ts = t;
        let te = (t + 252).min(total_bars);
        wf_windows.push((t.saturating_sub(252), ts, te));
        t += wf_step;
    }
    let n_windows = wf_windows.len();
    println!("Walk-forward windows: {} total", n_windows);
    for (i, &(tr, ts, te)) in wf_windows.iter().enumerate() {
        let train_yrs: Vec<i32> = btc_raw.dates[..tr].iter()
            .filter_map(|d| Some(year_from_date(d)))
            .collect();
        let test_yrs: Vec<i32> = btc_raw.dates[ts..te].iter()
            .filter_map(|d| Some(year_from_date(d)))
            .collect();
        println!("  W{}: train_end={}, test={}..{} (train:{:?}, test:{:?})",
            i, tr, ts, te, train_yrs, test_yrs);
    }
    println!();

    // ── HAP sweep ────────────────────────────────────────────────────────────
    // Full range: 0.01..0.99 step 0.01 = 99 values.
    let hap_values: Vec<f64> = (1..100).map(|i| i as f64 / 100.0).collect();
    let n_hap = hap_values.len();
    println!("Testing {} HAP values: 0.01..0.99 step 0.01", n_hap);

    // Per-HAP per-window results.
    let mut hap_sharpe: Vec<f64> = vec![0.0; n_hap];
    let mut hap_ret: Vec<f64> = vec![0.0; n_hap];
    let mut hap_dd: Vec<f64> = vec![0.0; n_hap];
    let mut hap_trades: Vec<usize> = vec![0; n_hap];
    let mut hap_pass: Vec<usize> = vec![0; n_hap];

    // Per-HAP per-window raw values for sweep CSV.
    let mut sweep_csv = String::from("hap,window,test_start,test_end,sharpe,return_pct,max_dd,trades\n");

    for (wi, &(train_end, test_start, test_end)) in wf_windows.iter().enumerate() {
        println!("Window {}: test {}..{}", wi, test_start, test_end);

        for (hi, &hap) in hap_values.iter().enumerate() {
            let r = run_sim(&data, &btc, hap, test_start, test_end, fee);
            hap_sharpe[hi] += r.sharpe;
            hap_ret[hi] += r.ret;
            hap_dd[hi] += r.dd;
            hap_trades[hi] += r.trades;
            if r.sharpe > 0.0 && r.ret > 0.0 {
                hap_pass[hi] += 1;
            }

            sweep_csv.push_str(&format!("{:.2},W{},{},{},{:.4},{:.2},{:.2},{}\n",
                hap, wi, test_start, test_end, r.sharpe, r.ret, r.dd, r.trades));
        }
        println!("  Window {} done ({} HAP values processed)", wi, n_hap);
    }

    // ── Summary by HAP ───────────────────────────────────────────────────────
    let mut summary_csv = String::from("hap,pass_count,pass_rate,avg_sharpe,avg_return,avg_dd,total_trades\n");
    let mut results: Vec<(f64, usize, f64, f64, f64, f64, usize)> = Vec::new();

    for (hi, &hap) in hap_values.iter().enumerate() {
        let passes = hap_pass[hi];
        let pass_rate = passes as f64 / n_windows as f64 * 100.0;
        let avg_s = hap_sharpe[hi] / n_windows as f64;
        let avg_r = hap_ret[hi] / n_windows as f64;
        let avg_d = hap_dd[hi] / n_windows as f64;
        let tot_t = hap_trades[hi];
        results.push((hap, passes, pass_rate, avg_s, avg_r, avg_d, tot_t));
    }

    // Sort by pass_count desc, then avg_sharpe desc.
    results.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.3.partial_cmp(&a.3).unwrap())
    });

    for (hap, passes, pass_rate, avg_s, avg_r, avg_d, tot_t) in &results {
        summary_csv.push_str(&format!("{:.2},{},{:.1},{:.4},{:.2},{:.2},{}\n",
            hap, passes, pass_rate, avg_s, avg_r, avg_d, tot_t));
    }

    // Top 10 for printing.
    println!("\n=== TOP 10 HAP VALUES BY ROBUSTNESS ===");
    for (i, &(hap, passes, pass_rate, avg_s, avg_r, avg_d, tot_t)) in results.iter().take(10).enumerate() {
        let rank = format!("{:>2}", i + 1);
        let pcts = format!("{:.0}", pass_rate);
        let rets = format!("{:+.1}", avg_r);
        let dds = format!("{:.1}", avg_d);
        println!("  {}: HAP={:.2} | {}/{} pass ({}%) | Sharpe={:.3} | Ret={}% | DD={}% | Trades={}",
            rank, hap, passes, n_windows, pcts, avg_s, rets, dds, tot_t);
    }
    println!();

    let best_hap = results.first().map(|r| r.0).unwrap_or(0.45);
    let best_pass = results.first().map(|r| r.1).unwrap_or(0);
    let best_sharpe = results.first().map(|r| r.3).unwrap_or(0.0);
    let baseline_hap = 0.45;

    let pct_str = format!("{:.0}", best_pass as f64 / n_windows as f64 * 100.0);
    println!("=== WINNER: HEDGE_ATR_PCT = {:.2} ===", best_hap);
    println!("Pass: {}/{} ({}%), Sharpe: {:.3}", best_pass, n_windows, pct_str, best_sharpe);

    // ── Write sweep CSVs ────────────────────────────────────────────────────
    File::create("snapshots/t95_hap_sweep.csv")?.write_all(sweep_csv.as_bytes())?;
    File::create("snapshots/t95_hap_summary.csv")?.write_all(summary_csv.as_bytes())?;
    println!("\nWritten: snapshots/t95_hap_sweep.csv, snapshots/t95_hap_summary.csv");

    // ── Equity curves for baseline, winner, and top runner-ups ───────────────
    // Pick top-5 HAP values for equity curves.
    let curve_haps: Vec<f64> = results.iter().take(5).map(|r| r.0).collect();
    println!("\nGenerating equity curves for: {:?}", curve_haps);

    // Run selected HAPs across all test windows and accumulate equity.
    let mut eq_cols: Vec<Vec<f64>> = vec![Vec::new(); curve_haps.len()];
    let mut total_test_bars = 0usize;

    for (wi, &(train_end, test_start, test_end)) in wf_windows.iter().enumerate() {
        for (ci, &hap) in curve_haps.iter().enumerate() {
            let r = run_sim(&data, &btc, hap, test_start, test_end, fee);
            // Normalize equity curve to start at 1.0.
            let norm = if let Some(&first) = r.equity.first() {
                if first > 0.0 { first } else { 1.0 }
            } else { 1.0 };
            let norm_eq: Vec<f64> = r.equity.iter().map(|&e| e / norm).collect();

            // Extend accumulated equity: concatenate windows.
            eq_cols[ci].extend(norm_eq);
        }
        total_test_bars += wf_windows[wi].2 - wf_windows[wi].1;
    }

    // Write equity CSV.
    let mut eq_csv = String::from("bar");
    for &hap in &curve_haps {
        eq_csv.push_str(&format!(",hap_{:.2}", hap));
    }
    eq_csv.push('\n');

    let n_bars = eq_cols.first().map(|c| c.len()).unwrap_or(0);
    for bar_i in 0..n_bars {
        eq_csv.push_str(&format!("{}", bar_i));
        for col in &eq_cols {
            let val = col.get(bar_i).copied().unwrap_or(1.0);
            eq_csv.push_str(&format!(",{:.6}", val));
        }
        eq_csv.push('\n');
    }
    File::create("snapshots/t95_hap_equity.csv")?.write_all(eq_csv.as_bytes())?;
    println!("Written: snapshots/t95_hap_equity.csv ({} bars, {} curves)", n_bars, curve_haps.len());

    // ── Python chart ─────────────────────────────────────────────────────────
    let chart_script = r#"
import sys, subprocess
try:
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    import matplotlib.ticker as mticker
except ImportError:
    print("matplotlib not available, skipping chart")
    sys.exit(0)

import csv

def load_csv(path):
    rows = []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            rows.append(row)
    return rows

rows = load_csv('/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t95_hap_equity.csv')
if not rows:
    print("No equity data")
    sys.exit(0)

headers = list(rows[0].keys())
hap_cols = [h for h in headers if h.startswith('hap_')]
print(f"Loaded {len(rows)} bars, {len(hap_cols)} curves: {hap_cols}")

fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), sharex=True)

colors = ['#2E86AB', '#E94F37', '#1B998B', '#F26419', '#9B5DE5']
style  = ['-', '--', '-.', ':', '-']

for i, col in enumerate(hap_cols):
    vals = [float(r[col]) for r in rows]
    bars = list(range(len(vals)))
    ax1.plot(bars, vals, color=colors[i % len(colors)], linestyle=style[i % len(style)],
            linewidth=1.5, label=col.replace('hap_', 'HAP='))

ax1.set_title('T95 HEDGE_ATR_PCT Sweep — Exact-Live Equity Curves\n(Baseline + Top Runners, Walk-Forward OOS)', fontsize=13)
ax1.set_ylabel('Equity (normalized, log scale)')
ax1.set_yscale('log')
ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.2f}x'))
ax1.grid(True, alpha=0.3)
ax1.legend(loc='upper left', fontsize=9)

# Drawdown panel.
for i, col in enumerate(hap_cols):
    vals = [float(r[col]) for r in rows]
    peak = vals[0]
    dds = []
    for v in vals:
        if v > peak: peak = v
        dds.append((peak - v) / peak * 100)
    ax2.plot(list(range(len(dds))), dds, color=colors[i % len(colors)],
             linestyle=style[i % len(style)], linewidth=1.5, label=col.replace('hap_', 'HAP='))

ax2.set_title('Drawdown (%)', fontsize=11)
ax2.set_ylabel('Drawdown (%)')
ax2.set_ylim(bottom=0)
ax2.yaxis.set_major_formatter(mticker.FuncFormatter(lambda v, _: f'{v:.0f}%'))
ax2.grid(True, alpha=0.3)
ax2.legend(loc='upper left', fontsize=9)

plt.tight_layout()
out = '/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png'
plt.savefig(out, dpi=150, bbox_inches='tight')
print(f"Chart saved: {out}")
"#;

    let chart_path = "/home/ubuntu/.openclaw/workspace-krypto/charts/plot_t95.py";
    File::create(chart_path)?.write_all(chart_script.as_bytes())?;
    println!("\nRunning chart script...");
    let result = std::process::Command::new("python3")
        .arg(chart_path)
        .current_dir("/home/ubuntu/.openclaw/workspace-krypto/krypto")
        .output();
    match result {
        Ok(out) => {
            println!("stdout: {}", String::from_utf8_lossy(&out.stdout));
            if !out.stderr.is_empty() {
                println!("stderr: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
        Err(e) => println!("Chart failed: {}", e),
    }

    // ── Mark best in source ───────────────────────────────────────────────────
    println!("\n=== BEST HEDGE_ATR_PCT = {:.2} (was 0.45) ===", best_hap);
    println!("Compare: baseline 0.45 vs winner {:.2}", best_hap);
    if (best_hap - 0.45).abs() > 0.001 {
        println!("UPDATE NEEDED: src/live/config.rs HEDGE_ATR_PCT");
    } else {
        println!("NO CHANGE: winner equals baseline (0.45)");
    }

    Ok(())
}

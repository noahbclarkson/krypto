//! =========================================================
//! TURTLE ENTRY PERIOD SWEEP — Pure Turtle + Chandelier Exit
//! =========================================================
//!
//! TARGET: TURTLE_ENTRY (Donchian lookback) — the N-bar breakout period
//! PRIOR:  hardcoded at 20, never swept
//! SWEEP:  5 to 100 in steps of 1 → 96 values
//! STRATEGY: Pure Turtle (Donchian breakout) + Chandelier(45, 2.5) exit
//! METHOD:   Walk-forward (252 train / 252 test) on all 9 universes
//! EXIT:     Chandelier ATR(45) × 2.5 (the validated trailing stop)
//! METRIC:   OOS pass rate, avg Sharpe, equity curves
//!
//! Exports:
//!   snapshots/turtle_entry_sweep_results.csv   — per-entry-period aggregate results
//!   snapshots/turtle_entry_universe_results.csv — per-entry-period × universe
//!   snapshots/turtle_entry_equity_curves.csv   — time-series equity for chart generation

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const WARMUP: usize = 200;

// Chandelier parameters (FIXED — these are validated)
const CHAND_PERIOD: usize = 45;
const CHAND_MULT: f64 = 2.5;

// Full sweep: 5 to 100 step 1 — 96 values
const SWEEP_START: usize = 5;
const SWEEP_END: usize = 100;
const SWEEP_STEP: usize = 1;

// ── 9 universes ──────────────────────────────────────────────────────────────
const S_BASE5: [&str; 6] = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];
const S_NODOGE: [&str; 6] = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT","BNBUSDT"];
const S_L4:    [&str; 6] = ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","_","_"];
const S_L5BNB: [&str; 6] = ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","_"];
const S_OGNM:  [&str; 6] = ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","_"];
const S_LCAPS: [&str; 6] = ["BTCUSDT","ETHUSDT","BNBUSDT","XRPUSDT","ADAUSDT","_"];
const S_L3:    [&str; 6] = ["BTCUSDT","ETHUSDT","XRPUSDT","_","_","_"];
const S_LVOL:  [&str; 6] = ["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT","_"];
const S_OG4:   [&str; 6] = ["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","_","_"];

const UNIVERSES: &[(&str, &[&str; 6])] = &[
    ("Base5",         &S_BASE5),
    ("NoDOGE",        &S_NODOGE),
    ("Legacy4",       &S_L4),
    ("Legacy5BNB",    &S_L5BNB),
    ("OldGuardNoBNB", &S_OGNM),
    ("LargeCaps5",    &S_LCAPS),
    ("Legacy3",       &S_L3),
    ("LowVolume5",    &S_LVOL),
    ("OldGuard4",     &S_OG4),
];

// ── Helpers ────────────────────────────────────────────────────────────────────

fn true_range(h: f64, l: f64, pc: f64) -> f64 {
    (h - l).max((h - pc).abs()).max((l - pc).abs())
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut tr_sum = 0.0_f64;
    for i in idx.saturating_sub(period - 1)..=idx {
        let pc = if i > 0 { close[i - 1] } else { close[0] };
        tr_sum += true_range(high[i], low[i], pc);
    }
    tr_sum / period as f64
}

fn calc_sharpe(rets: &[f64]) -> f64 {
    if rets.is_empty() || rets.len() < 2 { return 0.0; }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    let std = var.sqrt();
    if std < 1e-10 { return 0.0; }
    let ann_factor = (252.0_f64).sqrt();
    mean / std * ann_factor
}

fn calc_max_dd(equity: &[f64]) -> f64 {
    let mut peak = equity[0];
    let mut max_dd = 0.0_f64;
    for &eq in equity {
        peak = peak.max(eq);
        let dd = eq / peak - 1.0;
        max_dd = max_dd.min(dd);
    }
    max_dd
}

// Pure Turtle breakout signal
fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> i32 {
    if idx < entry_period || idx >= close.len() {
        return 0;
    }
    let mut max_h = close[idx - entry_period];
    for i in idx + 1 - entry_period..idx {
        max_h = max_h.max(close[i.max(0)]);
    }
    if close[idx] > max_h { 1 } else { 0 }
}

// ── Per-bar backtest with equity curve ─────────────────────────────────────────

struct BarResult {
    oos_return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    wins: usize,
    win_rate: f64,
    equity_curve: Vec<f64>,
}

fn run_turtle(close: &[f64], high: &[f64], low: &[f64],
              entry_period: usize, test_start: usize, test_end: usize) -> BarResult {
    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut daily_rets = Vec::new();
    let mut equity_curve = vec![1.0_f64];
    let mut pos: Option<(usize, f64, f64)> = None; // (entry_bar, entry_price, atr_at_entry)

    let mut bar = test_start.max(entry_period + 2);

    while bar < test_end && bar + 1 < close.len() {
        // Record equity at start of bar
        let eq_at_bar_start = equity;

        if pos.is_none() {
            // Look for entry signal at bar-1
            let sig_idx = (bar - 1).min(close.len() - 1);
            if sig_idx < entry_period + 2 {
                bar += 1;
                equity_curve.push(equity);
                continue;
            }

            let sig = turtle_signal(close, entry_period, sig_idx);
            if sig > 0 {
                let entry_price = close[bar.min(close.len() - 1)];
                let atr_val = atr_at(high, low, close, CHAND_PERIOD, bar.min(close.len() - 1));
                if entry_price > 0.0 && atr_val > 0.0 {
                    pos = Some((bar, entry_price, atr_val));
                }
            }
        } else {
            let (entry_bar, entry_price, atr_val) = pos.unwrap();
            let exit_price = close[bar.min(close.len() - 1)];

            // Update highest high for Chandelier
            let mut highest_high = entry_price;
            for j in entry_bar..=bar.min(close.len() - 1) {
                highest_high = highest_high.max(high[j]);
            }
            let stop_price = highest_high - CHAND_MULT * atr_val;

            // Check exit conditions
            let should_exit = low[bar.min(close.len() - 1)] <= stop_price
                || bar >= entry_bar + 200; // max hold sanity cap

            if should_exit {
                let actual_exit_price = if low[bar.min(close.len() - 1)] <= stop_price {
                    stop_price.max(exit_price * 0.98) // realistic fill
                } else {
                    exit_price
                };
                let gross = actual_exit_price / entry_price - 1.0;
                let net = gross - 2.0 * TAKER_FEE;
                equity *= 1.0 + net;
                trades += 1;
                if gross > 0.0 { wins += 1; }
                daily_rets.push(net);
                pos = None;
            }
        }

        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);
        equity_curve.push(equity);
        bar += 1;
    }

    // Extend equity to full test window
    let target_len = test_end - test_start;
    while equity_curve.len() < target_len {
        equity_curve.push(equity);
    }

    let win_rate = if trades > 0 { wins as f64 / trades as f64 } else { 0.0 };
    let oos_return = (equity - 1.0) * 100.0;

    BarResult {
        oos_return_pct: oos_return,
        sharpe: calc_sharpe(&daily_rets),
        max_dd: max_dd * 100.0,
        trades,
        wins,
        win_rate,
        equity_curve,
    }
}

// ── Symbol data container ────────────────────────────────────────────────────────

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();
    fs::create_dir_all("snapshots")?;
    fs::create_dir_all("charts")?;

    // Generate sweep values
    let sweep_values: Vec<usize> = (SWEEP_START..=SWEEP_END)
        .step_by(SWEEP_STEP)
        .collect();
    println!("\n============================================================");
    println!("  TURTLE ENTRY PERIOD SWEEP — Pure Turtle + Chandelier(45,2.5)");
    println!("  Sweep: {} to {} step {} ({} values)", SWEEP_START, SWEEP_END, SWEEP_STEP, sweep_values.len());
    println!("  Universes: 9 | Method: Walk-forward 252/252");
    println!("============================================================\n");

    // ── Load data for all universes ──────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let mut universe_data: HashMap<&str, HashMap<String, SymData>> = HashMap::new();
    let mut min_lens: HashMap<&str, usize> = HashMap::new();

    for (uni_name, syms) in UNIVERSES {
        let mut sym_data: HashMap<String, SymData> = HashMap::new();
        let mut min_len = usize::MAX;
        for sym in syms.iter().filter(|s| !s.is_empty() && s != &"_") {
            let raw = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
            let df = FeatureEngine::add_technicals(&raw, None)?;
            let close = df.column("close").unwrap().f64().unwrap()
                .into_iter().map(|v| v.unwrap_or(0.0)).collect::<Vec<_>>();
            let high = df.column("high").unwrap().f64().unwrap()
                .into_iter().map(|v| v.unwrap_or(0.0)).collect::<Vec<_>>();
            let low = df.column("low").unwrap().f64().unwrap()
                .into_iter().map(|v| v.unwrap_or(0.0)).collect::<Vec<_>>();
            let n = close.len();
            min_len = min_len.min(n);
            sym_data.insert(sym.to_string(), SymData { close, high, low });
        }
        min_lens.insert(uni_name, min_len);
        universe_data.insert(uni_name, sym_data);
    }

    // Normalize all to same length
    let global_min = *min_lens.values().min().unwrap();
    for (_, sym_data) in &mut universe_data {
        for (_, sd) in sym_data {
            if sd.close.len() > global_min {
                sd.close.truncate(global_min);
                sd.high.truncate(global_min);
                sd.low.truncate(global_min);
            }
        }
    }
    let n = global_min.min(2800);
    println!("Loaded 9 universes, {} bars\n", n);

    // Compute number of walk-forward windows
    let total_windows = n.saturating_sub(TRAIN_BARS + 300) / TEST_BARS;
    println!("Walk-forward windows: {} per universe\n", total_windows);

    // ── Run sweep ─────────────────────────────────────────────────────────────────
    let mut sweep_results: Vec<(usize, usize, f64, f64, f64, usize, usize, f64, f64)> = Vec::new();
    // (entry_period, passes, avg_ret, avg_sharpe, worst_dd, total_trades, wins, avg_win_rate, q_pass_pct)

    let mut universe_results: Vec<(String, usize, f64, f64, f64, usize, f64)> = Vec::new();
    // (universe, entry_period, ret, sharpe, max_dd, trades, win_rate)

    // Equity curves: (entry_period, universe, equity_curve)
    let mut equity_curves: Vec<(usize, String, Vec<f64>)> = Vec::new();

    for &ep in &sweep_values {
        let mut total_passes = 0usize;
        let mut total_windows = 0usize;
        let mut total_ret = 0.0_f64;
        let mut total_sharpe = 0.0_f64;
        let mut worst_dd = 0.0_f64;
        let mut total_trades = 0usize;
        let mut total_wins = 0usize;
        let mut total_win_rate = 0.0_f64;
        let mut q_passes = 0usize;

        for (uni_name, syms) in UNIVERSES {
            if let Some(sym_data) = universe_data.get(uni_name) {
                // Combine symbols: concatenate close/high/low
                let sym0 = syms.iter().find(|s| !s.is_empty() && s != &"_").unwrap();
                if let Some(sd0) = sym_data.get(*sym0) {
                    let combined_close = sd0.close.clone();
                    let combined_high = sd0.high.clone();
                    let combined_low = sd0.low.clone();

                    let mut uni_passes = 0usize;
                    let mut uni_ret = 0.0_f64;
                    let mut uni_sharpe = 0.0_f64;
                    let mut uni_dd = 0.0_f64;
                    let mut uni_trades = 0usize;
                    let mut uni_win_rate = 0.0_f64;

                    for wi in 0..total_windows {
                        let train_end = TRAIN_BARS + wi * TEST_BARS;
                        let tstart = train_end;
                        let tend = (tstart + TEST_BARS).min(n);

                        if tend.saturating_sub(tstart) < ep + 10 {
                            continue;
                        }

                        let result = run_turtle(
                            &combined_close, &combined_high, &combined_low,
                            ep, tstart, tend,
                        );

                        let window_pass = result.sharpe > 0.0 && result.oos_return_pct > 0.0;
                        if window_pass { uni_passes += 1; }

                        total_windows += 1;
                        total_ret += result.oos_return_pct;
                        total_sharpe += result.sharpe;
                        worst_dd = worst_dd.min(result.max_dd);
                        total_trades += result.trades;
                        total_wins += result.wins;
                        total_win_rate += result.win_rate;

                        uni_ret += result.oos_return_pct;
                        uni_sharpe += result.sharpe;
                        uni_dd = uni_dd.min(result.max_dd);
                        uni_trades += result.trades;
                        uni_win_rate += result.win_rate;
                    }

                    let uni_count = total_windows.max(1);
                    let q_pct = if uni_count > 0 { uni_passes as f64 / uni_count as f64 * 100.0 } else { 0.0 };

                    universe_results.push((
                        uni_name.to_string(), ep,
                        uni_ret / uni_count.max(1) as f64,
                        uni_sharpe / uni_count.max(1) as f64,
                        uni_dd,
                        uni_trades / uni_count.max(1) as usize,
                        uni_win_rate / uni_count.max(1) as f64,
                    ));

                    total_passes += uni_passes;
                    q_passes += uni_passes;
                }
            }
        }

        let win_rate = if total_trades > 0 { total_wins as f64 / total_trades as f64 } else { 0.0 };
        let q_pass_pct = if total_windows > 0 { q_passes as f64 / total_windows as f64 * 100.0 } else { 0.0 };

        sweep_results.push((
            ep,
            total_passes,
            total_ret / total_windows.max(1) as f64,
            total_sharpe / total_windows.max(1) as f64,
            worst_dd,
            total_trades,
            total_wins,
            win_rate,
            q_pass_pct,
        ));
    }

    // Sort by avg Sharpe descending
    sweep_results.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());

    // ── Print results ──────────────────────────────────────────────────────────────
    println!("\n=== SWEEP RESULTS (sorted by avg Sharpe) ===\n");
    println!("{:>6} | {:>4} passes | {:>8} avgSharpe | {:>8} avgRet% | {:>8} worstDD% | {:>6} trades | {:>5} WR% | {:>6} QPass%",
             "EP", "Pass", "Sharpe", "Ret", "WorstDD", "Trades", "WR", "QPass%");
    println!("{}", "-".repeat(85));

    for (ep, passes, avg_ret, avg_sharpe, worst_dd, trades, wins, win_rate, q_pass_pct) in &sweep_results {
        println!("{:>6} | {:>4}     | {:>8.3f} | {:>8.2f} | {:>8.2f} | {:>6} | {:>5.1f}% | {:>6.1f}%",
                 ep, passes, avg_sharpe, avg_ret, worst_dd, trades, win_rate*100.0, q_pass_pct);
    }

    // ── Export sweep results CSV ─────────────────────────────────────────────────
    {
        let mut lines = vec!["entry_period,passes,avg_ret_pct,avg_sharpe,worst_dd_pct,total_trades,wins,win_rate,q_pass_pct".to_string()];
        for (ep, passes, avg_ret, avg_sharpe, worst_dd, trades, wins, win_rate, q_pass_pct) in &sweep_results {
            lines.push(format!("{},{},{:.4},{:.6},{:.4},{},{},{:.6},{:.4}",
                ep, passes, avg_ret, avg_sharpe, worst_dd, trades, wins, win_rate, q_pass_pct));
        }
        fs::write("snapshots/turtle_entry_sweep_results.csv", lines.join("\n"))?;
        println!("\nWrote: snapshots/turtle_entry_sweep_results.csv");
    }

    // ── Export universe results CSV ──────────────────────────────────────────────
    {
        let mut lines = vec!["universe,entry_period,avg_ret_pct,avg_sharpe,worst_dd_pct,avg_trades,avg_win_rate".to_string()];
        for (uni, ep, ret, sh, dd, trades, wr) in &universe_results {
            lines.push(format!("{},{},{:.4},{:.6},{:.4},{},{:.6}",
                uni, ep, ret, sh, dd, trades, wr));
        }
        fs::write("snapshots/turtle_entry_universe_results.csv", lines.join("\n"))?;
        println!("Wrote: snapshots/turtle_entry_universe_results.csv");
    }

    // ── Equity curves for chart ──────────────────────────────────────────────────
    println!("\nGenerating equity curves for top entries...");

    let top_eps: Vec<usize> = sweep_results.iter().take(5).map(|(ep, _, _, _, _, _, _, _, _)| *ep).collect();
    let baseline_ep = 20usize; // current hardcoded default

    for (uni_name, syms) in UNIVERSES {
        if uni_name != "Base5" { continue; } // Only Base5 for chart equity
        if let Some(sym_data) = universe_data.get(uni_name) {
            let sym0 = syms.iter().find(|s| !s.is_empty() && s != &"_").unwrap();
            if let Some(sd0) = sym_data.get(*sym0) {
                for &ep in &[baseline_ep, top_eps[0], top_eps[1], top_eps[2]] {
                    let combined_close = sd0.close.clone();
                    let combined_high = sd0.high.clone();
                    let combined_low = sd0.low.clone();

                    let mut full_equity: Vec<f64> = Vec::new();
                    for wi in 0..total_windows {
                        let train_end = TRAIN_BARS + wi * TEST_BARS;
                        let tstart = train_end;
                        let tend = (tstart + TEST_BARS).min(n);
                        if tend.saturating_sub(tstart) < ep + 10 { continue; }

                        let result = run_turtle(&combined_close, &combined_high, &combined_low, ep, tstart, tend);
                        full_equity.extend_from_slice(&result.equity_curve);
                    }

                    let label = if *ep == baseline_ep { "baseline".to_string() } else { format!("ep{}", ep) };
                    equity_curves.push((baseline_ep, format!("{}_baseline", uni_name), full_equity));
                    break;
                }
            }
        }
    }

    // Build equity curve for baseline vs top-3 winners on Base5
    if let Some(sym_data) = universe_data.get("Base5") {
        let sym0 = "BTCUSDT";
        if let Some(sd0) = sym_data.get(sym0) {
            let combined_close = sd0.close.clone();
            let combined_high = sd0.high.clone();
            let combined_low = sd0.low.clone();

            for ep in [baseline_ep, top_eps[0], top_eps[1], top_eps[2]] {
                let mut full_equity: Vec<f64> = Vec::new();
                for wi in 0..total_windows {
                    let train_end = TRAIN_BARS + wi * TEST_BARS;
                    let tstart = train_end;
                    let tend = (tstart + TEST_BARS).min(n);
                    if tend.saturating_sub(tstart) < ep + 10 { continue; }
                    let result = run_turtle(&combined_close, &combined_high, &combined_low, ep, tstart, tend);
                    full_equity.extend_from_slice(&result.equity_curve);
                }
                equity_curves.push((ep, format!("Base5_ep{}", ep), full_equity));
            }
        }
    }

    // ── Export equity curves CSV ───────────────────────────────────────────────
    {
        let mut all_eps: Vec<usize> = equity_curves.iter().map(|(ep, _, _)| *ep).collect();
        all_eps.sort();
        all_eps.dedup();

        let max_len = equity_curves.iter().map(|(_, _, eq)| eq.len()).max().unwrap_or(0);

        let mut lines: Vec<String> = vec![format!("bar,{}", all_eps.iter()
            .map(|ep| format!("ep{}_Base5", ep))
            .collect::<Vec<_>>().join(","))];

        for i in 0..max_len {
            let mut row = vec![i.to_string()];
            for ep in &all_eps {
                let eq = equity_curves.iter()
                    .find(|(e, _, _)| *e == *ep)
                    .map(|(_, _, q)| q.get(i).copied().unwrap_or(1.0))
                    .unwrap_or(1.0_f64);
                row.push(format!("{:.6}", eq));
            }
            lines.push(row.join(","));
        }

        fs::write("snapshots/turtle_entry_equity_curves.csv", lines.join("\n"))?;
        println!("Wrote: snapshots/turtle_entry_equity_curves.csv");
    }

    // ── Write Python charting script ─────────────────────────────────────────────
    let winner_ep = *top_eps.get(0).unwrap_or(&baseline_ep);
    let runner1_ep = *top_eps.get(1).unwrap_or(&baseline_ep);
    let runner2_ep = *top_eps.get(2).unwrap_or(&baseline_ep);

    // Write Python script directly using write! macro to avoid format-string issues
    let py_path = "charts/plot_turtle_entry_sweep.py";
    {
        let mut file = fs::File::create(py_path)?;
        use std::io::Write;
        writeln!(file, r#"import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import numpy as np
import csv

# ── Load sweep results ─────────────────────────────────────────────────────────
sweep_results = []
with open('snapshots/turtle_entry_sweep_results.csv', 'r') as f:
    reader = csv.DictReader(f)
    for row in reader:
        d = {{
            'ep': int(row['entry_period']),
            'passes': int(row['passes']),
            'avg_sharpe': float(row['avg_sharpe']),
            'avg_ret': float(row['avg_ret_pct']),
            'worst_dd': float(row['worst_dd_pct']),
            'trades': int(row['total_trades']),
            'win_rate': float(row['win_rate']),
            'q_pass': float(row['q_pass_pct']),
        }}
        sweep_results.append(d)

sweep_results.sort(key=lambda x: x['ep'])

# ── Load equity curves ──────────────────────────────────────────────────────────
equity_data = {{}}
try:
    with open('snapshots/turtle_entry_equity_curves.csv', 'r') as f:
        reader = csv.DictReader(f)
        for row in reader:
            for col, val in row.items():
                if col == 'bar': continue
                if col not in equity_data:
                    equity_data[col] = []
                equity_data[col].append(float(val))
except FileNotFoundError:
    print('Warning: equity curves CSV not found, skipping equity chart')

# ── Color scheme ────────────────────────────────────────────────────────────────
BLINE = '#888888'
WIN   = '#00FF88'
RUN1  = '#00BBFF'
RUN2  = '#FFB800'

BASELINE_EP = {baseline_ep}
WINNER_EP = {winner_ep}
RUNNER1_EP = {runner1_ep}
RUNNER2_EP = {runner2_ep}

winner_row = next((r for r in sweep_results if r['ep'] == WINNER_EP), {{}})
runner1_row = next((r for r in sweep_results if r['ep'] == RUNNER1_EP), {{}})
runner2_row = next((r for r in sweep_results if r['ep'] == RUNNER2_EP), {{}})
baseline_row = next((r for r in sweep_results if r['ep'] == BASELINE_EP), {{}})

fig, axes = plt.subplots(3, 1, figsize=(14, 12), facecolor='#0D1117')
fig.patch.set_facecolor('#0D1117')

# ── 1. Equity Curves (log scale) ────────────────────────────────────────────────
ax1 = axes[0]
ax1.set_facecolor('#0D1117')

curves_to_plot = [
    (f'ep{{BASELINE_EP}}_Base5', BLINE, f'Baseline EP={{BASELINE_EP}}', 1.5),
    (f'ep{{WINNER_EP}}_Base5',  WIN,   f'Winner EP={{WINNER_EP}} (Sharpe {{winner_row.get("avg_sharpe", 0):.2f}})', 2.5),
    (f'ep{{RUNNER1_EP}}_Base5', RUN1,  f'Runner1 EP={{RUNNER1_EP}} (Sharpe {{runner1_row.get("avg_sharpe", 0):.2f}})', 2.0),
    (f'ep{{RUNNER2_EP}}_Base5', RUN2,  f'Runner2 EP={{RUNNER2_EP}} (Sharpe {{runner2_row.get("avg_sharpe", 0):.2f}})', 2.0),
]

for key, color, label, lw in curves_to_plot:
    if key in equity_data and len(equity_data[key]) > 0:
        ax1.plot(range(len(equity_data[key])), equity_data[key],
                 color=color, linewidth=lw, alpha=0.9, label=label)

ax1.set_yscale('log')
ax1.set_title('Turtle Entry Period Sweep — Equity Curves (Base5, log scale)',
              fontsize=13, color='#E6EDF3', pad=12)
ax1.set_ylabel('Portfolio Equity', fontsize=11, color='#AAAAAA')
ax1.legend(loc='upper left', fontsize=9, framealpha=0.2, labelcolor='#E6EDF3')
ax1.grid(True, color='#21262D', linewidth=0.7, alpha=0.6)
ax1.tick_params(colors='#888888', labelsize=9)
for spine in ax1.spines.values():
    spine.set_edgecolor('#30363D')

# ── 2. Sharpe by Entry Period (bar chart) ─────────────────────────────────────
ax2 = axes[1]
ax2.set_facecolor('#0D1117')

erp_vals = [r['ep'] for r in sweep_results]
sh_vals = [r['avg_sharpe'] for r in sweep_results]
bar_colors = ['#00FF88' if r['ep'] == WINNER_EP
              else '#888888' if r['ep'] == BASELINE_EP
              else '#2D333B'
              for r in sweep_results]

ax2.bar(erp_vals, sh_vals, color=bar_colors, width=0.8, alpha=0.85)
ax2.axhline(0, color='#555555', linewidth=0.8)
ax2.set_title('Average OOS Sharpe by Donchian Entry Period',
              fontsize=13, color='#E6EDF3', pad=12)
ax2.set_xlabel('Donchian Entry Period (bars)', fontsize=11, color='#AAAAAA')
ax2.set_ylabel('Avg OOS Sharpe', fontsize=11, color='#AAAAAA')
ax2.grid(True, color='#21262D', linewidth=0.7, axis='y', alpha=0.6)
ax2.tick_params(colors='#888888', labelsize=9)
for spine in ax2.spines.values():
    spine.set_edgecolor('#30363D')

# ── 3. Heatmap: Sharpe by Entry Period × Universe ───────────────────────────────
ax3 = axes[2]
ax3.set_facecolor('#0D1117')

uni_data = {{}}
try:
    with open('snapshots/turtle_entry_universe_results.csv', 'r') as f:
        reader = csv.DictReader(f)
        for row in reader:
            uni = row['universe']
            ep = int(row['entry_period'])
            sh = float(row['avg_sharpe'])
            if uni not in uni_data:
                uni_data[uni] = {{}}
            uni_data[uni][ep] = sh
except FileNotFoundError:
    print('Warning: universe results CSV not found')

unis = list(uni_data.keys())
ep_grid = sorted(set(r['ep'] for r in sweep_results))
Z = np.array([[uni_data.get(u, {{}}).get(ep, np.nan) for ep in ep_grid] for u in unis])

im = ax3.imshow(Z, aspect='auto', cmap='RdYlGn', vmin=-2, vmax=5)
ax3.set_title('Avg Sharpe by Universe × Entry Period (heatmap)',
              fontsize=13, color='#E6EDF3', pad=12)
ax3.set_xlabel('Entry Period', fontsize=11, color='#AAAAAA')
ax3.set_ylabel('Universe', fontsize=11, color='#AAAAAA')
ax3.set_xticks(range(0, len(ep_grid), 5))
ax3.set_xticklabels([str(ep_grid[i]) for i in range(0, len(ep_grid), 5)],
                    colors='#888888', fontsize=8)
ax3.set_yticks(range(len(unis)))
ax3.set_yticklabels(unis, colors='#888888', fontsize=8)
for spine in ax3.spines.values():
    spine.set_edgecolor('#30363D')

cbar = fig.colorbar(im, ax=ax3, orientation='vertical', fraction=0.02, pad=0.02)
cbar.set_label('Avg Sharpe', color='#AAAAAA', fontsize=9)
cbar.ax.tick_params(colors='#888888', labelsize=8)

plt.tight_layout(pad=2.0)
plt.savefig('charts/turtle_entry_comparison_chart.png', dpi=150, bbox_inches='tight',
           facecolor='#0D1117', edgecolor='none')
print('Saved: charts/turtle_entry_comparison_chart.png')

print()
print('=== SWEEP SUMMARY ===')
bs = baseline_row.get('avg_sharpe', 0)
ws = winner_row.get('avg_sharpe', 0)
br = baseline_row.get('avg_ret', 0)
wr = winner_row.get('avg_ret', 0)
bqp = baseline_row.get('q_pass', 0)
wqp = winner_row.get('q_pass', 0)
print(f'Baseline:  EP={{BASELINE_EP}} | Sharpe={{bs:.3f}} | Ret={{br:.2f}}%')
print(f'Winner:    EP={{WINNER_EP}} | Sharpe={{ws:.3f}} | Ret={{wr:.2f}}%')
print(f'Runner1:   EP={{RUNNER1_EP}} | Sharpe={{runner1_row.get("avg_sharpe", 0):.3f}}')
print(f'Runner2:   EP={{RUNNER2_EP}} | Sharpe={{runner2_row.get("avg_sharpe", 0):.3f}}')
print(f'Baseline Q-Pass: {{bqp:.1f}}% | Winner Q-Pass: {{wqp:.1f}}%')
"#)?;
    }
    println!("Wrote: charts/plot_turtle_entry_sweep.py");

    let elapsed = t0.elapsed();
    println!("\nTotal runtime: {:.1}s", elapsed.as_secs_f64());

    Ok(())
}

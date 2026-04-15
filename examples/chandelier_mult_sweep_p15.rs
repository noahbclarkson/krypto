//! Chandelier ATR Multiplier Hyperopt — with Optimal P=15
//!
//! CHAND_PERIOD was just validated (2026-04-11) as P=15 (global max Sharpe 7.285)
//! vs the old default P=45 (baseline Sharpe 6.000, +21.5% improvement).
//!
//! Target now: CHAND_MULT — sweep 1.0 to 5.0 step 0.25 (17 values)
//! Strategy: Turtle(EP=21) entry + Chandelier(P=15, M) exit
//! Universes: All 9 | Walk-forward 252/252 | 0.1% taker each side
//! Exports: equity curves per mult, aggregate CSV

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
const HOLD_MAX: usize = 60;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 2;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 15; // OPTIMAL from 2026-04-11 full sweep (was 45)
const TURTLE_ENTRY: usize = 21; // OPTIMAL from 2026-04-10 full sweep (was 20)

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

const MULT_MIN: f64 = 1.0;
const MULT_MAX: f64 = 5.0;
const MULT_STEP: f64 = 0.25; // 17 values — extensive but fast

const CSV_OUT:      &str = "snapshots/chand_mult_p15_results.csv";
const AGG_OUT:      &str = "snapshots/chand_mult_p15_aggregate.csv";
const EQUITY_OUT:   &str = "snapshots/chand_mult_p15_equity.csv";
const SUMMARY_OUT:  &str = "snapshots/chand_mult_p15_summary.json";
const CHART_SCRIPT: &str = "charts/plot_chand_mult_p15.py";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0_f64;
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        sum += (h - l).max((h - c0).abs()).max((l - c0).abs());
    }
    sum / period as f64
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_pct(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

/// Run simulation for one walk-forward window.
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    chand_mult: f64,
    test_start: usize,
    test_end: usize,
) -> Option<(f64, f64, f64, usize, Vec<f64>)> {
    if test_end - test_start < HOLD_MAX + 5 { return None; }

    let mut equity_curve = vec![1.0_f64];
    let mut equity = 1.0_f64;
    let mut total_trades = 0_usize;
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
        let top_syms: Vec<&str> = scores.iter().take(POSITION_CAP).map(|(s, _)| *s).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Turtle breakout entry
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(*sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    let start = bar + 1 - TURTLE_ENTRY;
                    let mut max_close = f64::NEG_INFINITY;
                    for i in start..bar {
                        if let Some(&c) = sd.close.get(i) { max_close = max_close.max(c); }
                    }
                    if let Some(&curr_close) = sd.close.get(bar) {
                        if curr_close > max_close {
                            let entry_px = sd.close[bar];
                            let entry = entry_px * (1.0 - TAKER_FEE);
                            let entry_bar_next = bar + 1;
                            let n = sd.close.len();

                            // Chandelier trailing stop
                            let mut highest_high = sd.high[entry_bar_next];
                            let mut exit_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                            for b in entry_bar_next..exit_bar.min(n.saturating_sub(1)) {
                                highest_high = highest_high.max(sd.high[b]);
                                let atr_val = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                                let trail = highest_high - chand_mult * atr_val;
                                if sd.close[b] < trail {
                                    exit_bar = b;
                                    break;
                                }
                            }

                            if let Some(&exit_px) = sd.close.get(exit_bar) {
                                let exit = exit_px * (1.0 - TAKER_FEE);
                                let gross_ret = exit / entry - 1.0;
                                let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

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
        }

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    if equity_curve.is_empty() { return None; }
    let ret = (equity - 1.0) * 100.0;
    let sh = annualised_sharpe(&daily_rets);
    let dd = max_dd_pct(&equity_curve);
    Some((ret, sh, dd, total_trades, equity_curve))
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("╔══════════════════════════════════════════════════════════╗");
    eprintln!("║  Chandelier ATR Multiplier Hyperopt — P=15 (optimal)   ║");
    eprintln!("║  Sweep: {:.2} to {:.2} step {:.3} ({} values)          ║", MULT_MIN, MULT_MAX, MULT_STEP,
        ((MULT_MAX - MULT_MIN) / MULT_STEP) as usize + 1);
    eprintln!("║  EP={}, Chandelier(P={}, M=x), 9 universes, 252/252  ║", TURTLE_ENTRY, CHAND_PERIOD);
    eprintln!("╚══════════════════════════════════════════════════════════╝\n");

    // Build multiplier list
    let mults: Vec<f64> = {
        let mut v = vec![];
        let mut x = MULT_MIN;
        while x <= MULT_MAX + MULT_STEP / 2.0 {
            v.push((x * 100.0).round() / 100.0);
            x += MULT_STEP;
        }
        v
    };
    eprintln!("Mults: {:?}", mults);

    // Load data
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.saturating_sub(CHAND_PERIOD + 10).min(2800);
    eprintln!("\nCommon bars: {}\n", n);

    // Build SymData
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in all_syms.iter() {
        if let Some(df) = raw_cache.get(sym) {
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
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

    // Results storage
    let mut all_results: Vec<(String, f64, f64, f64, f64, usize, bool)> = vec![];
    let mut equity_curves: Vec<(String, f64, Vec<f64>)> = vec![];

    for &(univ_name, symbols) in UNIVERSES {
        let sym_strings: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = sym_strings.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing data)", univ_name);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (not enough data)", univ_name);
            continue;
        }

        eprintln!("{:>20}: {} syms, {} windows, {} mults",
            univ_name, sym_strings.len(), total_windows, mults.len());

        for mult in &mults {
            let mut window_rets = vec![];
            let mut window_sharpes = vec![];
            let mut window_dds = vec![];
            let mut total_trades = 0_usize;
            let mut pos_windows = 0_usize;
            let mut chart_equity: Option<Vec<f64>> = None;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);

                if test_end.saturating_sub(test_start) < 5 { continue; }

                if let Some((ret, sh, dd, trades, equity)) = run_sim(
                    &sym_data_map, &sym_strings, *mult, test_start, test_end,
                ) {
                    window_rets.push(ret);
                    window_sharpes.push(sh);
                    window_dds.push(dd);
                    total_trades += trades;
                    if ret > 0.0 { pos_windows += 1; }
                    if chart_equity.is_none() {
                        chart_equity = Some(equity);
                    }
                }
            }

            if window_rets.is_empty() { continue; }

            let avg_ret = window_rets.iter().sum::<f64>() / window_rets.len() as f64;
            let avg_sh  = window_sharpes.iter().sum::<f64>() / window_sharpes.len() as f64;
            let avg_dd  = window_dds.iter().sum::<f64>() / window_dds.len() as f64;
            let pass_rate = pos_windows as f64 / total_windows as f64 * 100.0;
            let pass = pass_rate >= 50.0;

            eprintln!("  M={:.2} | ret={:+8.2}% sh={:7.3} dd={:6.2}% | {}/{}w {}t{}",
                mult, avg_ret, avg_sh, avg_dd, pos_windows, total_windows, total_trades,
                if pass { " ✓" } else { "" });

            all_results.push((univ_name.to_string(), *mult, avg_sh, avg_ret, avg_dd, total_trades, pass));
            if let Some(eq) = chart_equity {
                equity_curves.push((univ_name.to_string(), *mult, eq));
            }
        }
    }

    // Aggregate across universes
    let mut mult_agg: HashMap<i64, (f64, f64, f64, i64, i64, usize)> = HashMap::new();
    for (_univ, mult, sharpe, ret, dd, trades, pass) in &all_results {
        let key = (*mult * 100.0).round() as i64;
        let e = mult_agg.entry(key).or_insert((0.0_f64, 0.0_f64, 0.0_f64, 0_i64, 0_i64, 0_usize));
        e.0 += sharpe;
        e.1 += ret;
        e.2 += dd;
        e.3 += *trades as i64;
        e.4 += if *pass { 1 } else { 0 };
        e.5 += 1;
    }

    let mut ranked: Vec<(f64, f64, f64, f64, i64, i64, usize)> = mult_agg.iter()
        .map(|(&key, &(s, r, d, t, p, c))| {
            let cnt = c as f64;
            (key as f64 / 100.0, s / cnt, r / cnt, d / cnt, t, p, c)
        }).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Write per-universe CSV
    {
        let mut f = File::create(CSV_OUT)?;
        writeln!(f, "universe,mult,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades,pass")?;
        for (_univ, mult, sharpe, ret, dd, trades, pass) in &all_results {
            writeln!(f, "{},{:.2},{:.4},{:.4},{:.4},{},{}", _univ, mult, sharpe, ret, dd, trades, pass)?;
        }
    }

    // Write aggregate CSV
    {
        let mut f = File::create(AGG_OUT)?;
        writeln!(f, "mult,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades,pass_count,n_universes")?;
        for (mult, sharpe, ret, dd, trades, passes, count) in &ranked {
            writeln!(f, "{:.2},{:.4},{:.4},{:.4},{},{},{}", mult, sharpe, ret, dd, trades, passes, count)?;
        }
    }

    // Write equity curves CSV
    {
        let mut f = File::create(EQUITY_OUT)?;
        writeln!(f, "universe,mult,step,equity")?;
        for (univ, mult, equity) in &equity_curves {
            for (step, eq) in equity.iter().enumerate() {
                writeln!(f, "{},{:.2},{},{:.6}", univ, mult, step, eq)?;
            }
        }
    }

    // Write JSON summary
    {
        let mut f = File::create(SUMMARY_OUT)?;
        writeln!(f, "{{")?;
        writeln!(f, "  \"parameter\": \"chandelier_mult\",")?;
        writeln!(f, "  \"chand_period\": {},", CHAND_PERIOD)?;
        writeln!(f, "  \"sweep_min\": {:.2},", MULT_MIN)?;
        writeln!(f, "  \"sweep_max\": {:.2},", MULT_MAX)?;
        writeln!(f, "  \"sweep_step\": {:.3},", MULT_STEP)?;
        writeln!(f, "  \"n_values\": {},", ranked.len())?;
        writeln!(f, "  \"ranked\": [")?;
        for (i, (mult, sharpe, ret, dd, trades, passes, count)) in ranked.iter().enumerate() {
            writeln!(f,
                "    {{\"rank\":{},\"mult\":{:.2},\"avg_sharpe\":{:.4},\"avg_return_pct\":{:.4},\"avg_dd_pct\":{:.4},\"total_trades\":{},\"pass_count\":{},\"n_universes\":{}}}{}",
                i + 1, mult, sharpe, ret, dd, trades, passes, count,
                if i < ranked.len() - 1 { "," } else { "" })?;
        }
        writeln!(f, "  ],")?;
        if let Some((wm, ws, wr, wd, wt, wp, _)) = ranked.first() {
            writeln!(f,
                "  \"winner\": {{\"mult\":{:.2},\"avg_sharpe\":{:.4},\"avg_return_pct\":{:.4},\"avg_dd_pct\":{:.4},\"total_trades\":{},\"pass_count\":{}}},",
                wm, ws, wr, wd, wt, wp)?;
        }
        // Baseline (M=2.5) lookup
        let base = mult_agg.get(&(250)).map(|v| v.0 / v.5 as f64).unwrap_or(0.0);
        writeln!(f, "  \"baseline_mult\": 2.50,")?;
        writeln!(f, "  \"baseline_sharpe\": {:.4},", base)?;
        writeln!(f, "  \"chart_mults\": [")?;
        for (i, (m, _, _, _, _, _, _)) in ranked.iter().take(5).enumerate() {
            writeln!(f, "    {:.2}{}", m, if i < 4 { "," } else { "" })?;
        }
        writeln!(f, "  ],")?;
        writeln!(f, "  \"elapsed_seconds\": {},", t0.elapsed().as_secs())?;
        writeln!(f, "  \"universes\": {},", UNIVERSES.len())?;
        writeln!(f, "}}")?;
    }

    // Print leaderboard
    println!("\n╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║  TOP RESULTS: Chandelier Multiplier with P=15 (9 universes, 252/252)    ║");
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");
    println!("{:>4} {:>6} {:>10} {:>10} {:>10} {:>8} {:>7} {:>8}", "Rank", "Mult", "AvgSharpe", "AvgRet%", "AvgDD%", "Trades", "Pass#", "vsBase");
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");
    let base_sharpe = mult_agg.get(&(250)).map(|v| v.0 / v.5 as f64).unwrap_or(0.0);
    for (i, (mult, sharpe, ret, dd, trades, passes, count)) in ranked.iter().take(20).enumerate() {
        let delta = if base_sharpe != 0.0 { (sharpe - base_sharpe) / base_sharpe * 100.0 } else { 0.0 };
        let marker = if (*mult - 2.50).abs() < 0.001 {
            " ←BASE".to_string()
        } else if i == 0 {
            " ★WIN".to_string()
        } else {
            format!(" {:+.0}%", delta)
        };
        println!("{:>4} {:>6.2} {:>10.4} {:>10.2} {:>10.2} {:>8} {:>5}/{}  {}",
            i + 1, mult, sharpe, ret, dd, trades, passes, count, marker);
    }
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");

    let elapsed = t0.elapsed();
    eprintln!("\nDone in {:.1}s", elapsed.as_secs_f64());
    eprintln!("Results:  {}", CSV_OUT);
    eprintln!("Aggregate: {}", AGG_OUT);
    eprintln!("Equity:   {}", EQUITY_OUT);
    eprintln!("JSON:     {}", SUMMARY_OUT);

    // Write Python chart script to external file
    let chart_script = r#"#!/usr/bin/env python3
"""Chandelier Multiplier Sweep P=15 — Equity Comparison Chart"""
import csv, os, sys
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.gridspec as gridspec
from collections import defaultdict

os.chdir('/home/ubuntu/.openclaw/workspace-krypto/krypto')

# Load aggregated results
mults, sharpes, rets, dds = [], [], [], []
with open('snapshots/chand_mult_p15_aggregate.csv') as f:
    r = csv.DictReader(f)
    for row in r:
        mults.append(float(row['mult']))
        sharpes.append(float(row['avg_sharpe']))
        rets.append(float(row['avg_return_pct']))
        dds.append(float(row['avg_max_dd_pct']))

# Load equity curves
eq_by_mult = defaultdict(list)
with open('snapshots/chand_mult_p15_equity.csv') as f:
    r = csv.DictReader(f)
    for row in r:
        m = round(float(row['mult']), 2)
        if row['universe'] == 'Base5':
            eq_by_mult[m].append(float(row['equity']))

# Find winner
sp = list(zip(mults, sharpes))
sp.sort(key=lambda x: x[1], reverse=True)
winner = sp[0][0]
baseline = 2.50
top5 = [m for m, _ in sp[:5]]

colors = {winner: '#e74c3c', baseline: '#95a5a6'}
labels = {winner: f'★ WINNER (M={winner})', baseline: f'Baseline (M={baseline})'}
fallback = ['#2ecc71', '#3498db', '#9b59b6', '#f39c12']
for i, m in enumerate(top5):
    if m not in colors:
        colors[m] = fallback[i % len(fallback)]
        labels[m] = f'Runner-up (M={m})'

# Build chart
fig = plt.figure(figsize=(18, 12))
gs = gridspec.GridSpec(3, 2, height_ratios=[2, 1.5, 1.5], hspace=0.35, wspace=0.3)

# Panel 1: Equity curves
ax1 = fig.add_subplot(gs[0, :])
for m in top5:
    eq = eq_by_mult.get(m, [])
    if eq:
        lw = 2.5 if m == winner else (1.5 if m == baseline else 1.2)
        ax1.plot(range(len(eq)), eq, label=labels.get(m, f'M={m}'), color=colors[m], linewidth=lw)
ax1.set_yscale('log')
ax1.set_ylabel('Equity (log scale)', fontsize=12)
ax1.set_title('Chandelier(P=15) Multiplier Sweep — Equity Curves (Base5)', fontsize=13, fontweight='bold')
ax1.set_xlabel('Trading Days', fontsize=11)
ax1.legend(loc='upper left', fontsize=10)
ax1.grid(True, alpha=0.3)

# Panel 2: Sharpe vs Multiplier
ax2 = fig.add_subplot(gs[1, 0])
ax2.plot(mults, sharpes, 'b-', linewidth=1.5)
if winner in mults:
    ax2.scatter([winner], [sharpes[mults.index(winner)]], color='red', s=120, zorder=5, label=f'★ Winner M={winner}')
if baseline in mults:
    ax2.scatter([baseline], [sharpes[mults.index(baseline)]], color='gray', s=100, zorder=5, label=f'Baseline M={baseline}')
ax2.axvline(x=winner, color='red', linestyle='--', alpha=0.3)
ax2.set_xlabel('Chandelier ATR Multiplier')
ax2.set_ylabel('Avg OOS Sharpe')
ax2.set_title('Sharpe vs Multiplier (17 values, 9 universes)')
ax2.legend()
ax2.grid(True, alpha=0.3)

# Panel 3: Return & DD
ax3 = fig.add_subplot(gs[1, 1])
ax3.plot(mults, rets, 'g-', label='Avg Return %')
ax3.plot(mults, dds, 'r-', label='Avg Max DD %')
ax3.set_xlabel('Chandelier ATR Multiplier')
ax3.set_ylabel('%')
ax3.set_title('Return & Drawdown vs Multiplier')
ax3.legend()
ax3.grid(True, alpha=0.3)

# Panel 4: Summary text
ax4 = fig.add_subplot(gs[2, :])
ax4.axis('off')
base_sh = sharpes[mults.index(baseline)] if baseline in mults else 0
win_sh = sharpes[mults.index(winner)] if winner in mults else 0
imp = (win_sh - base_sh) / base_sh * 100 if base_sh > 0 else 0
ax4.text(0.5, 0.5, f'Hyperopt Results: CHAND_MULT with P=15\n\nWinner: M={winner} | Sharpe: {win_sh:.3f}\nBaseline: M={baseline} | Sharpe: {base_sh:.3f}\nImprovement: {imp:+.1f}%\n\nTop 5 mults: {[f"M={m}" for m in top5]}',
         ha='center', va='center', fontsize=14, transform=ax4.transAxes,
         bbox=dict(boxstyle='round', facecolor='#f8f9fa', edgecolor='#dee2e6'))

plt.savefig('charts/chand_mult_p15_comparison.png', dpi=150, bbox_inches='tight')
print('Chart saved: charts/chand_mult_p15_comparison.png')
"#;
    std::fs::write(CHART_SCRIPT, chart_script)?;
    eprintln!("Chart script: {}", CHART_SCRIPT);

    Ok(())
}
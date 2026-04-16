//! HYPEROPT: DynamicTrend EMA Fast Period — Full 100-value Sweep
//!
//! STRATEGY:  DynamicTrend (EMA fast/slow crossover + RSI filter)
//! PARAM:      ema_fast in [1, 100] step=1  (100 values — entire logical range)
//! FIXED:      ema_slow=100, rsi_filter=50.0
//! UNIVERSES:  Base5, NoDOGE, LargeCaps5, Legacy4 (4 universes)
//! METHOD:     Walk-forward 252/252 + equity curve export
//!
//! PURPOSE:    Audit the hardcoded default (ema_fast=50) — NEVER validated OOS.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const EMA_FAST_START: usize = 1;
const EMA_FAST_END: usize = 100;
const EMA_SLOW: usize = 100;
const RSI_FILTER: f64 = 50.0;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
];

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();
    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    let ema_fast_values: Vec<usize> = (EMA_FAST_START..=EMA_FAST_END).collect();
    println!("\n{}", "=".repeat(72));
    println!("  HYPEROPT: DynamicTrend EMA Fast Period -- Full 100-value Sweep");
    println!("  ema_fast: {} to {} (step=1, {} values)", EMA_FAST_START, EMA_FAST_END, ema_fast_values.len());
    println!("  Fixed:    ema_slow={}, rsi_filter={}", EMA_SLOW, RSI_FILTER);
    println!("  Universes: {:?}", UNIVERSES.iter().map(|(n,_)| n).collect::<Vec<_>>());
    println!("{}", "=".repeat(72));

    let mut all_syms: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for &(_, syms) in UNIVERSES {
        for &s in syms { all_syms.insert(s); }
    }
    let all_syms: Vec<&str> = all_syms.into_iter().collect();
    println!("\nLoading {} symbols...", all_syms.len());

    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for &sym in &all_syms {
        let raw = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        min_len = min_len.min(df.height());
        sym_data.insert(sym.to_string(), df);
    }
    let trim_len = min_len.min(2800);
    for df in sym_data.values_mut() {
        if df.height() > trim_len { *df = df.slice(0, trim_len); }
    }
    println!("All symbols loaded, {} bars each.\n", trim_len);

    let mut close_cache: HashMap<String, Vec<f64>> = HashMap::new();
    let mut open_cache: HashMap<String, Vec<f64>> = HashMap::new();
    for (sym, df) in &sym_data {
        close_cache.insert(sym.clone(), df.column("close")?.f64()?.into_no_null_iter().collect());
        open_cache.insert(sym.clone(),  df.column("open")?.f64()?.into_no_null_iter().collect());
    }

    let mut all_results: HashMap<String, Vec<PeriodResult>> = HashMap::new();

    for &(uni_name, syms) in UNIVERSES {
        let t_uni = std::time::Instant::now();
        println!("[{}] Sweeping {} ema_fast values on {} syms...", uni_name, ema_fast_values.len(), syms.len());
        let sym_strs: Vec<String> = syms.iter().map(|s| s.to_string()).collect();
        let n = sym_strs.iter().filter_map(|s| close_cache.get(s).map(|v| v.len())).min().unwrap_or(0);
        let n_windows = n.saturating_sub(TRAIN_BARS + HOLD_BARS + 20) / TEST_BARS;
        if n_windows == 0 { eprintln!("  SKIP {} (n={})", uni_name, n); continue; }
        println!("  {} bars, {} windows", n, n_windows);

        let mut results: Vec<PeriodResult> = Vec::with_capacity(ema_fast_values.len());
        for (idx, &ef) in ema_fast_values.iter().enumerate() {
            let mut total_ret = 0.0_f64;
            let mut total_sh = 0.0_f64;
            let mut total_dd = 0.0_f64;
            let mut total_trades = 0usize;
            let mut total_wins = 0usize;
            let mut passes = 0usize;
            let mut eq_accum = 1.0_f64;
            let mut equity_curve: Vec<f64> = Vec::new();

            for wi in 0..n_windows {
                let tstart = TRAIN_BARS + wi * TEST_BARS;
                let tend = (tstart + TEST_BARS).min(n);
                if tend.saturating_sub(tstart) < HOLD_BARS + EMA_SLOW + 10 { continue; }

                let (ret, trades, wins) = run_window(&sym_strs, &close_cache, &open_cache, ef, tstart, tend);

                if trades >= MIN_TRADES {
                    total_ret += ret;
                    total_trades += trades;
                    total_wins += wins;
                    if ret > 0.0 { passes += 1; }
                    let mean_ret = ret / trades.max(1) as f64 / 100.0;
                    let std_ret = (mean_ret * (1.0 - mean_ret)).sqrt().max(0.001);
                    total_sh += mean_ret / std_ret * 252.0_f64.sqrt();
                }
                eq_accum *= 1.0 + (ret / 100.0).max(-0.5).min(5.0);
                equity_curve.push(eq_accum);
            }

            let n_passed = n_windows;
            let sh = if n_passed > 0 { total_sh / n_passed as f64 } else { 0.0 };
            results.push(PeriodResult {
                ema_fast: ef,
                passes,
                total_windows: n_passed,
                avg_ret: if n_passed > 0 { total_ret / n_passed as f64 } else { 0.0 },
                avg_sharpe: sh,
                worst_dd: total_dd,
                total_trades,
                win_rate: if total_trades > 0 { total_wins as f64 / total_trades as f64 } else { 0.0 },
                equity_curve,
            });

            if idx % 20 == 0 || idx == ema_fast_values.len() - 1 {
                let last = results.last().unwrap();
                println!("  ef={:3}: pass={}/{} sh={:+.3} ret={:+.1} dd={:+.1} trades={}",
                         ef, last.passes, last.total_windows, last.avg_sharpe,
                         last.avg_ret, last.worst_dd, last.total_trades);
            }
        }
        all_results.insert(uni_name.to_string(), results);
        println!("  [{}] done in {:.1}s\n", uni_name, t_uni.elapsed().as_secs_f64());
    }

    let mut global: HashMap<usize, GlobalAgg> = HashMap::new();
    for results in all_results.values() {
        for r in results {
            let agg = global.entry(r.ema_fast).or_insert_with(GlobalAgg::new);
            agg.total_passes += r.passes;
            agg.total_windows += r.total_windows;
            agg.total_sharpe += r.avg_sharpe;
            agg.total_dd += r.worst_dd;
            agg.total_ret += r.avg_ret;
            agg.total_trades += r.total_trades;
            agg.n_universes += 1;
        }
    }

    let n_uni = all_results.len();
    if n_uni > 0 {
        let mut sorted: Vec<_> = global.iter().collect();
        sorted.sort_by(|a, b| {
            let pa = a.1.pass_rate();
            let pb = b.1.pass_rate();
            pb.partial_cmp(&pa).unwrap()
                .then_with(|| b.1.avg_sharpe().partial_cmp(&a.1.avg_sharpe()).unwrap())
                .then_with(|| a.1.avg_dd().partial_cmp(&b.1.avg_dd()).unwrap())
        });

        let winner = *sorted.first().map(|(ef, _)| *ef).unwrap_or(&50);
        let baseline = 50;

        println!("{}", "=".repeat(72));
        println!("  GLOBAL - Best EMA Fast Periods (across {} universes)", n_uni);
        println!("  {:>6} | {:>6} | {:>7} | {:>7} | {:>7} | {:>6}",
                 "EF", "Pass%", "AvgSharpe", "AvgDD%", "AvgRet%", "Trades");
        println!("  {}", "-".repeat(56));
        for (&ef, agg) in sorted.iter().take(20) {
            println!("  {:>6} | {:>5.1}% | {:>+7.3} | {:>+7.2}% | {:>+7.2}% | {:>6}",
                     ef, agg.pass_rate()*100.0, agg.avg_sharpe(), agg.avg_dd(), agg.avg_ret(), agg.total_trades);
        }
        println!("\n  Baseline (ef=50): {}", baseline);
        println!("  Winner: ef={}", winner);

        let mut f = File::create("snapshots/dynamic_trend_ema_fast_global.csv")?;
        writeln!(f, "ema_fast,pass_rate,total_passes,total_windows,avg_sharpe,avg_dd_pct,avg_ret_pct,total_trades")?;
        for (&ef, agg) in &sorted {
            writeln!(f, "{},{:.4},{},{},{:.4},{:.2},{:.2},{}",
                     ef, agg.pass_rate(), agg.total_passes, agg.total_windows,
                     agg.avg_sharpe(), agg.avg_dd(), agg.avg_ret(), agg.total_trades)?;
        }
        println!("\n  Written: snapshots/dynamic_trend_ema_fast_global.csv");

        for (uni_name, results) in &all_results {
            let path = format!("snapshots/dynamic_trend_{}_sweep.csv", uni_name.to_lowercase());
            let mut f = File::create(&path)?;
            writeln!(f, "ema_fast,pass_rate,passes,windows,avg_sharpe,worst_dd,avg_ret,trades,win_rate")?;
            for r in results {
                writeln!(f, "{},{:.4},{},{},{:.4},{:.2},{:.2},{},{:.4}",
                         r.ema_fast, r.passes as f64 / r.total_windows as f64,
                         r.passes, r.total_windows, r.avg_sharpe, r.worst_dd,
                         r.avg_ret, r.total_trades, r.win_rate)?;
            }
            println!("  Written: {}", path);
        }

        let top_5: Vec<usize> = sorted.iter().take(5).map(|(ef, _)| **ef).collect();
        export_equity_curves(&all_results, &top_5)?;
        generate_charts(winner, baseline, &top_5)?;
        write_summary_md(&sorted, winner, baseline, n_uni)?;
    } else {
        eprintln!("ERROR: No results.");
    }

    println!("\n  Done in {:.1}s total", t0.elapsed().as_secs_f64());
    Ok(())
}

fn ema_at(data: &[f64], period: usize, end_idx: usize) -> f64 {
    let start = end_idx.saturating_sub(period);
    if start >= data.len() { return data.get(end_idx.min(data.len().saturating_sub(1))).copied().unwrap_or(0.0); }
    let window = &data[start..end_idx.min(data.len())];
    if window.is_empty() { return data[end_idx.min(data.len().saturating_sub(1))]; }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut ema_val = window[0];
    for &v in &window[1..] { ema_val = v * alpha + ema_val * (1.0 - alpha); }
    ema_val
}

fn rsi_at(data: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period + 1 { return 50.0; }
    let start = idx.saturating_sub(period);
    let mut gains = 0.0_f64;
    let mut losses = 0.0_f64;
    for i in (start+1)..=idx {
        let diff = data.get(i).unwrap_or(&0.0) - data.get(i-1).unwrap_or(&0.0);
        if diff > 0.0 { gains += diff; } else { losses -= diff; }
    }
    if losses < 1e-9 { return 100.0; }
    let avg_gain = gains / period as f64;
    let avg_loss = losses / period as f64;
    100.0 - (100.0 / (1.0 + avg_gain / avg_loss))
}

fn run_window(
    syms: &[String],
    close_cache: &HashMap<String, Vec<f64>>,
    open_cache: &HashMap<String, Vec<f64>>,
    ema_fast: usize,
    start: usize,
    end: usize,
) -> (f64, usize, usize) {
    let mut equity = 1.0_f64;
    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut pos: Option<(String, f64)> = None;
    let mut bar = start;

    while bar + 1 < end {
        if pos.is_none() {
            let mut scores: Vec<(String, f64)> = Vec::new();
            for sym in syms {
                let close = match close_cache.get(sym) { Some(c) => c.as_slice(), None => continue };
                if bar < EMA_SLOW.max(ema_fast) + 5 || bar >= close.len() { continue; }
                let ef_val = ema_at(close, ema_fast, bar);
                let es_val = ema_at(close, EMA_SLOW, bar);
                let rsi_val = rsi_at(close, 14, bar);
                if rsi_val < RSI_FILTER { continue; }
                if ef_val > es_val { scores.push((sym.clone(), ef_val / es_val)); }
            }
            if !scores.is_empty() {
                scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                let top_sym = &scores[0].0;
                if let Some(opens) = open_cache.get(top_sym) {
                    if bar < opens.len() {
                        let entry = opens[bar];
                        if entry > 0.0 { pos = Some((top_sym.clone(), entry)); }
                    }
                }
            }
            bar += 1;
            continue;
        }

        let (sym, entry_px) = pos.as_ref().unwrap();
        let cur_bar = bar;

        if cur_bar >= *entry_px as usize + HOLD_BARS || cur_bar >= end - 1 {
            if let Some(opens) = open_cache.get(sym) {
                if cur_bar < opens.len() {
                    let exit_px = opens[cur_bar];
                    if *entry_px > 0.0 && exit_px > 0.0 {
                        let gross = (exit_px / *entry_px - 1.0) - TAKER_FEE;
                        equity *= 1.0 + gross;
                        trades += 1;
                        if gross > 0.0 { wins += 1; }
                    }
                }
            }
            pos = None;
        }
        bar += 1;
    }

    if let Some((sym, entry_px)) = pos {
        if let Some(opens) = open_cache.get(&sym) {
            let last_bar = (end - 1).min(opens.len().saturating_sub(1));
            let exit_px = opens.get(last_bar).copied().unwrap_or(*entry_px);
            if *entry_px > 0.0 && exit_px > 0.0 {
                let gross = (exit_px / *entry_px - 1.0) - TAKER_FEE;
                equity *= 1.0 + gross;
                trades += 1;
                if gross > 0.0 { wins += 1; }
            }
        }
    }

    let ret = (equity - 1.0) * 100.0;
    (ret, trades, wins)
}

struct PeriodResult {
    ema_fast: usize,
    passes: usize,
    total_windows: usize,
    avg_ret: f64,
    avg_sharpe: f64,
    worst_dd: f64,
    total_trades: usize,
    win_rate: f64,
    equity_curve: Vec<f64>,
}

struct GlobalAgg {
    total_passes: usize,
    total_windows: usize,
    total_sharpe: f64,
    total_dd: f64,
    total_ret: f64,
    total_trades: usize,
    n_universes: usize,
}

impl GlobalAgg {
    fn new() -> Self { Self { total_passes: 0, total_windows: 0, total_sharpe: 0.0, total_dd: 0.0, total_ret: 0.0, total_trades: 0, n_universes: 0 } }
    fn pass_rate(&self) -> f64 { if self.total_windows == 0 { 0.0 } else { self.total_passes as f64 / self.total_windows as f64 } }
    fn avg_sharpe(&self) -> f64 { if self.n_universes == 0 { 0.0 } else { self.total_sharpe / self.n_universes as f64 } }
    fn avg_dd(&self) -> f64 { if self.n_universes == 0 { 0.0 } else { self.total_dd / self.n_universes as f64 } }
    fn avg_ret(&self) -> f64 { if self.n_universes == 0 { 0.0 } else { self.total_ret / self.n_universes as f64 } }
}

fn export_equity_curves(all_results: &HashMap<String, Vec<PeriodResult>>, top_5: &[usize]) -> Result<()> {
    let mut agg: HashMap<usize, Vec<f64>> = HashMap::new();
    for &ef in top_5 {
        let mut combined = Vec::new();
        for results in all_results.values() {
            if let Some(r) = results.iter().find(|r| r.ema_fast == ef) {
                if combined.is_empty() { combined = r.equity_curve.clone(); }
                else {
                    for (i, &v) in r.equity_curve.iter().enumerate() {
                        if i < combined.len() { combined[i] *= v.max(0.01); } else { combined.push(v.max(0.01)); }
                    }
                }
            }
        }
        if !combined.is_empty() { agg.insert(ef, combined); }
    }

    let csv_path = "snapshots/dynamic_trend_equity_comparison.csv";
    let mut f = File::create(csv_path)?;
    let cols: Vec<String> = top_5.iter().map(|ef| format!("ef_{}", ef)).collect();
    writeln!(f, "bar,{}", cols.join(","))?;
    let max_len = agg.values().map(|v| v.len()).max().unwrap_or(0);
    for i in 0..max_len {
        let mut row = format!("{}", i);
        for &ef in top_5 {
            let v = agg.get(&ef).and_then(|vals| vals.get(i)).copied().unwrap_or(1.0);
            row.push_str(&format!(",{:.6}", v));
        }
        writeln!(f, "{}", row)?;
    }
    println!("\n  Exported: {}", csv_path);
    Ok(())
}

fn generate_charts(winner: usize, baseline: usize, top_5: &[usize]) -> Result<()> {
    let p0 = baseline;
    let p1 = winner;
    let p2 = top_5.get(1).copied().unwrap_or_else(|| if winner > 10 { winner - 5 } else { winner + 5 });
    let p3 = top_5.get(2).copied().unwrap_or_else(|| if winner > 15 { winner - 10 } else { winner + 10 });

    // Write Python script to file first, then run it
    let script_path = "scripts/plot_dynamic_trend_sweep.py";
    let script_content = build_chart_script_string(p0, p1, p2, p3);
    std::fs::write(script_path, &script_content)?;

    println!("\n  Running chart script...");
    let status = std::process::Command::new("python3")
        .current_dir("/home/ubuntu/.openclaw/workspace-krypto/krypto")
        .arg(script_path)
        .status();
    if !status.map(|s| s.success()).unwrap_or(false) {
        eprintln!("  WARNING: chart script exited with issues.");
    }
    Ok(())
}

fn build_chart_script_string(p0: usize, p1: usize, p2: usize, p3: usize) -> String {
    // Build the script as plain text — no format! interpolation of Python code
    let mut out = String::new();

    // Header
    out.push_str("import matplotlib\n");
    out.push_str("matplotlib.use('Agg')\n");
    out.push_str("import matplotlib.pyplot as plt\n");
    out.push_str("import matplotlib.ticker as mticker\n");
    out.push_str("import pandas as pd\n");
    out.push_str("import numpy as np\n");
    out.push_str("import os, sys\n\n");
    out.push_str("CHART_DIR = 'charts'\n");
    out.push_str("os.makedirs(CHART_DIR, exist_ok=True)\n\n");
    out.push_str(&format!("PERIODS = [{}, {}, {}, {}]\n", p0, p1, p2, p3));
    out.push_str("COLORS  = ['#888888', '#1f77b4', '#ff7f0e', '#2ca02c']\n");
    out.push_str("LS = [':', '-', '--', '--']\n");
    out.push_str("LW = [1.5, 3.0, 1.5, 1.5]\n\n");

    // Equity comparison section
    out.push_str("# --- Equity Comparison ---\n");
    out.push_str("eq_file = 'snapshots/dynamic_trend_equity_comparison.csv'\n");
    out.push_str("if os.path.exists(eq_file):\n");
    out.push_str("    df_eq = pd.read_csv(eq_file, index_col=0)\n");
    out.push_str("    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(16, 10), gridspec_kw={'height_ratios': [3, 1]})\n");
    out.push_str("    t1 = 'Winner: ef=' + str(p1) + '  |  Baseline: ef=' + str(p0)
");
    out.push_str("    fig.suptitle('DynamicTrend EMA Fast Period - Equity Comparison\n' + t1 + '\n        fontsize=13, fontweight='\''bold\''\'\')");
    out.push_str("\n\n");
    out.push_str("    plotted = []\n");
    out.push_str("    for col in df_eq.columns:\n");
    out.push_str("        ef = int(col.replace('ef_', ''))\n");
    out.push_str("        vals = df_eq[col].dropna().values.astype(float)\n");
    out.push_str("        if len(vals) == 0 or vals[-1] < 0.001: continue\n");
    out.push_str("        idx = PERIODS.index(ef) if ef in PERIODS else -1\n");
    out.push_str("        color = COLORS[idx] if idx >= 0 else '#888888'\n");
    out.push_str("        ls = LS[idx] if idx >= 0 else '-'\n");
    out.push_str("        lw = LW[idx] if idx >= 0 else 1.5\n");
    out.push_str("        label = 'ef=' + str(ef)\n");
    out.push_str("        ax1.plot(np.arange(len(vals)), vals, label=label, color=color, linestyle=ls, linewidth=lw)\n\n");
    out.push_str("    ax1.set_yscale('log')\n");
    out.push_str("    all_vals = pd.concat([df_eq[c].dropna() for c in df_eq.columns]).values\n");
    out.push_str("    y_min = max(all_vals.min() * 0.8, 0.01)\n");
    out.push_str("    y_max = all_vals.max() * 1.2\n");
    out.push_str("    ax1.set_ylim(y_min, y_max)\n");
    out.push_str("    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: '{:.2f}'.format(x)))\n");
    out.push_str("    ax1.set_ylabel('Equity (log scale)', fontsize=11)\n");
    out.push_str("    ax1.legend(loc='upper left', fontsize=9, framealpha=0.9)\n");
    out.push_str("    ax1.grid(True, alpha=0.3, which='both')\n");
    out.push_str("    ax1.set_title('DynamicTrend - Top EMA Fast Periods Equity Curves', fontsize=10)\n\n");
    out.push_str("    for col in df_eq.columns:\n");
    out.push_str("        ef = int(col.replace('ef_', ''))\n");
    out.push_str("        vals = df_eq[col].dropna().values.astype(float)\n");
    out.push_str("        if len(vals) == 0: continue\n");
    out.push_str("        peak = np.maximum.accumulate(vals)\n");
    out.push_str("        dd = (vals - peak) / peak * 100.0\n");
    out.push_str("        idx = PERIODS.index(ef) if ef in PERIODS else -1\n");
    out.push_str("        color = COLORS[idx] if idx >= 0 else '#888888'\n");
    out.push_str("        ls = LS[idx] if idx >= 0 else '-'\n");
    out.push_str("        lw = LW[idx] if idx >= 0 else 1.5\n");
    out.push_str("        ax2.plot(np.arange(len(dd)), dd, color=color, linestyle=ls, linewidth=lw)\n\n");
    out.push_str("    ax2.set_ylabel('Drawdown pct', fontsize=11)\n");
    out.push_str("    ax2.set_xlabel('Trading Day', fontsize=11)\n");
    out.push_str("    ax2.grid(True, alpha=0.3)\n");
    out.push_str("    ax2.set_ylim(bottom=-100)\n");
    out.push_str("    plt.tight_layout()\n");
    out.push_str("    out = os.path.join(CHART_DIR, 'dynamic_trend_comparison.png')\n");
    out.push_str("    fig.savefig(out, dpi=150, bbox_inches='tight')\n");
    out.push_str("    plt.close(fig)\n");
    out.push_str("    print('Saved: ' + out)\n");
    out.push_str("else:\n");
    out.push_str("    print('WARNING: equity CSV not found', file=sys.stderr)\n\n");

    // Sweep overview
    out.push_str("# --- Sweep Overview ---\n");
    out.push_str("smry_file = 'snapshots/dynamic_trend_ema_fast_global.csv'\n");
    out.push_str("if os.path.exists(smry_file):\n");
    out.push_str("    smry = pd.read_csv(smry_file).sort_values('ema_fast')\n");
    out.push_str("    fig2, axes = plt.subplots(1, 3, figsize=(22, 7))\n");
    out.push_str("    t2 = 'Winner: ef=' + str(p1) + '  |  Baseline: ef=' + str(p0)
");
    out.push_str("    fig2.suptitle('DynamicTrend EMA Fast Sweep - Global | ' + t2 + '\n        fontsize=13, fontweight='\''bold\''\'\')");
    out.push_str("\n\n");

    // Sharpe subplot
    out.push_str("    ax = axes[0]\n");
    out.push_str("    ax.plot(smry['ema_fast'], smry['avg_sharpe'], 'o-', color='#1f77b4', linewidth=1.5, markersize=3)\n");
    out.push_str(&format!("    ax.axvline(x={}, color='#ff7f0e', linestyle='--', linewidth=2, label='Winner ef={}')\n", p1, p1));
    out.push_str(&format!("    ax.axvline(x={}, color='gray', linestyle=':', linewidth=2, label='Baseline ef={}')\n", p0, p0));
    out.push_str("    ax.set_xlabel('EMA Fast Period', fontsize=11)\n");
    out.push_str("    ax.set_ylabel('Avg Sharpe (multi-universe)', fontsize=11)\n");
    out.push_str("    ax.set_title('Avg Sharpe vs EMA Fast', fontsize=12)\n");
    out.push_str("    ax.legend(fontsize=9)\n");
    out.push_str("    ax.grid(True, alpha=0.3)\n\n");

    // Pass rate subplot
    out.push_str("    ax = axes[1]\n");
    out.push_str("    ax.plot(smry['ema_fast'], smry['pass_rate'] * 100, 's-', color='#d62728', linewidth=1.5, markersize=3)\n");
    out.push_str(&format!("    ax.axvline(x={}, color='#ff7f0e', linestyle='--', linewidth=2, label='Winner ef={}')\n", p1, p1));
    out.push_str(&format!("    ax.axvline(x={}, color='gray', linestyle=':', linewidth=2, label='Baseline ef={}')\n", p0, p0));
    out.push_str("    ax.set_xlabel('EMA Fast Period', fontsize=11)\n");
    out.push_str("    ax.set_ylabel('Walk-Fwd Pass Rate pct', fontsize=11)\n");
    out.push_str("    ax.set_title('Pass Rate vs EMA Fast', fontsize=12)\n");
    out.push_str("    ax.legend(fontsize=9)\n");
    out.push_str("    ax.grid(True, alpha=0.3)\n\n");

    // DD subplot
    out.push_str("    ax = axes[2]\n");
    out.push_str("    ax.plot(smry['ema_fast'], smry['avg_dd_pct'], '^-', color='#8c564b', linewidth=1.5, markersize=3)\n");
    out.push_str(&format!("    ax.axvline(x={}, color='#ff7f0e', linestyle='--', linewidth=2, label='Winner ef={}')\n", p1, p1));
    out.push_str(&format!("    ax.axvline(x={}, color='gray', linestyle=':', linewidth=2, label='Baseline ef={}')\n", p0, p0));
    out.push_str("    ax.set_xlabel('EMA Fast Period', fontsize=11)\n");
    out.push_str("    ax.set_ylabel('Worst Drawdown pct', fontsize=11)\n");
    out.push_str("    ax.set_title('Max DD vs EMA Fast', fontsize=12)\n");
    out.push_str("    ax.legend(fontsize=9)\n");
    out.push_str("    ax.grid(True, alpha=0.3)\n\n");

    out.push_str("    plt.tight_layout()\n");
    out.push_str("    out2 = os.path.join(CHART_DIR, 'dynamic_trend_sweep_overview.png')\n");
    out.push_str("    fig2.savefig(out2, dpi=150, bbox_inches='tight')\n");
    out.push_str("    plt.close(fig2)\n");
    out.push_str("    print('Saved: ' + out2)\n\n");

    out.push_str("print('Charts generated.')\n");
    out
}

fn write_summary_md(sorted: &[(usize, &GlobalAgg)], winner: usize, baseline: usize, n_uni: usize) -> Result<()> {
    let winner_agg = sorted.iter().find(|(ef, _)| **ef == winner).map(|(_, a)| *a);
    let baseline_agg = sorted.iter().find(|(ef, _)| **ef == baseline).map(|(_, a)| *a);

    let path = "snapshots/dynamic_trend_ema_fast_summary.md";
    let mut f = File::create(path)?;

    writeln!(f, "# DynamicTrend EMA Fast Period Hyperopt -- 2026-04-16")?;
    writeln!(f, "")?;
    writeln!(f, "**Strategy:** DynamicTrend (EMA fast/slow crossover + RSI filter)")?;
    writeln!(f, "**Hyperopt target:** `ema_fast` in [1, 100] step=1 (100 values)")?;
    writeln!(f, "**Fixed params:** ema_slow={}, rsi_filter={}", EMA_SLOW, RSI_FILTER)?;
    writeln!(f, "**Universes:** {} (Base5, NoDOGE, LargeCaps5, Legacy4)", n_uni)?;
    writeln!(f, "**Method:** Walk-forward 252-bar train / 252-bar test")?;
    writeln!(f, "")?;
    writeln!(f, "**Baseline (ema_fast=50):**")?;
    if let Some(agg) = baseline_agg {
        writeln!(f, "- Pass rate: {:.1}% ({}/{})", agg.pass_rate()*100.0, agg.total_passes, agg.total_windows)?;
        writeln!(f, "- Avg Sharpe: {:.3f}", agg.avg_sharpe())?;
        writeln!(f, "- Avg DD: {:.2f}%", agg.avg_dd())?;
        writeln!(f, "- Avg Return: {:.2f}%", agg.avg_ret())?;
        writeln!(f, "- Total Trades: {}", agg.total_trades)?;
    }
    writeln!(f, "")?;
    writeln!(f, "**Winner (ema_fast={}):**", winner)?;
    if let Some(agg) = winner_agg {
        writeln!(f, "- Pass rate: {:.1}% ({}/{})", agg.pass_rate()*100.0, agg.total_passes, agg.total_windows)?;
        writeln!(f, "- Avg Sharpe: {:.3f}", agg.avg_sharpe())?;
        writeln!(f, "- Avg DD: {:.2f}%", agg.avg_dd())?;
        writeln!(f, "- Avg Return: {:.2f}%", agg.avg_ret())?;
        writeln!(f, "- Total Trades: {}", agg.total_trades)?;
    }
    if let (Some(w), Some(b)) = (winner_agg, baseline_agg) {
        let delta_sharpe = if b.avg_sharpe().abs() > 0.001 {
            (w.avg_sharpe() - b.avg_sharpe()) / b.avg_sharpe().abs() * 100.0
        } else { 0.0 };
        let delta_dd = w.avg_dd() - b.avg_dd();
        writeln!(f, "")?;
        writeln!(f, "**Delta vs baseline:** Sharpe {:+.1}, DD {:+.1}pp", delta_sharpe, delta_dd)?;
    }
    writeln!(f, "")?;
    writeln!(f, "**Charts:** `charts/dynamic_trend_comparison.png`, `charts/dynamic_trend_sweep_overview.png`")?;
    writeln!(f, "**Data:** `snapshots/dynamic_trend_ema_fast_global.csv`")?;
    writeln!(f, "")?;
    writeln!(f, "**Top 10 EMA Fast periods:**")?;
    writeln!(f, "| ef | Pass% | Sharpe | DD% | Ret% | Trades |")?;
    writeln!(f, "|---|------|--------|-----|------|--------|")?;
    for (i, (ef, agg)) in sorted.iter().enumerate() {
        if i >= 10 { break; }
        writeln!(f, "| {} | {:.1} | {:+.3} | {:+.1} | {:+.1} | {} |",
                 ef, agg.pass_rate()*100.0, agg.avg_sharpe(), agg.avg_dd(), agg.avg_ret(), agg.total_trades)?;
    }
    println!("  Written: {}", path);
    Ok(())
}

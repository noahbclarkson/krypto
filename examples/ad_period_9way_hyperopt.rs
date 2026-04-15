//! =========================================================
//! HYPERPARAMETER OPTIMIZATION: A/D Momentum Period
//! =========================================================
//!
//! TARGET: A/D momentum lookback period — hardcoded at 20, never swept properly
//! SWEEP:  1 to 100 bars in steps of 1 → 100 values (FULL SWEEP)
//! UNIVERSES: All 9 harsh universes
//! METHOD:    Walk-forward 252/252 + 15 CPCV resamples
//! METRIC:    Chronology-first (quarter passes > resample passes > Sharpe)

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
const TOP_K: usize = 2;
const CPCV_RESAMPLES: usize = 15;

// FULL SWEEP: 1 to 100 in steps of 1 — 100 values
// Previously only tested 5-10 values coarsely. Now testing the entire logical range.
const PERIODS: [usize; 100] = const {
    let mut arr = [0usize; 100];
    let mut i = 0;
    while i < 100 {
        arr[i] = i + 1;
        i += 1;
    }
    arr
};

// Separate symbol constants to avoid type conflicts in array literals
const S_BASE5: [&str; 6] = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];
const S_NODOGE: [&str; 6] = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT","BNBUSDT"];
const S_L4:    [&str; 6] = ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","_","_"];
const S_L5BNB: [&str; 6] = ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","_"];
const S_OGNM:  [&str; 6] = ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","_"];
const S_LCAPS: [&str; 6] = ["BTCUSDT","ETHUSDT","BNBUSDT","XRPUSDT","ADAUSDT","_"];
const S_L3:    [&str; 6] = ["BTCUSDT","ETHUSDT","XRPUSDT","_","_","_"];
const S_LVOL:  [&str; 6] = ["BTCUSDT","ETHUSDT","XRPUSDT","EOSUSDT","_","_"];
const S_OG4:   [&str; 6] = ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","_","_"];

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

#[derive(Clone)]
struct PeriodResult {
    period: usize,
    quarter_passes: usize,
    windows: usize,
    resample_passes: usize,
    avg_ret: f64,
    avg_sharpe: f64,
    avg_dd: f64,
    total_trades: usize,
    wins: usize,
    equity_curve: Vec<f64>,
}

#[derive(Clone)]
struct UniverseResult {
    name: String,
    period_results: HashMap<usize, PeriodResult>,
    best_period: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();
    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    println!("\n============================================================");
    println!("  HYPEROPT: A/D Momentum Period — Full 100-value Sweep (1..100 step=1)");
    println!("  Periods: {:?}", PERIODS);
    println!("  Universes: {} | Method: Walk-forward + CPCV", UNIVERSES.len());
    println!("============================================================\n");

    // ── Load data for all universes ────────────────────────────────────────────
    let mut universe_data: HashMap<&str, HashMap<String, DataFrame>> = HashMap::new();
    let loader = DataLoader::new(None, None);

    for (uni_name, syms) in UNIVERSES {
        let mut cache: HashMap<String, DataFrame> = HashMap::new();
        let mut min_len = usize::MAX;

        for sym in syms.iter().flatten() {
            if sym.is_empty() { continue; }
            match loader.fetch_with_cache(sym, "1d", CANDLES).await {
                Ok(raw) => match FeatureEngine::add_technicals(&raw, None) {
                    Ok(df) => {
                        min_len = min_len.min(df.height());
                        cache.insert(sym.to_string(), df);
                    }
                    Err(e) => eprintln!("  WARNING: FeatureEngine failed for {sym}: {e}"),
                },
                Err(e) => eprintln!("  WARNING: Could not load {sym}: {e}"),
            }
        }

        if cache.is_empty() || min_len < TRAIN_BARS + TEST_BARS + HOLD_BARS + 10 {
            eprintln!("SKIP {uni_name}: insufficient data ({} bars)", min_len);
            continue;
        }

        let trim_len = min_len.min(2800);
        for (_, df) in &mut cache {
            if df.height() > trim_len { *df = df.slice(0, trim_len as i64); }
        }

        println!("  Loaded {uni_name}: {} syms, {} bars", cache.len(), trim_len);
        universe_data.insert(uni_name, cache);
    }

    let universe_names: Vec<&str> = universe_data.keys().cloned().collect();
    println!("\n{} universes ready. Starting sweep...\n", universe_names.len());

    // ── Run sweep ─────────────────────────────────────────────────────────────
    let mut all_universe_results: Vec<UniverseResult> = Vec::new();

    for uni_name in &universe_names {
        let cache = universe_data.get(uni_name as &str).unwrap();
        let syms: Vec<String> = cache.keys().cloned().collect();
        let n = cache.values().next().map(|df| df.height()).unwrap_or(0);

        let mut period_results: HashMap<usize, PeriodResult> = HashMap::new();
        for &period in &PERIODS {
            let result = run_period_backtest(cache, &syms, n, period);
            period_results.insert(period, result);
        }

        let best = period_results.values()
            .max_by(|a, b| {
                a.quarter_passes.cmp(&b.quarter_passes)
                    .then_with(|| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap())
            })
            .map(|r| r.period)
            .unwrap_or(20);

        println!("  [{:>15}] best period={:3}", uni_name, best);
        all_universe_results.push(UniverseResult {
            name: uni_name.to_string(),
            period_results,
            best_period: best,
        });
    }

    print_global_table(&all_universe_results);

    let (winner, runner1, runner2, baseline) = pick_top_periods(&all_universe_results);
    println!("\n============================================================");
    println!("  RESULT: winner=p={}  runner1=p={}  runner2=p={}  baseline=p=20",
        winner, runner1, runner2);
    println!("============================================================\n");

    export_comparison_curves(&universe_data, &universe_names,
        &[baseline, winner, runner1, runner2])?;
    write_summary_csv(&all_universe_results)?;
    write_detail_csv(&all_universe_results)?;

    let elapsed = t0.elapsed();
    println!("\nSweep done in {:.1}s.", elapsed.as_secs_f64());

    generate_chart(winner, runner1, runner2)?;

    println!("\nDone. Charts in charts/, CSVs in snapshots/.");
    Ok(())
}

// ─── Core backtest for one period on one universe ─────────────────────────────

fn run_period_backtest(
    cache: &HashMap<String, DataFrame>,
    syms: &[String],
    n: usize,
    period: usize,
) -> PeriodResult {
    let n_windows = n.saturating_sub(TRAIN_BARS + 42) / TEST_BARS;
    let mut total_ret = 0.0_f64;
    let mut total_sharpe = 0.0_f64;
    let mut total_dd = 0.0_f64;
    let mut total_trades = 0usize;
    let mut wins = 0usize;
    let mut quarter_passes = 0usize;
    let mut all_oos_trades: Vec<f64> = Vec::new();
    let mut equity_accum: Vec<f64> = Vec::new();
    let mut eq_bar = 1.0_f64;

    for wi in 0..n_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);

        if tend.saturating_sub(tstart) < HOLD_BARS + period + 2 { continue; }

        let (ret, sh, dd, trades, trade_rets, eq_slice) =
            run_ad_window(cache, syms, tstart, tend, period);

        all_oos_trades.extend(trade_rets);

        let passed = trades >= MIN_TRADES && ret > 0.0;
        if passed { quarter_passes += 1; }

        total_ret += ret;
        total_sharpe += sh;
        total_dd += dd;
        total_trades += trades;
        if ret > 0.0 { wins += 1; }

        for (ei, &r) in eq_slice.iter().enumerate() {
            eq_bar = eq_bar * (1.0 + r);
            equity_accum.push(eq_bar);
        }
    }

    let n_w = n_windows;
    let avg_ret = if n_w > 0 { total_ret / n_w as f64 } else { 0.0 };
    let avg_sharpe = if n_w > 0 { total_sharpe / n_w as f64 } else { 0.0 };
    let avg_dd = if n_w > 0 { total_dd / n_w as f64 } else { 0.0 };

    // CPCV resamples
    let mut resample_pos = 0usize;
    let nt = all_oos_trades.len();
    for ri in 0..CPCV_RESAMPLES {
        let mut eq = 1.0_f64;
        for j in 0..nt {
            let idx = (ri * 17 + j * 7) % nt.max(1);
            eq *= 1.0 + all_oos_trades[idx] - TAKER_FEE;
        }
        if eq > 1.0 { resample_pos += 1; }
    }

    PeriodResult {
        period, quarter_passes, windows: n_w, resample_passes: resample_pos,
        avg_ret, avg_sharpe, avg_dd, total_trades, wins,
        equity_curve: equity_accum,
    }
}

// ─── Single window A/D momentum backtest ─────────────────────────────────────

fn run_ad_window(
    cache: &HashMap<String, DataFrame>,
    syms: &[String],
    start: usize,
    end: usize,
    period: usize,
) -> (f64, f64, f64, usize, Vec<f64>, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut trade_rets: Vec<f64> = Vec::new();
    let mut equity_slice: Vec<f64> = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;
    let mut bar = start;

    while bar + 1 < end {
        if pos.is_none() {
            // ── Entry: rank symbols by A/D momentum ──────────────────────────
            let mut candidates: Vec<(String, f64)> = Vec::new();

            for sym in syms {
                let df = match cache.get(sym) {
                    Some(d) => d,
                    None => continue,
                };
                let n = df.height();
                let idx = bar.saturating_sub(1);
                if idx < period { break; }

                // Get OHLCV data using the working pattern
                let close_s = match df.column("close") {
                    Ok(s) => s,
                    Err(_) => { break; }
                };
                let close_ch = match close_s.f64() {
                    Ok(ch) => ch,
                    Err(_) => { break; }
                };
                let high_ch = df.column("high").unwrap().f64().unwrap();
                let low_ch  = df.column("low").unwrap().f64().unwrap();
                let vol_ch  = df.column("volume").unwrap().f64().unwrap();

                // Cumulative A/D up to idx
                let mut ad_now: f64 = 0.0;
                for i in 0..=idx {
                    let h = high_ch.get(i).unwrap_or(0.0);
                    let l = low_ch.get(i).unwrap_or(0.0);
                    let c = close_ch.get(i).unwrap_or(0.0);
                    let v = vol_ch.get(i).unwrap_or(0.0);
                    let range = h - l;
                    let mf = if range > 1e-9 { ((c - l) - (h - c)) / range * v } else { 0.0 };
                    ad_now += mf;
                }

                // A/D at idx-period (cumulative up to idx-period)
                let past_idx = idx.saturating_sub(period);
                let mut ad_past: f64 = 0.0;
                for i in 0..=past_idx {
                    let h = high_ch.get(i).unwrap_or(0.0);
                    let l = low_ch.get(i).unwrap_or(0.0);
                    let c = close_ch.get(i).unwrap_or(0.0);
                    let v = vol_ch.get(i).unwrap_or(0.0);
                    let range = h - l;
                    let mf = if range > 1e-9 { ((c - l) - (h - c)) / range * v } else { 0.0 };
                    ad_past += mf;
                }

                let mom = ad_now - ad_past;
                candidates.push((sym.clone(), mom));
            }

            candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let longs: Vec<_> = candidates.iter().filter(|(_, m)| *m > 0.0).take(TOP_K).collect();

            if !longs.is_empty() {
                let sym = &longs[0].0;
                if let Some(df) = cache.get(sym) {
                    let open_ch = df.column("open").unwrap().f64().unwrap();
                    if let Ok(entry_price) = open_ch.get(bar) {
                        if entry_price > 0.0 {
                            pos = Some((sym.clone(), bar, entry_price));
                        }
                    }
                }
            }

            equity_slice.push(equity - 1.0);
            bar += 1;
            continue;
        }

        // ── Hold: check exit ────────────────────────────────────────────────
        let (sym, entry_bar, entry_px) = pos.as_ref().unwrap();
        let df = match cache.get(sym) {
            Some(d) => d,
            None => { pos = None; bar += 1; continue; }
        };
        let n = df.height();
        let cur_bar = bar;

        if cur_bar >= *entry_bar + HOLD_BARS || cur_bar >= end.saturating_sub(1) {
            let close_ch = df.column("close").unwrap().f64().unwrap();
            let exit_px = close_ch.get(cur_bar.min(n.saturating_sub(1))).unwrap_or(*entry_px);
            if *entry_px > 0.0 && exit_px > 0.0 {
                let gross = (exit_px / entry_px - 1.0) - TAKER_FEE;
                equity *= 1.0 + gross;
                trade_rets.push(gross);
                trades += 1;
            }
            pos = None;
        }

        peak = peak.max(equity);
        max_dd = (equity / peak - 1.0).min(max_dd);
        equity_slice.push(equity - 1.0);
        bar += 1;
    }

    // Close open position at end
    if let Some((sym, _, entry_px)) = pos {
        if let Some(df) = cache.get(&sym) {
            let close_ch = df.column("close").unwrap().f64().unwrap();
            let end_idx = (end - 1).min(df.height().saturating_sub(1));
            let exit_px = close_ch.get(end_idx).unwrap_or(entry_px);
            if entry_px > 0.0 && exit_px > 0.0 {
                let gross = (exit_px / entry_px - 1.0) - TAKER_FEE;
                equity *= 1.0 + gross;
                trade_rets.push(gross);
                trades += 1;
            }
        }
    }

    peak = peak.max(equity);
    max_dd = (equity / peak - 1.0).min(max_dd);
    let ret = (equity - 1.0) * 100.0;

    let sh = if trade_rets.is_empty() {
        0.0_f64
    } else {
        let n = trade_rets.len() as f64;
        let mean = trade_rets.iter().sum::<f64>() / n;
        let var = trade_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
        let std = var.sqrt();
        if std < 1e-9 { 0.0_f64 } else { mean / std * (252.0_f64.sqrt()) }
    };

    (ret, sh, max_dd * 100.0, trades, trade_rets, equity_slice)
}

// ─── Print global comparison table ─────────────────────────────────────────────

struct PeriodAgg {
    period: usize,
    total_qp: usize,
    total_cp: usize,
    total_sharpe: f64,
    total_dd: f64,
}

fn print_global_table(all_universe_results: &[UniverseResult]) {
    let n_uni = all_universe_results.len();
    let max_quarter = UNIVERSES.len() * 4;

    println!("\n============================================================");
    println!("  GLOBAL SUMMARY — Period vs All 9 Universes");
    println!("============================================================");
    println!("{:>6} | {:>4}/{:>4} | {:>7} | {:>8} | {:>7}",
        "Period", "QP", "maxQP", "CPCV%", "AvgSharpe", "AvgDD%");
    println!("------------------------------------------------------------");

    let mut period_agg: HashMap<usize, PeriodAgg> = HashMap::new();
    for uni in all_universe_results {
        for (&period, result) in &uni.period_results {
            let e = period_agg.entry(period).or_insert_with(|| PeriodAgg {
                period, total_qp: 0, total_cp: 0, total_sharpe: 0.0, total_dd: 0.0,
            });
            e.total_qp += result.quarter_passes;
            e.total_cp += result.resample_passes;
            e.total_sharpe += result.avg_sharpe;
            e.total_dd += result.avg_dd;
        }
    }

    let mut sorted: Vec<_> = period_agg.values().collect();
    sorted.sort_by(|a, b| {
        b.total_qp.cmp(&a.total_qp)
            .then_with(|| b.total_cp.cmp(&a.total_cp))
            .then_with(|| b.total_sharpe.partial_cmp(&a.total_sharpe).unwrap())
    });

    for agg in sorted {
        let avg_sh = agg.total_sharpe / n_uni as f64;
        let avg_dd = agg.total_dd / n_uni as f64;
        let cp_pct = agg.total_cp as f64 / (n_uni as f64 * 4.0 * CPCV_RESAMPLES as f64) * 100.0;
        println!("{:>6} | {:>4}/{:>4} | {:>7.1}% | {:>+8.3} | {:>+7.2}",
            agg.period, agg.total_qp, max_quarter,
            cp_pct, avg_sh, avg_dd);
    }
}

fn pick_top_periods(all_universe_results: &[UniverseResult]) -> (usize, usize, usize, usize) {
    let n_uni = all_universe_results.len();
    let mut period_scores: HashMap<usize, (usize, usize, f64)> = HashMap::new();

    for uni in all_universe_results {
        for (&period, result) in &uni.period_results {
            let e = period_scores.entry(period).or_insert((0, 0, 0.0_f64));
            e.0 += result.quarter_passes;
            e.1 += result.resample_passes;
            e.2 += result.avg_sharpe;
        }
    }

    let mut sorted: Vec<_> = period_scores.iter().collect();
    sorted.sort_by(|a, b| {
        b.1.0.cmp(&a.1.0)
            .then_with(|| b.1.1.cmp(&a.1.1))
            .then_with(|| b.1.2.partial_cmp(&a.1.2).unwrap())
    });

    let winner   = *sorted.first().map(|(p, _)| *p).unwrap_or(&20);
    let runner1  = *sorted.get(1).map(|(p, _)| *p).unwrap_or(&25);
    let runner2  = *sorted.get(2).map(|(p, _)| *p).unwrap_or(&30);
    let baseline = 20;
    (winner, runner1, runner2, baseline)
}

// ─── Export equity curves ─────────────────────────────────────────────────────

fn export_comparison_curves(
    universe_data: &HashMap<&str, HashMap<String, DataFrame>>,
    universe_names: &[&str],
    periods: &[usize],
) -> Result<()> {
    for uni_name in universe_names {
        let cache = match universe_data.get(uni_name as &str) {
            Some(c) => c,
            None => continue,
        };
        let syms: Vec<String> = cache.keys().cloned().collect();
        let n = cache.values().next().map(|df| df.height()).unwrap_or(0);

        let csv_path = format!("snapshots/eqcurve_{}_pcomparison.csv", uni_name);
        let mut f = File::create(&csv_path)?;
        writeln!(f, "bar,{}", periods.iter().map(|p| format!("period_{}", p)).collect::<Vec<_>>().join(","))?;

        let mut period_eqs: Vec<Vec<f64>> = Vec::new();
        for &period in periods {
            let result = run_period_backtest(cache, &syms, n, period);
            period_eqs.push(result.equity_curve);
        }

        let max_len = period_eqs.iter().map(|v| v.len()).max().unwrap_or(0);
        for i in 0..max_len {
            let mut row = format!("{}", i);
            for peq in &period_eqs {
                let val = peq.get(i).copied()
                    .unwrap_or_else(|| if i == 0 { 1.0_f64 } else { *peq.last().unwrap_or(&1.0_f64) });
                row.push_str(&format!(",{:.6}", val));
            }
            writeln!(f, "{}", row)?;
        }
        println!("  Exported: {csv_path}");
    }
    Ok(())
}

// ─── Write CSVs ───────────────────────────────────────────────────────────────

fn write_summary_csv(all_universe_results: &[UniverseResult]) -> Result<()> {
    let path = "snapshots/ad_period_9way_summary.csv";
    let mut f = File::create(path)?;
    writeln!(f, "period,total_qp,max_qp,cpcv_pct,avg_sharpe,avg_dd_pct,total_trades")?;

    let n_uni = all_universe_results.len();
    let mut period_agg: HashMap<usize, PeriodAgg> = HashMap::new();
    for uni in all_universe_results {
        for (&period, result) in &uni.period_results {
            let e = period_agg.entry(period).or_insert_with(|| PeriodAgg {
                period, total_qp: 0, total_cp: 0, total_sharpe: 0.0, total_dd: 0.0,
            });
            e.total_qp += result.quarter_passes;
            e.total_cp += result.resample_passes;
            e.total_sharpe += result.avg_sharpe;
            e.total_dd += result.avg_dd;
        }
    }

    let max_qp = UNIVERSES.len() * 4;
    let max_cp = UNIVERSES.len() * 4 * CPCV_RESAMPLES;

    let mut sorted: Vec<_> = period_agg.values().collect();
    sorted.sort_by(|a, b| b.total_qp.cmp(&a.total_qp)
        .then_with(|| b.total_cp.cmp(&a.total_cp))
        .then_with(|| b.total_sharpe.partial_cmp(&a.total_sharpe).unwrap()));

    for agg in sorted {
        let cp_pct = agg.total_cp as f64 / max_cp as f64 * 100.0;
        let avg_sh = agg.total_sharpe / n_uni as f64;
        let avg_dd = agg.total_dd / n_uni as f64;
        writeln!(f, "{},{},{},{:.2},{:.3},{:.2},{}", agg.period, agg.total_qp, max_qp, cp_pct, avg_sh, avg_dd, agg.total_cp)?;
    }
    println!("  Written: snapshots/ad_period_9way_summary.csv");
    Ok(())
}

fn write_detail_csv(all_universe_results: &[UniverseResult]) -> Result<()> {
    let path = "snapshots/ad_period_9way_detail.csv";
    let mut f = File::create(path)?;
    writeln!(f, "universe,period,quarter_passes,windows,resample_passes,max_resamples,avg_sharpe,avg_dd_pct,total_trades,wins")?;

    for uni in all_universe_results {
        for (&period, result) in &uni.period_results {
            let max_cp = 4 * CPCV_RESAMPLES;
            writeln!(f, "{},{},{},{},{},{},{:.3},{:.2},{},{}",
                uni.name, period, result.quarter_passes, result.windows,
                result.resample_passes, max_cp,
                result.avg_sharpe, result.avg_dd, result.total_trades, result.wins)?;
        }
    }
    println!("  Written: snapshots/ad_period_9way_detail.csv");
    Ok(())
}

// ─── Generate Python comparison chart ─────────────────────────────────────────

fn generate_chart(winner: usize, runner1: usize, runner2: usize) -> Result<()> {
    let winner_s = winner.to_string();
    let runner1_s = runner1.to_string();
    let runner2_s = runner2.to_string();

    let script = format!(r##"
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd
import numpy as np
import os, glob, sys

CHART_DIR = 'charts'
os.makedirs(CHART_DIR, exist_ok=True)

PERIODS = [20, {ws}, {r1s}, {r2s}]
COLORS  = ['#aaaaaa', '#1f77b4', '#ff7f0e', '#2ca02c']
LABELS  = ['Baseline (p=20)', 'Winner (p={ws})', 'Runner-up (p={r1s})', 'Runner-up (p={r2s})']
LINESTYLES = ['-', '-', '--', '--']
LINEWIDTHS = [1.5, 3.0, 1.5, 1.5]

eq_files = sorted(glob.glob('snapshots/eqcurve_*_pcomparison.csv'))
if not eq_files:
    print("ERROR: No equity curve CSVs found. Run the sweep first.", file=sys.stderr)
    sys.exit(1)

for fpath in eq_files:
    uni = os.path.basename(fpath).replace('eqcurve_', '').replace('_pcomparison.csv', '')
    df = pd.read_csv(fpath, index_col=0)

    for col in df.columns:
        first_val = df[col].dropna().iloc[0] if not df[col].dropna().empty else 1.0
        if first_val != 0:
            df[col] = df[col] / first_val

    n_rows = df.shape[0]

    fig, axes = plt.subplots(2, 1, figsize=(16, 10), sharex=True,
                               gridspec_kw={{'height_ratios': [3, 1]}})
    fig.suptitle(
        'A/D Momentum Period Sweep — ' + uni + '\nEquity Curve (log) | Drawdown (linear)',
        fontsize=14, fontweight='bold')

    ax = axes[0]
    for col, color, label, ls, lw in zip(df.columns, COLORS, LABELS, LINESTYLES, LINEWIDTHS):
        vals = df[col].dropna().values.astype(float)
        if len(vals) == 0:
            continue
        x = np.arange(len(vals))
        ax.plot(x, vals, label=label, color=color, linestyle=ls, linewidth=lw)

    ax.set_yscale('log')
    ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: '{{:.2f}}'.format(x)))
    ax.set_ylabel('Equity (log scale)', fontsize=11)
    ax.legend(loc='upper left', fontsize=9, framealpha=0.9)
    ax.grid(True, alpha=0.3, which='both')
    ax.set_title('Top-4 Period Configs vs Baseline | Winner: p={}'.format({ws}), fontsize=10)

    ax2 = axes[1]
    for col, color, label, ls, lw in zip(df.columns, COLORS, LABELS, LINESTYLES, LINEWIDTHS):
        vals = df[col].dropna().values.astype(float)
        if len(vals) == 0:
            continue
        peak = np.maximum.accumulate(vals)
        dd = (vals - peak) / peak * 100.0
        x = np.arange(len(vals))
        ax2.plot(x, dd, label=label, color=color, linestyle=ls, linewidth=lw)

    ax2.set_ylabel('Drawdown %', fontsize=11)
    ax2.set_xlabel('Trading Day (walk-forward window)', fontsize=11)
    ax2.legend(loc='lower left', fontsize=8, framealpha=0.9)
    ax2.grid(True, alpha=0.3)
    ax2.set_ylim(bottom=-100)

    plt.tight_layout()
    out = os.path.join(CHART_DIR, 'ad_period_sweep_' + uni + '.png')
    fig.savefig(out, dpi=150, bbox_inches='tight')
    plt.close(fig)
    print('  Saved: ' + out)

# Aggregate summary
if os.path.exists('snapshots/ad_period_9way_summary.csv'):
    smry = pd.read_csv('snapshots/ad_period_9way_summary.csv')
    fig2, axes2 = plt.subplots(1, 2, figsize=(18, 7))
    fig2.suptitle('A/D Period Sweep — Aggregate Across ' + str(len(eq_files)) + ' Universes | Winner: p={}'.format({ws}),
                   fontsize=14, fontweight='bold')

    ax_s = axes2[0]
    ax_s.plot(smry['period'], smry['avg_sharpe'], 'o-', color='#1f77b4', linewidth=2, markersize=6)
    ax_s.axvline(x={ws}, color='orange', linestyle='--', linewidth=2, label='Winner p={}'.format({ws}))
    ax_s.axvline(x=20, color='gray', linestyle=':', linewidth=2, label='Baseline p=20')
    ax_s.set_xlabel('A/D Period', fontsize=11)
    ax_s.set_ylabel('Avg Sharpe (higher is better)', fontsize=11)
    ax_s.set_title('Avg Sharpe vs Period', fontsize=12)
    ax_s.legend()
    ax_s.grid(True, alpha=0.3)

    ax_d = axes2[1]
    ax_d.plot(smry['period'], smry['avg_dd_pct'], 's-', color='#d62728', linewidth=2, markersize=6)
    ax_d.axvline(x={ws}, color='orange', linestyle='--', linewidth=2, label='Winner p={}'.format({ws}))
    ax_d.axvline(x=20, color='gray', linestyle=':', linewidth=2, label='Baseline p=20')
    ax_d.set_xlabel('A/D Period', fontsize=11)
    ax_d.set_ylabel('Avg Max Drawdown % (lower is better)', fontsize=11)
    ax_d.set_title('Avg MaxDD vs Period', fontsize=12)
    ax_d.legend()
    ax_d.grid(True, alpha=0.3)

    plt.tight_layout()
    agg_out = os.path.join(CHART_DIR, 'ad_period_sweep_aggregate.png')
    fig2.savefig(agg_out, dpi=150, bbox_inches='tight')
    plt.close(fig2)
    print('  Saved aggregate: ' + agg_out)
else:
    print('WARNING: snapshots/ad_period_9way_summary.csv not found')

print('Charts generated successfully.')
"##,
        ws = winner_s,
        r1s = runner1_s,
        r2s = runner2_s);

    let script_path = "scripts/plot_ad_period_sweep.py";
    std::fs::write(script_path, &script)?;
    println!("\n  Running chart script...");
    let status = std::process::Command::new("python3")
        .arg(script_path)
        .current_dir(".")
        .status();

    if !status.map(|s| s.success()).unwrap_or(false) {
        eprintln!("  WARNING: chart script had issues — check matplotlib installed");
    }

    Ok(())
}
"##,
    winner = winner,
    runner1 = runner1,
    runner2 = runner2);

    let script_path = "scripts/plot_ad_period_sweep.py";
    std::fs::write(script_path, &script)?;
    println!("\n  Running chart script...");
    let status = std::process::Command::new("python3")
        .arg(script_path)
        .current_dir(".")
        .status();

    if !status.map(|s| s.success()).unwrap_or(false) {
        eprintln!("  WARNING: chart script had issues — check matplotlib installed");
    }

    Ok(())
}
"##,
    winner = winner,
    runner1 = runner1,
    runner2 = runner2);

    let script_path = "scripts/plot_ad_period_sweep.py";
    std::fs::write(script_path, &script)?;
    println!("\n  Running chart script...");
    let _ = std::process::Command::new("python3")
        .arg(script_path)
        .current_dir(".")
        .status();
    Ok(())
}
"##,
    winner = winner, runner1 = runner1, runner2 = runner2);

    let script_path = "scripts/plot_ad_period_sweep.py";
    std::fs::write(script_path, &script)?;
    println!("\n  Running chart script...");
    let _ = std::process::Command::new("python3")
        .arg(script_path)
        .current_dir(".")
        .status();
    Ok(())
}

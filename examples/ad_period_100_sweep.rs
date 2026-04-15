//! =========================================================
//! HYPERPARAMETER OPTIMIZATION: A/D Momentum Period — FULL 100-VALUE SWEEP
//! =========================================================
//!
//! TARGET: A/D momentum lookback period — tested only at 5-10 values before
//! SWEEP:  1 to 100 bars in steps of 1 → 100 values (entire logical range)
//! UNIVERSES: All 9 harsh universes
//! METHOD:    Walk-forward 252/252 + 15 CPCV resamples + equity curve export
//! OPTIMIZED: Precomputes cumulative A/D lines to avoid O(N²) recomputation

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

const S_BASE5: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const S_NODOGE: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT",
];
const S_L4: [&str; 6] = ["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "_", "_"];
const S_L5BNB: [&str; 6] = ["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "_"];
const S_OGNM: [&str; 6] = ["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "_"];
const S_LCAPS: [&str; 6] = ["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT", "_"];
const S_L3: [&str; 6] = ["BTCUSDT", "ETHUSDT", "XRPUSDT", "_", "_", "_"];
const S_LVOL: [&str; 6] = ["BTCUSDT", "ETHUSDT", "XRPUSDT", "EOSUSDT", "_", "_"];
const S_OG4: [&str; 6] = ["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "_", "_"];

const UNIVERSES: &[(&str, &[&str; 6])] = &[
    ("Base5", &S_BASE5),
    ("NoDOGE", &S_NODOGE),
    ("Legacy4", &S_L4),
    ("Legacy5BNB", &S_L5BNB),
    ("OldGuardNoBNB", &S_OGNM),
    ("LargeCaps5", &S_LCAPS),
    ("Legacy3", &S_L3),
    ("LowVolume5", &S_LVOL),
    ("OldGuard4", &S_OG4),
];

#[derive(Clone, Debug)]
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
    period_results: Vec<PeriodResult>,
    best_period: usize,
}

#[derive(Clone)]
struct PeriodAgg {
    period: usize,
    total_qp: usize,
    total_cp: usize,
    total_sharpe: f64,
    total_dd: f64,
}

#[derive(Clone, Debug)]
struct SymbolData {
    opens: Vec<f64>,
    closes: Vec<f64>,
    ad_cum: Vec<f64>,
}

fn compute_ad_line(df: &DataFrame) -> Vec<f64> {
    let high = df.column("high").unwrap().f64().unwrap();
    let low = df.column("low").unwrap().f64().unwrap();
    let close = df.column("close").unwrap().f64().unwrap();
    let vol = df.column("volume").unwrap().f64().unwrap();
    let n = df.height();
    let mut ad = Vec::with_capacity(n);
    let mut cum: f64 = 0.0;
    for i in 0..n {
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let c = close.get(i).unwrap_or(0.0);
        let v = vol.get(i).unwrap_or(0.0);
        let r = h - l;
        let mf = if r > 1e-9 {
            ((c - l) - (h - c)) / r
        } else {
            0.0
        };
        cum += mf * v;
        ad.push(cum);
    }
    ad
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();
    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    println!("\n{}", "=".repeat(72));
    println!("  HYPEROPT: A/D Momentum Period — Full 100-value Sweep (1..100)");
    println!(
        "  Universes: {} | Hold: {} bars | Taker fee: {:.1}%",
        UNIVERSES.len(),
        HOLD_BARS,
        TAKER_FEE * 100.0
    );
    println!("{}", "=".repeat(72));

    // Load data
    let mut sym_cache: HashMap<String, SymbolData> = HashMap::new();
    let loader = DataLoader::new(None, None);

    let all_syms: Vec<&str> = UNIVERSES
        .iter()
        .flat_map(|(_, s)| s.iter().filter(|s| !s.is_empty()).copied())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for sym in all_syms.iter().filter(|s| **s != "_") {
        let raw = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        let n = df.height();
        let opens: Vec<f64> = df.column("open")?.f64()?.into_no_null_iter().collect();
        let closes: Vec<f64> = df.column("close")?.f64()?.into_no_null_iter().collect();
        let ad_cum = compute_ad_line(&df);
        assert_eq!(opens.len(), n);
        assert_eq!(closes.len(), n);
        assert_eq!(ad_cum.len(), n);
        sym_cache.insert(
            sym.to_string(),
            SymbolData {
                opens,
                closes,
                ad_cum,
            },
        );
    }
    println!("Loaded {} symbols.\n", sym_cache.len());

    // Build per-universe trimmed data
    let mut uni_data: Vec<(&str, Vec<(String, SymbolData)>)> = Vec::new();
    for &(uni_name, syms) in UNIVERSES {
        let mut items: Vec<(String, SymbolData)> = Vec::new();
        for &sym in syms.iter().filter(|s| !s.is_empty()) {
            if let Some(sd) = sym_cache.get(sym) {
                items.push((sym.to_string(), sd.clone()));
            }
        }
        if !items.is_empty() {
            uni_data.push((uni_name, items));
        }
    }

    let mut all_uni_results: Vec<UniverseResult> = Vec::new();
    let mut global_agg: HashMap<usize, PeriodAgg> = HashMap::new();

    for &(uni_name, ref items) in &uni_data {
        let n = items.iter().map(|(_, s)| s.opens.len()).min().unwrap_or(0);
        let n_win = n.saturating_sub(TRAIN_BARS) / TEST_BARS;
        if n_win < 1 {
            println!("  SKIP {} (n_win=0)", uni_name);
            continue;
        }

        println!(
            "[{}] {} syms, {} bars, {} windows",
            uni_name,
            items.len(),
            n,
            n_win
        );
        let t_uni = std::time::Instant::now();

        let mut p_results: Vec<PeriodResult> = Vec::with_capacity(100);

        for period in 1..=100 {
            let mut qp = 0;
            let mut t_ret: f64 = 0.0;
            let mut t_sharpe: f64 = 0.0;
            let mut t_dd: f64 = 0.0;
            let mut t_trades = 0;
            let mut wins = 0;
            let mut eq_bar = 1.0_f64;
            let mut eq_curve: Vec<f64> = Vec::new();
            let mut all_trades: Vec<f64> = Vec::new();

            for wi in 0..n_win {
                let ts = TRAIN_BARS + wi * TEST_BARS;
                let te = (ts + TEST_BARS).min(n);
                let (ret, sh, dd, trades, trets, eslice) = run_window(items, ts, te, period);
                let passed = trades >= MIN_TRADES && ret > 0.0;
                if passed {
                    qp += 1;
                }
                t_ret += ret;
                t_sharpe += sh;
                t_dd = t_dd.max(dd);
                t_trades += trades;
                if ret > 0.0 {
                    wins += 1;
                }
                all_trades.extend(trets);
                for &r in &eslice {
                    eq_bar *= 1.0 + r;
                    eq_curve.push(eq_bar);
                }
            }

            let n_w = n_win;
            let avg_ret = if n_w > 0 { t_ret / n_w as f64 } else { 0.0 };
            let avg_sh = if n_w > 0 { t_sharpe / n_w as f64 } else { 0.0 };
            let avg_dd = if n_w > 0 { t_dd / n_w as f64 } else { 0.0 };

            // CPCV
            let mut rs = 0;
            if !all_trades.is_empty() {
                for ri in 0..CPCV_RESAMPLES {
                    let mut eq = 1.0;
                    for j in 0..all_trades.len() {
                        let idx = (ri * 17 + j * 7) % all_trades.len();
                        eq *= 1.0 + all_trades[idx];
                    }
                    if eq > 1.0 {
                        rs += 1;
                    }
                }
            }

            p_results.push(PeriodResult {
                period,
                quarter_passes: qp,
                windows: n_w,
                resample_passes: rs,
                avg_ret,
                avg_sharpe: avg_sh,
                avg_dd,
                total_trades: t_trades,
                wins,
                equity_curve: eq_curve,
            });

            let agg = global_agg.entry(period).or_insert(PeriodAgg {
                period,
                total_qp: 0,
                total_cp: 0,
                total_sharpe: 0.0,
                total_dd: 0.0,
            });
            agg.total_qp += qp;
            agg.total_cp += rs;
            agg.total_sharpe += avg_sh;
            agg.total_dd += avg_dd;

            if period % 20 == 0 || period == 100 {
                println!(
                    "    p={:3}: QP={}/{} RS={} Sharpe={:+.2} DD={:+.2}% t={}",
                    period, qp, n_w, rs, avg_sh, avg_dd, t_trades
                );
            }
        }

        let best = p_results
            .iter()
            .max_by(|a, b| {
                a.quarter_passes
                    .cmp(&b.quarter_passes)
                    .then_with(|| a.avg_sharpe.partial_cmp(&b.avg_sharpe).unwrap())
            })
            .map(|r| r.period)
            .unwrap_or(30);

        println!(
            "  {} done in {:.1}s — best = {}\n",
            uni_name,
            t_uni.elapsed().as_secs_f64(),
            best
        );
        all_uni_results.push(UniverseResult {
            name: uni_name.to_string(),
            period_results: p_results,
            best_period: best,
        });
    }

    // Global table
    let n_uni = all_uni_results.len();
    let max_qp = n_uni * 4;
    println!("\n{}", "=".repeat(72));
    println!("  GLOBAL: Period vs {} Universes", n_uni);
    println!(
        "  {:>6} | {:>4}/{:>4} | {:>5} | {:>7} | {:>7}",
        "Period", "QP", "max", "CPCV%", "Sharpe", "DD%"
    );
    println!("  {}", "-".repeat(52));
    let mut sorted: Vec<_> = global_agg.values().collect();
    sorted.sort_by(|a, b| {
        b.total_qp
            .cmp(&a.total_qp)
            .then_with(|| b.total_sharpe.partial_cmp(&a.total_sharpe).unwrap())
    });
    for agg in &sorted {
        let sh = agg.total_sharpe / n_uni as f64;
        let dd = agg.total_dd / n_uni as f64;
        let cp = agg.total_cp as f64 / (n_uni as f64 * 4.0 * CPCV_RESAMPLES as f64) * 100.0;
        let m = if agg.period == 20 { " *" } else { "" };
        println!(
            "  {:>6} | {:>4}/{:>4} | {:>5.1}% | {:>+7.2} | {:>+7.2}{}",
            agg.period, agg.total_qp, max_qp, cp, sh, dd, m
        );
    }

    // Pick winners
    sorted.sort_by(|a, b| {
        b.total_qp
            .cmp(&a.total_qp)
            .then_with(|| {
                (b.total_cp as f64 / (n_uni as f64 * 4.0 * CPCV_RESAMPLES as f64))
                    .partial_cmp(
                        &(a.total_cp as f64 / (n_uni as f64 * 4.0 * CPCV_RESAMPLES as f64)),
                    )
                    .unwrap()
            })
            .then_with(|| b.total_sharpe.partial_cmp(&a.total_sharpe).unwrap())
    });
    let winner = sorted.first().map(|a| a.period).unwrap_or(30);
    let cand: Vec<_> = sorted
        .iter()
        .filter(|a| a.period != winner && a.period != 20)
        .take(2)
        .map(|a| a.period)
        .collect();
    let r1 = cand
        .get(0)
        .copied()
        .unwrap_or(if winner < 50 { winner + 7 } else { winner - 7 });
    let r2 = cand.get(1).copied().unwrap_or(if winner < 30 {
        winner + 13
    } else {
        winner - 13
    });

    println!("\n{}", "=".repeat(72));
    println!(
        "  GLOBAL WINNER: p={}  |  Runners: p={}, p={}",
        winner, r1, r2
    );
    println!("  Baseline: p=20");
    println!("{}", "=".repeat(72));

    // Export equity curves
    export_equity_curves(&uni_data, &[20, winner, r1, r2])?;
    write_detail_csv(&all_uni_results)?;
    write_global_csv(&global_agg, &all_uni_results)?;
    let elapsed = t0.elapsed();
    println!(
        "\nSweep done in {:.1}s. Artifacts in snapshots/.",
        elapsed.as_secs_f64()
    );
    generate_chart(winner, r1, r2)?;
    println!("Done.");
    Ok(())
}

fn run_window(
    items: &[(String, SymbolData)],
    start: usize,
    end: usize,
    period: usize,
) -> (f64, f64, f64, usize, Vec<f64>, Vec<f64>) {
    let mut equity: f64 = 1.0;
    let mut peak: f64 = 1.0;
    let mut max_dd: f64 = 0.0;
    let mut trades = 0usize;
    let mut trade_rets: Vec<f64> = Vec::new();
    let mut equity_slice: Vec<f64> = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;
    let mut bar = start;

    while bar + 1 < end {
        if pos.is_none() {
            let mut cands: Vec<(String, f64)> = Vec::new();
            for (sym, sd) in items {
                if bar.saturating_sub(1) < period || bar.saturating_sub(1) >= sd.ad_cum.len() {
                    continue;
                }
                let idx = bar.saturating_sub(1);
                let ad_now = sd.ad_cum[idx];
                let ad_past = if period <= idx {
                    sd.ad_cum[idx - period]
                } else {
                    0.0
                };
                let mom = ad_now - ad_past;
                if mom > 0.0 {
                    cands.push((sym.clone(), mom));
                }
            }
            cands.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let longs: Vec<_> = cands.iter().take(TOP_K).collect();
            if !longs.is_empty() {
                let sym = &longs[0].0;
                if let Some((_, sd)) = items.iter().find(|(s, _)| s == sym) {
                    if bar < sd.opens.len() && sd.opens[bar] > 0.0 {
                        pos = Some((sym.clone(), bar, sd.opens[bar]));
                    }
                }
            }
            equity_slice.push(equity - 1.0);
            bar += 1;
            continue;
        }

        let (sym, entry_bar, entry_px) = pos.as_ref().unwrap();
        let cur_bar = bar;
        if cur_bar >= *entry_bar + HOLD_BARS || cur_bar >= end.saturating_sub(1) {
            if let Some((_, sd)) = items.iter().find(|(s, _)| s == sym) {
                let exit_idx = cur_bar.min(sd.closes.len().saturating_sub(1));
                let exit_px = sd.closes.get(exit_idx).copied().unwrap_or(*entry_px);
                if *entry_px > 0.0 && exit_px > 0.0 {
                    let gross = (exit_px / *entry_px - 1.0) - TAKER_FEE;
                    equity *= 1.0 + gross;
                    trade_rets.push(gross);
                    trades += 1;
                }
            }
            pos = None;
        }
        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);
        equity_slice.push(equity - 1.0);
        bar += 1;
    }

    if let Some((sym, _, entry_px)) = pos {
        if let Some((_, sd)) = items.iter().find(|(s, _)| s == &sym) {
            let ei = (end.saturating_sub(1)).min(sd.closes.len().saturating_sub(1));
            let exit_px = sd.closes.get(ei).copied().unwrap_or(entry_px);
            if entry_px > 0.0 && exit_px > 0.0 {
                let gross = (exit_px / entry_px - 1.0) - TAKER_FEE;
                equity *= 1.0 + gross;
                trade_rets.push(gross);
                trades += 1;
            }
        }
    }
    peak = peak.max(equity);
    max_dd = max_dd.min(equity / peak - 1.0);
    let ret = (equity - 1.0) * 100.0;
    let sh = if trade_rets.len() < 2 {
        0.0
    } else {
        let n = trade_rets.len() as f64;
        let mean = trade_rets.iter().sum::<f64>() / n;
        let std = (trade_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n).sqrt();
        if std < 1e-9 {
            0.0
        } else {
            mean / std * (252.0_f64.sqrt())
        }
    };
    (ret, sh, max_dd * 100.0, trades, trade_rets, equity_slice)
}

fn export_equity_curves(
    uni_data: &[(&str, Vec<(String, SymbolData)>)],
    periods: &[usize],
) -> Result<()> {
    for &(uni_name, ref items) in uni_data {
        let n = items.iter().map(|(_, s)| s.opens.len()).min().unwrap_or(0);
        let csv_path = format!("snapshots/eqcurve_{}_adperiod.csv", uni_name);
        let mut f = File::create(&csv_path)?;
        writeln!(
            f,
            "bar,{}",
            periods
                .iter()
                .map(|p| format!("p{}", p))
                .collect::<Vec<_>>()
                .join(",")
        )?;

        let mut period_eqs: Vec<Vec<f64>> = Vec::new();
        for &period in periods {
            let mut eq_bar = 1.0_f64;
            let mut eq_curve: Vec<f64> = Vec::new();
            let n_win = n.saturating_sub(TRAIN_BARS) / TEST_BARS;
            for wi in 0..n_win {
                let ts = TRAIN_BARS + wi * TEST_BARS;
                let te = (ts + TEST_BARS).min(n);
                let (_, _, _, _, _, eslice) = run_window(items, ts, te, period);
                for &r in &eslice {
                    eq_bar *= 1.0 + r;
                    eq_curve.push(eq_bar);
                }
            }
            period_eqs.push(eq_curve);
        }

        let max_len = period_eqs.iter().map(|v| v.len()).max().unwrap_or(0);
        for i in 0..max_len {
            let mut row = format!("{}", i);
            for peq in &period_eqs {
                let val = peq.get(i).copied().unwrap_or_else(|| {
                    if i == 0 {
                        1.0
                    } else {
                        peq.last().copied().unwrap_or(1.0)
                    }
                });
                row.push_str(&format!(",{:.6}", val));
            }
            writeln!(f, "{}", row)?;
        }
        println!("  Exported: {}", csv_path);
    }
    Ok(())
}

fn write_detail_csv(all_uni_results: &[UniverseResult]) -> Result<()> {
    let path = "snapshots/ad_period_fullsweep_detail.csv";
    let mut f = File::create(path)?;
    writeln!(f, "universe,period,quarter_passes,windows,resample_passes,max_resamples,avg_sharpe,avg_dd_pct,total_trades,wins")?;
    for uni in all_uni_results {
        for r in &uni.period_results {
            let max_cp = 4 * CPCV_RESAMPLES;
            writeln!(
                f,
                "{},{},{},{},{},{},{:.3},{:.2},{},{}",
                uni.name,
                r.period,
                r.quarter_passes,
                r.windows,
                r.resample_passes,
                max_cp,
                r.avg_sharpe,
                r.avg_dd,
                r.total_trades,
                r.wins
            )?;
        }
    }
    println!("  Written: {}", path);
    Ok(())
}

fn write_global_csv(
    global_agg: &HashMap<usize, PeriodAgg>,
    all_uni_results: &[UniverseResult],
) -> Result<()> {
    let path = "snapshots/ad_period_fullsweep_global.csv";
    let mut f = File::create(path)?;
    let n_uni = all_uni_results.len();
    let max_qp = n_uni * 4;
    let max_cp = n_uni * 4 * CPCV_RESAMPLES;
    writeln!(
        f,
        "period,total_qp,max_qp,cpcv_total,max_cpcv,cpcv_pct,avg_sharpe,avg_dd_pct"
    )?;
    let mut sorted: Vec<_> = global_agg.values().collect();
    sorted.sort_by_key(|a| a.period);
    for agg in sorted.iter() {
        let cp_pct = if max_cp > 0 {
            agg.total_cp as f64 / max_cp as f64 * 100.0
        } else {
            0.0
        };
        let avg_sh = agg.total_sharpe / n_uni as f64;
        let avg_dd = agg.total_dd / n_uni as f64;
        writeln!(
            f,
            "{},{},{},{},{},{:.1},{:.3},{:.2}",
            agg.period, agg.total_qp, max_qp, agg.total_cp, max_cp, cp_pct, avg_sh, avg_dd
        )?;
    }
    println!("  Written: {}", path);
    Ok(())
}

fn generate_chart(winner: usize, runner1: usize, runner2: usize) -> Result<()> {
    let script = format!(
        r#"
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd
import numpy as np
import os, glob, sys

CHART_DIR = 'charts'
os.makedirs(CHART_DIR, exist_ok=True)

PERIODS = [20, {w}, {r1}, {r2}]
COLORS  = ['#888888', '#1f77b4', '#ff7f0e', '#2ca02c']
LABELS  = ['Baseline (p=20)', 'Winner (p={w})', 'Runner-up (p={r1})', 'Runner-up (p={r2})']
LS = ['-', '-', '--', '--']
LW = [1.5, 3.0, 1.5, 1.5]

eq_files = sorted(glob.glob('snapshots/eqcurve_*_adperiod.csv'))
if not eq_files:
    print("ERROR: No equity curve CSVs found.", file=sys.stderr)
    sys.exit(1)

for fpath in eq_files:
    uni = os.path.basename(fpath).replace('eqcurve_', '').replace('_adperiod.csv', '')
    df = pd.read_csv(fpath, index_col=0)
    for col in df.columns:
        vals = df[col].dropna().values.astype(float)
        if len(vals) == 0: continue
        first_val = vals[0]
        if abs(first_val) < 1e-9:
            first_val = 1.0
    
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(16, 10), sharex=True,
                                    gridspec_kw={{'height_ratios': [3, 1]}})
    fig.suptitle(
        f'A/D Period Sweep — Equity Curve vs Drawdown ({{uni}})\n'
        f'Winner: p={w}  |  Runners: p={r1}, p={r2}  |  Baseline: p=20',
        fontsize=14, fontweight='bold')

    for col, color, label, ls, lw in zip(df.columns, COLORS, LABELS, LS, LW):
        vals = df[col].dropna().replace([np.inf, -np.inf], np.nan).dropna().values.astype(float)
        if len(vals) == 0: continue
        x = np.arange(len(vals))
        ax1.plot(x, vals, label=label, color=color, linestyle=ls, linewidth=lw, alpha=0.9)

    # Fix infinity before setting Y limits
    all_vals = pd.concat([df[c].dropna() for c in df.columns])
    if len(all_vals) > 0 and not all_vals.isna().all():
        all_vals = all_vals.replace([np.inf, -np.inf], np.nan).dropna()
        if len(all_vals) > 0:
            y_min, y_max = all_vals.min(), all_vals.max()
            if not np.isnan(y_min) and not np.isnan(y_max) and np.isfinite(y_min) and np.isfinite(y_max):
                rng = y_max - y_min
                if rng > 0 and rng < 1e10:
                    try:
                        ax1.set_ylim(max(y_min - rng * 0.02, 0.01), y_max + rng * 0.05)
                    except:
                        pass
                elif rng == 0:
                    try:
                        ax1.set_ylim(max(y_min * 0.9, 0.01), y_max * 1.1)
                    except:
                        pass
    
    try:
        ax1.set_yscale('log')
    except Exception as e:
        print("Log scale err:", e)
    ax1.set_ylabel('Equity (log scale)')
    ax1.legend(loc='upper left', fontsize=9)
    ax1.grid(True, alpha=0.3, which='both')

    for col, color, label, ls, lw in zip(df.columns, COLORS, LABELS, LS, LW):
        vals = df[col].dropna().replace([np.inf, -np.inf], np.nan).dropna().values.astype(float)
        if len(vals) == 0: continue
        pk = np.maximum.accumulate(vals)
        pk[pk == 0] = 1.0 # avoid div by zero
        dd = (vals - pk) / pk * 100.0
        
        # fix infs in dd
        dd[np.isinf(dd)] = 0.0
        dd[np.isnan(dd)] = 0.0
        
        x = np.arange(len(vals))
        ax2.fill_between(x, dd, 0, alpha=0.3, color=color)
        ax2.plot(x, dd, label=label, color=color, linestyle=ls, linewidth=lw)
        ax2.plot(x, dd, color=color, linestyle=ls, linewidth=lw)

    ax2.set_ylabel('Drawdown %')
    ax2.set_xlabel('Trading Day (walk-forward)')
    ax2.legend(loc='lower left', fontsize=8)
    ax2.grid(True, alpha=0.3)
    ax2.set_ylim(bottom=max(-100, dd.min() * 1.1))

    plt.tight_layout()
    out = os.path.join(CHART_DIR, f'ad_period_sweep_{{uni}}.png')
    fig.savefig(out, dpi=150, bbox_inches='tight')
    plt.close(fig)
    print(f'  Saved: {{out}}')

# Aggregate
if os.path.exists('snapshots/ad_period_fullsweep_global.csv'):
    smry = pd.read_csv('snapshots/ad_period_fullsweep_global.csv')
    fig, axes = plt.subplots(1, 3, figsize=(22, 7))
    fig.suptitle(f'A/D Period Sweep — Aggregate Across {{len(eq_files)}} Universes | Winner: p={w}',
                 fontsize=14, fontweight='bold')
    
    for ax, ycol, title, ylabel in zip(axes,
        ['avg_sharpe', 'total_qp', 'avg_dd_pct'],
        ['Avg Sharpe vs Period', 'Total Quarter Passes vs Period', 'Avg Max DD vs Period'],
        ['Avg Sharpe', 'Total QP', 'Avg Max DD %']):
        ax.plot(smry['period'], smry[ycol], 'o-', color='#1f77b4', linewidth=2, markersize=4)
        ax.axvline(x={w}, color='#ff7f0e', linestyle='--', linewidth=2, label=f'Winner p={w}')
        ax.axvline(x=20, color='gray', linestyle=':', linewidth=2, label='Baseline p=20')
        ax.set_xlabel('A/D Period')
        ax.set_ylabel(ylabel)
        ax.set_title(title)
        ax.legend()
        ax.grid(True, alpha=0.3)
    plt.tight_layout()
    out = os.path.join(CHART_DIR, 'ad_period_fullsweep_aggregate.png')
    fig.savefig(out, dpi=150, bbox_inches='tight')
    plt.close(fig)
    print(f'  Saved: {{out}}')

print('Charts generated.')
"#,
        w = winner,
        r1 = runner1,
        r2 = runner2
    );

    std::fs::create_dir_all("scripts")?;
    let script_path = "scripts/plot_ad_period_100_sweep.py";
    std::fs::write(script_path, &script)?;
    println!("\n  Running chart script...");
    let status = std::process::Command::new("python3")
        .arg(script_path)
        .status();
    if !status.map(|s| s.success()).unwrap_or(false) {
        eprintln!("  WARNING: chart script had issues");
    }
    Ok(())
}

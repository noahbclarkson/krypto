//! Hyperparameter Optimization: A/D Momentum Period
//!
//! Tests: 10, 15, 20, 30, 42 bars
//! Baseline (20 bars) vs candidates
//!
//! Universe: Base5 (BTC, ETH, SOL, XRP, DOGE, ADA)
//! Method: Walk-forward (4 windows, 252 train / 252 test)
//! Metrics: Sharpe, MaxDD, WinRate, PassCount
//!
//! Rule: pick the most ROBUST across windows, not the most flattering single window.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;
const TOP_K: usize = 2;

const SYMS: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];

const PERIODS: [usize; 5] = [10, 15, 20, 30, 42];

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n========================================================");
    println!("  HYPEROPT: A/D Momentum Period Sweep (Base5)");
    println!("  Periods: {:?}", PERIODS);
    println!("========================================================\n");

    // Load data
    let loader = DataLoader::new(None, None);
    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in SYMS {
        let raw = loader.fetch_with_cache(s, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        let n = df.height();
        min_len = min_len.min(n);
        cache.insert(s.to_string(), df);
    }
    let n = min_len.min(2800);
    for (_, df) in &mut cache {
        if df.height() > n {
            *df = df.slice(0, n);
        }
    }
    let windows = n.saturating_sub(TRAIN_BARS + 42) / TEST_BARS;
    println!(
        "Loaded {} syms, {} bars, {} test windows\n",
        SYMS.len(),
        n,
        windows
    );

    let mut all_results: Vec<PeriodResult> = Vec::new();

    for &period in &PERIODS {
        let mut recs: Vec<WindowRec> = Vec::new();
        let mut total_ret = 0.0_f64;
        let mut total_sharpe = 0.0_f64;
        let mut worst_dd = 0.0_f64;
        let mut total_trades = 0usize;
        let mut passes = 0usize;

        for wi in 0..windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let tstart = train_end;
            let tend = (tstart + TEST_BARS).min(n);
            if tend.saturating_sub(tstart) < HOLD_BARS + period + 2 {
                continue;
            }

            let (ret, sh, dd, trades) = run_ad_backtest(&cache, tstart, tend, period);
            let passed = trades >= MIN_TRADES && ret > 0.0;
            if passed {
                passes += 1;
            }
            total_ret += ret;
            total_sharpe += sh;
            worst_dd = worst_dd.min(dd);
            total_trades += trades;

            recs.push(WindowRec {
                wi,
                ret,
                sh,
                dd,
                trades,
                passed,
            });
            let flag = if passed { "PASS" } else { "FAIL" };
            println!(
                "  period={:2} W{:02}: {:4}t {:+8.1}% sh={:+.2} DD={:+6.1}%  {}",
                period, wi, trades, ret, sh, dd, flag
            );
        }

        let n_recs = recs.len();
        let avg_ret = if n_recs > 0 {
            total_ret / n_recs as f64
        } else {
            0.0
        };
        let avg_sh = if n_recs > 0 {
            total_sharpe / n_recs as f64
        } else {
            0.0
        };
        let avg_tr = if n_recs > 0 {
            total_trades as f64 / n_recs as f64
        } else {
            0.0
        };

        all_results.push(PeriodResult {
            period,
            windows: n_recs,
            passes,
            avg_ret,
            avg_sharpe: avg_sh,
            worst_dd,
            avg_trades: avg_tr,
            recs,
        });

        println!("  → period={:2}: {}/{} passes | avg OOS {:+7.1}% | Sharpe {:.2} | DD {:+.1}% | avg {}t\n",
            period, passes, n_recs, avg_ret, avg_sh, worst_dd, avg_tr as usize);
    }

    // Summary table
    println!("\n========================================================");
    println!("  SWEEP RESULTS");
    println!("========================================================");
    println!(
        "{:>6} | {:>4} | {:>5}% | {:>10} | {:>7} | {:>9}",
        "Period", "Pass", "Win%", "Avg OOS%", "Sharpe", "Worst DD%"
    );
    println!("------------------------------------------------");
    for r in &all_results {
        let win_pct = if r.windows > 0 {
            r.passes as f64 / r.windows as f64 * 100.0
        } else {
            0.0
        };
        println!(
            "{:>6} | {}/{} | {:>5.0}% | {:>+10.1}% | {:>+7.2} | {:>+9.1}",
            r.period, r.passes, r.windows, win_pct, r.avg_ret, r.avg_sharpe, r.worst_dd
        );
    }

    // Best by Sharpe
    let best = all_results
        .iter()
        .max_by(|a, b| a.avg_sharpe.partial_cmp(&b.avg_sharpe).unwrap())
        .unwrap();
    println!(
        "\nBest by avg Sharpe: period={} (Sharpe {:.2})",
        best.period, best.avg_sharpe
    );

    // Sensitivity
    println!("\nSensitivity (ΔSharpe vs winner):");
    for r in &all_results {
        let delta = best.avg_sharpe - r.avg_sharpe;
        println!("  period={:2}: Δ={:+.3}", r.period, delta);
    }

    write_csv(&all_results)?;
    println!("\nDone. CSV written to snapshots/ad_period_sweep.csv");

    Ok(())
}

// ─── Inline A/D momentum backtest (period is a param) ───────────────────────

fn run_ad_backtest(
    cache: &HashMap<String, DataFrame>,
    start: usize,
    end: usize,
    period: usize,
) -> (f64, f64, f64, usize) {
    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets: Vec<f64> = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;

    let sym_list: Vec<String> = SYMS.iter().map(|s| s.to_string()).collect();
    let mut bar = start;

    while bar + 1 < end {
        if pos.is_none() {
            // ── A/D momentum ranking (inline, period is a param) ──
            let mut longs: Vec<(String, f64)> = Vec::new();
            let mut shorts: Vec<(String, f64)> = Vec::new();

            for sym in &sym_list {
                let df = cache.get(sym).unwrap();
                let n = df.height();
                let idx = bar.saturating_sub(1);
                if idx < period {
                    bar += 1;
                    continue;
                }

                let close_ch = df.column("close").unwrap().f64().unwrap();
                let high_ch = df.column("high").unwrap().f64().unwrap();
                let low_ch = df.column("low").unwrap().f64().unwrap();
                let vol_ch = df.column("volume").unwrap().f64().unwrap();

                // Cumulative A/D line up to idx
                let mut ad_now: f64 = 0.0;
                for i in 0..=idx {
                    let h = high_ch.get(i).unwrap_or(0.0);
                    let l = low_ch.get(i).unwrap_or(0.0);
                    let c = close_ch.get(i).unwrap_or(0.0);
                    let v = vol_ch.get(i).unwrap_or(0.0);
                    let range = h - l;
                    let mf = if range > 1e-9 {
                        ((c - l) - (h - c)) / range
                    } else {
                        0.0
                    };
                    ad_now += mf * v;
                }

                // A/D line period bars ago
                let mut ad_past: f64 = 0.0;
                let pstart = idx.saturating_sub(period);
                for i in pstart..=idx.saturating_sub(period) {
                    if i >= idx.saturating_sub(period) {
                        break;
                    }
                }
                let pstart2 = idx.saturating_sub(period);
                for i in pstart2..idx {
                    if i >= n {
                        break;
                    }
                    let h = high_ch.get(i).unwrap_or(0.0);
                    let l = low_ch.get(i).unwrap_or(0.0);
                    let c = close_ch.get(i).unwrap_or(0.0);
                    let v = vol_ch.get(i).unwrap_or(0.0);
                    let range = h - l;
                    let mf = if range > 1e-9 {
                        ((c - l) - (h - c)) / range
                    } else {
                        0.0
                    };
                    ad_past += mf * v;
                }

                let mom = ad_now - ad_past;
                if mom > 0.0 {
                    longs.push((sym.clone(), mom));
                } else {
                    shorts.push((sym.clone(), mom));
                }
            }

            longs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            shorts.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            if !longs.is_empty() {
                let sym = &longs[0].0;
                let df = cache.get(sym).unwrap();
                let open_ch = df.column("open").unwrap().f64().unwrap();
                let entry_price = open_ch.get(bar).unwrap_or(0.0);
                if entry_price > 0.0 {
                    pos = Some((sym.clone(), bar, entry_price));
                }
            }

            bar += 1;
            continue;
        }

        // Position held: check exit
        let (sym, entry_bar, entry_px) = pos.as_ref().unwrap();
        let df = cache.get(sym).unwrap();
        let n = df.height();
        let cur_bar = bar;

        if cur_bar >= *entry_bar + HOLD_BARS || cur_bar >= end - 1 {
            let close_ch = df.column("close").unwrap().f64().unwrap();
            let exit_price = close_ch.get(cur_bar.min(n - 1)).unwrap_or(0.0);
            if *entry_px > 0.0 && exit_price > 0.0 {
                let gross = (exit_price / *entry_px - 1.0) - TAKER_FEE;
                equity *= 1.0 + gross;
                trades += 1;
                rets.push(gross);
            }
            pos = None;
        }

        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);
        bar += 1;
    }

    // Close any open position at end
    if let Some((sym, entry_bar, entry_px)) = pos {
        let df = cache.get(&sym).unwrap();
        let close_ch = df.column("close").unwrap().f64().unwrap();
        let exit_price = close_ch.get((end - 1).min(df.height() - 1)).unwrap_or(0.0);
        if entry_px > 0.0 && exit_price > 0.0 {
            let gross = (exit_price / entry_px - 1.0) - TAKER_FEE;
            equity *= 1.0 + gross;
            trades += 1;
            rets.push(gross);
        }
    }

    peak = peak.max(equity);
    max_dd = max_dd.min(equity / peak - 1.0);

    let ret = (equity - 1.0) * 100.0;
    let sh = if rets.is_empty() || rets.iter().map(|r| r.powi(2)).sum::<f64>() == 0.0 {
        0.0
    } else {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let std = (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64).sqrt();
        if std == 0.0 {
            0.0
        } else {
            mean / std * (252.0_f64.sqrt())
        }
    };

    (ret, sh, max_dd * 100.0, trades)
}

// ─── Data structures ──────────────────────────────────────────────────────────

struct PeriodResult {
    period: usize,
    windows: usize,
    passes: usize,
    avg_ret: f64,
    avg_sharpe: f64,
    worst_dd: f64,
    avg_trades: f64,
    recs: Vec<WindowRec>,
}

struct WindowRec {
    wi: usize,
    ret: f64,
    sh: f64,
    dd: f64,
    trades: usize,
    passed: bool,
}

// ─── CSV ─────────────────────────────────────────────────────────────────────

fn write_csv(results: &[PeriodResult]) -> Result<()> {
    std::fs::create_dir_all("snapshots")?;
    let mut lines = vec![
        "period,windows,passes,pass_rate_pct,avg_ret_pct,avg_sharpe,worst_dd_pct,avg_trades"
            .to_string(),
    ];
    for r in results {
        let win_pct = if r.windows > 0 {
            r.passes as f64 / r.windows as f64 * 100.0
        } else {
            0.0
        };
        lines.push(format!(
            "{},{},{},{:.1},{:.2},{:.2},{:.2},{:.1}",
            r.period,
            r.windows,
            r.passes,
            win_pct,
            r.avg_ret,
            r.avg_sharpe,
            r.worst_dd,
            r.avg_trades
        ));
    }
    std::fs::write("snapshots/ad_period_sweep.csv", lines.join("\n"))?;
    Ok(())
}

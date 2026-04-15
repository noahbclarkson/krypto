//! Hyperparameter Optimization: HOLD_BARS Full Integer Sweep
//!
//! Target: HOLD_BARS (how many bars to hold a position)
//! The prior sweep only tested 12 coarse values (3,5,7,10,14,21,28,35,42,49,56,63).
//! THIS sweep tests EVERY integer from 3 to 63 (61 values) for robustness.
//!
//! Strategy: A/D momentum (AD_PERIOD=47, already optimized)
//! Universe: Base5 (BTC, ETH, SOL, XRP, DOGE, ADA) — long-only top A/D momentum
//! Method: Walk-forward (4 windows, 252 train / 252 test) + 9-universe full-sample
//! Metrics: Sharpe, MaxDD, WinRate, PassCount, Equity Curves
//!
//! Rule: pick the most ROBUST hold period, not the most flattering single window.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::io::Write;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47) // Already optimized
const TOP_K: usize = 2;

const SYMS: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];

// FULL INTEGER SWEEP: every value from 3 to 63 (61 values)
const HOLDS: &[usize] = &[
    3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51,
    52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63,
];

// 9-universe stress test
const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
    "EOSUSDT", "BCHUSDT",
];
const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
    ),
    ("NoDOGE", &["ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("Legacy4", &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "Legacy5BNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"],
    ),
    (
        "OldGuardNoBNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
    (
        "LargeCaps5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"],
    ),
    ("Legacy3", &["XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    ("OldGuard4", &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
];

#[derive(Clone, Debug)]
struct WindowRec {
    wi: usize,
    ret: f64,
    sh: f64,
    dd: f64,
    trades: usize,
    passed: bool,
}

#[derive(Clone, Debug)]
struct ResultRow {
    hold: usize,
    windows: usize,
    passes: usize,
    avg_ret: f64,
    avg_sharpe: f64,
    worst_dd: f64,
    avg_trades: f64,
    recs: Vec<WindowRec>,
}

#[derive(Clone, Debug)]
struct UniverseResult {
    universe: String,
    hold: usize,
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
}

// Equity curve record for a single walk-forward run
#[derive(Clone)]
struct EquityPoint {
    bar: usize,
    equity: f64,
}

struct EquityResult {
    hold: usize,
    cumulative: Vec<f64>, // per-bar equity across all windows
}

#[tokio::main]
async fn main() -> Result<()> {
    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    // ── Load data ──────────────────────────────────────────────────────────
    println!("\n{}", "=".repeat(72));
    println!(
        "  HYPEROPT: HOLD_BARS Full Integer Sweep (3–63, {} values)",
        HOLDS.len()
    );
    println!("  Strategy: A/D momentum (AD_PERIOD={})", AD_PERIOD);
    println!("  Method: Walk-forward 4 windows + 9-universe full-sample");
    println!("{}\n", "=".repeat(72));

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
    let windows = n.saturating_sub(TRAIN_BARS + 63) / TEST_BARS;
    println!(
        "Loaded {} syms, {} bars, {} test windows\n",
        SYMS.len(),
        n,
        windows
    );

    // ── Walk-forward sweep (all hold values) ──────────────────────────────
    println!("{}", "=".repeat(72));
    println!("  WALK-FORWARD SWEEP (Base5)");
    println!("{}\n", "=".repeat(72));

    let mut wf_results: Vec<ResultRow> = Vec::new();
    let mut equity_curves: HashMap<usize, Vec<f64>> = HashMap::new(); // hold -> merged equity vec

    for &hold_bars in HOLDS {
        let mut recs: Vec<WindowRec> = Vec::new();
        let mut total_ret = 0.0_f64;
        let mut total_sharpe = 0.0_f64;
        let mut worst_dd = 0.0_f64;
        let mut total_trades = 0usize;
        let mut passes = 0usize;
        let mut merged_equity: Vec<f64> = vec![1.0];

        for wi in 0..windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let tstart = train_end;
            let tend = (tstart + TEST_BARS).min(n);
            if tend.saturating_sub(tstart) < hold_bars + AD_PERIOD + 2 {
                continue;
            }

            let (ret, sh, dd, trades, eq_vec) =
                run_ad_backtest_with_equity(&cache, tstart, tend, AD_PERIOD, hold_bars, &SYMS);

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

            // Extend merged equity across windows
            if !merged_equity.is_empty() && !eq_vec.is_empty() {
                let base = *merged_equity.last().unwrap();
                merged_equity.extend(eq_vec.iter().map(|&v| base * v));
            } else {
                merged_equity.extend(eq_vec);
            }

            let flag = if passed { "PASS" } else { "FAIL" };
            println!(
                "  hold={:3} W{:02}: {:4}t {:+8.1}% sh={:+.2} DD={:+6.1}%  {}",
                hold_bars, wi, trades, ret, sh, dd, flag
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

        wf_results.push(ResultRow {
            hold: hold_bars,
            windows: n_recs,
            passes,
            avg_ret,
            avg_sharpe: avg_sh,
            worst_dd,
            avg_trades: avg_tr,
            recs,
        });

        // Store equity curve for baseline (21) and winner
        equity_curves.insert(hold_bars, merged_equity);

        println!(
            "  → hold={:3}: {}/{} passes | avg OOS {:+7.1}% | Sharpe {:.2} | DD {:+.1}%\n",
            hold_bars, passes, n_recs, avg_ret, avg_sh, worst_dd
        );
    }

    // ── Rank walk-forward results ──────────────────────────────────────────
    let mut wf_sorted = wf_results.clone();
    wf_sorted.sort_by(|a, b| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap());

    println!("\n{}", "=".repeat(72));
    println!("  WALK-FORWARD RANKING (by avg Sharpe)");
    println!(
        "  {:>6} | {:>4} | {:>5}% | {:>10} | {:>7} | {:>9}",
        "Hold", "Pass", "Win%", "Avg OOS%", "Sharpe", "Worst DD%"
    );
    println!("{}", "─".repeat(62));
    for (i, r) in wf_sorted.iter().take(20).enumerate() {
        let win_pct = if r.windows > 0 {
            r.passes as f64 / r.windows as f64 * 100.0
        } else {
            0.0
        };
        let marker = if r.hold == 21 {
            " ←BASELINE"
        } else if i == 0 {
            " ★WINNER"
        } else {
            ""
        };
        println!(
            "  {:>6} | {}/{} | {:>5.0}% | {:>+10.1}% | {:>+7.2} | {:>+9.1}{}",
            r.hold, r.passes, r.windows, win_pct, r.avg_ret, r.avg_sharpe, r.worst_dd, marker
        );
    }

    let baseline_sharpe = wf_results
        .iter()
        .find(|r| r.hold == 21)
        .map(|r| r.avg_sharpe)
        .unwrap_or(0.0);
    let winner = &wf_sorted[0];
    println!("\n  Baseline(21): Sharpe {:.2}", baseline_sharpe);
    println!(
        "  Winner({:3}):  Sharpe {:.2}  (Δ = {:+.2})",
        winner.hold,
        winner.avg_sharpe,
        winner.avg_sharpe - baseline_sharpe
    );

    // ── 9-Universe Full-Sample ────────────────────────────────────────────
    println!("\n{}", "=".repeat(72));
    println!("  9-UNIVERSE FULL-SAMPLE SWEEP");
    println!("{}\n", "=".repeat(72));

    let mut uni_data = HashMap::<String, DataFrame>::new();
    for &sym in LOAD_SYMBOLS {
        if !uni_data.contains_key(sym) {
            let raw = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
            let df = FeatureEngine::add_technicals(&raw, None)?;
            uni_data.insert(sym.to_string(), df);
        }
    }
    let uni_min = LOAD_SYMBOLS
        .iter()
        .filter_map(|s| uni_data.get(*s).map(|df| df.height()))
        .min()
        .unwrap_or(0)
        .min(2800);
    for df in uni_data.values_mut() {
        if df.height() > uni_min {
            *df = df.slice(0, uni_min);
        }
    }

    let mut uni_results: Vec<UniverseResult> = Vec::new();
    println!(
        "  {:>13} | {:>5} | {:>12} | {:>8} | {:>8} | {:>5}",
        "Universe", "Hold", "Return%", "Sharpe", "MaxDD%", "Trades"
    );
    println!("  {}", "─".repeat(65));

    for &(uni_name, syms) in UNIVERSES {
        let mut local_min = usize::MAX;
        let mut local_cache = HashMap::new();
        for &sym in syms {
            if let Some(df) = uni_data.get(sym) {
                local_min = local_min.min(df.height());
                local_cache.insert(sym.to_string(), df.clone());
            }
        }
        for df in local_cache.values_mut() {
            if df.height() > local_min {
                *df = df.slice(0, local_min);
            }
        }

        for &hold_bars in HOLDS {
            let (ret, sh, dd, trades) =
                run_ad_backtest_universe(&local_cache, 0, local_min, AD_PERIOD, hold_bars, syms);
            uni_results.push(UniverseResult {
                universe: uni_name.to_string(),
                hold: hold_bars,
                return_pct: ret,
                sharpe: sh,
                max_dd_pct: dd.abs(),
                trades,
            });
        }
        println!("  {}", "─".repeat(65));
    }

    // 9-uni ranking
    let mut ranked: Vec<(usize, f64, f64, f64, usize, f64)> = Vec::new();
    for &h in HOLDS {
        let subset: Vec<&UniverseResult> = uni_results.iter().filter(|r| r.hold == h).collect();
        if subset.is_empty() {
            continue;
        }
        let avg_sh = subset.iter().map(|r| r.sharpe).sum::<f64>() / subset.len() as f64;
        let avg_ret = subset.iter().map(|r| r.return_pct).sum::<f64>() / subset.len() as f64;
        let worst_dd = subset
            .iter()
            .map(|r| r.max_dd_pct)
            .fold(f64::INFINITY, f64::min);
        let valid = subset
            .iter()
            .filter(|r| r.trades >= MIN_TRADES && r.return_pct > 0.0)
            .count();
        let avg_tr = subset.iter().map(|r| r.trades as f64).sum::<f64>() / subset.len() as f64;
        ranked.push((h, avg_sh, avg_ret, worst_dd, valid, avg_tr));
    }
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!("\n  9-UNIVERSE RANKING (by Avg Sharpe):");
    println!(
        "  {:>6} | {:>9} | {:>10} | {:>8} | {:>6} | {:>7}",
        "Hold", "AvgSharpe", "AvgRet%", "WorstDD", "Valid", "AvgTrades"
    );
    println!("{}", "─".repeat(55));
    for (i, (h, sh, ret, dd, v, avg_tr)) in ranked.iter().take(15).enumerate() {
        let m = if *h == 21 {
            " ←BASELINE"
        } else if i == 0 {
            " ★WINNER"
        } else {
            ""
        };
        println!(
            "  {:>6} | {:>9.2} | {:>+10.1}% | {:>7.1}% | {:>6} | {:>7.0}{}",
            h, sh, ret, dd, v, avg_tr, m
        );
    }

    // ── Composite ranking (WF Sharpe + 9-uni Sharpe) ───────────────────────
    println!("\n{}", "=".repeat(72));
    println!("  COMPOSITE RANKING (WF Sharpe × 9-uni Sharpe)");
    println!("{}", "─".repeat(62));

    let mut composite: Vec<(usize, f64, f64, f64)> = Vec::new();
    for r in &wf_results {
        let uni_entry = ranked.iter().find(|x| x.0 == r.hold);
        let uni_sh = uni_entry.map(|x| x.1).unwrap_or(0.0);
        // Geometric mean of walk-forward and universe scores
        let score = (r.avg_sharpe.max(0.0) * uni_sh.max(0.0)).sqrt();
        composite.push((r.hold, score, r.avg_sharpe, uni_sh));
    }
    composite.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!(
        "  {:>6} | {:>10} | {:>10} | {:>10}",
        "Hold", "Composite", "WF Sharpe", "9-uni Sharpe"
    );
    println!("{}", "─".repeat(42));
    for (i, (h, score, wf_sh, uni_sh)) in composite.iter().take(20).enumerate() {
        let m = if *h == 21 {
            " ←BASELINE"
        } else if i == 0 {
            " ★WINNER"
        } else {
            ""
        };
        println!(
            "  {:>6} | {:>10.3} | {:>+10.3} | {:>+10.3}{}",
            h, score, wf_sh, uni_sh, m
        );
    }

    let composite_winner = composite[0].0;
    let winner_wf = wf_results
        .iter()
        .find(|r| r.hold == composite_winner)
        .unwrap();
    let winner_uni = ranked.iter().find(|r| r.0 == composite_winner).unwrap();
    println!(
        "\n  *** COMPOSITE WINNER: HOLD_BARS = {} ***",
        composite_winner
    );
    println!(
        "      WF Sharpe: {:.3} | 9-uni Sharpe: {:.3} | Composite: {:.3}",
        winner_wf.avg_sharpe, winner_uni.1, composite[0].1
    );

    // ── Export CSVs ───────────────────────────────────────────────────────
    write_wf_csv(&wf_results)?;
    write_uni_csv(&uni_results, HOLDS)?;
    write_equity_csv(&equity_curves, &SYMS)?;
    write_composite_csv(&composite)?;
    println!("\nWrote: snapshots/hold_period_sweep.csv");
    println!("Wrote: snapshots/hold_period_equity.csv");
    println!("Wrote: snapshots/hold_period_composite.csv");

    Ok(())
}

// ── Backtest with equity curve ─────────────────────────────────────────────────

fn run_ad_backtest_with_equity(
    cache: &HashMap<String, DataFrame>,
    start: usize,
    end: usize,
    period: usize,
    hold_bars: usize,
    sym_list: &[&str],
) -> (f64, f64, f64, usize, Vec<f64>) {
    let syms: Vec<String> = sym_list.iter().map(|s| s.to_string()).collect();

    // Precompute A/D lines
    let ad_lines: HashMap<String, Vec<f64>> = syms
        .iter()
        .map(|sym| (sym.clone(), compute_ad_line(cache.get(sym).unwrap())))
        .collect();

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets: Vec<f64> = Vec::new();
    let mut equity_curve: Vec<f64> = vec![equity];
    let mut pos: Option<(String, usize, f64)> = None;
    let mut bar = start;

    while bar < end {
        if pos.is_none() {
            let mut best_sym: Option<(String, f64)> = None;
            for sym in &syms {
                let df = cache.get(sym).unwrap();
                let n = df.height();
                let idx = bar.saturating_sub(1);
                if idx < period + 1 || idx >= n {
                    bar += 1;
                    continue;
                }
                let ad_line = ad_lines.get(sym).unwrap();
                let ad_now = ad_line[idx];
                let ad_past = ad_line[idx - period];
                let mom = ad_now - ad_past;
                if mom > 0.0 {
                    if let Some((_, best_mom)) = &best_sym {
                        if mom > *best_mom {
                            best_sym = Some((sym.clone(), mom));
                        }
                    } else {
                        best_sym = Some((sym.clone(), mom));
                    }
                }
            }
            if let Some((sym, _)) = best_sym {
                let df = cache.get(&sym).unwrap();
                let open_ch = df.column("open").unwrap().f64().unwrap();
                let entry_idx = bar.min(df.height() - 1);
                let entry_price = open_ch.get(entry_idx).unwrap_or(0.0);
                if entry_price > 0.0 {
                    pos = Some((sym.clone(), bar, entry_price));
                }
            }
            bar += 1;
            equity_curve.push(equity);
            continue;
        }

        let (sym, entry_bar, entry_px) = pos.as_ref().unwrap();
        let df = cache.get(sym).unwrap();
        let n = df.height();
        let cur_bar = bar;

        if cur_bar >= *entry_bar + hold_bars || cur_bar >= end - 1 {
            let close_ch = df.column("close").unwrap().f64().unwrap();
            let exit_idx = cur_bar.min(n - 1);
            let exit_price = close_ch.get(exit_idx).unwrap_or(0.0);
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
        equity_curve.push(equity);
        bar += 1;
    }

    // Close open position at end
    if let Some((sym, _entry_bar, entry_px)) = pos {
        let df = cache.get(&sym).unwrap();
        let close_ch = df.column("close").unwrap().f64().unwrap();
        let exit_price = close_ch.get((end - 1).min(df.height() - 1)).unwrap_or(0.0);
        if entry_px > 0.0 && exit_price > 0.0 {
            let gross = (exit_price / entry_px - 1.0) - TAKER_FEE;
            equity *= 1.0 + gross;
            rets.push(gross);
        }
        equity_curve.push(equity);
    }

    let ret = (equity - 1.0) * 100.0;
    let sh = if rets.len() < 2 {
        0.0
    } else {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let std =
            (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (rets.len() - 1) as f64).sqrt();
        if std == 0.0 {
            0.0
        } else {
            mean / std
        }
    };

    (ret, sh, max_dd * 100.0, trades, equity_curve)
}

fn run_ad_backtest_universe(
    cache: &HashMap<String, DataFrame>,
    start: usize,
    end: usize,
    period: usize,
    hold_bars: usize,
    sym_list: &[&str],
) -> (f64, f64, f64, usize) {
    let syms: Vec<String> = sym_list.iter().map(|s| s.to_string()).collect();
    let ad_lines: HashMap<String, Vec<f64>> = syms
        .iter()
        .map(|sym| (sym.clone(), compute_ad_line(cache.get(sym).unwrap())))
        .collect();

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets: Vec<f64> = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;
    let mut bar = start;

    while bar < end {
        if pos.is_none() {
            let mut best_sym: Option<(String, f64)> = None;
            for sym in &syms {
                let df = cache.get(sym).unwrap();
                let n = df.height();
                let idx = bar.saturating_sub(1);
                if idx < period + 1 || idx >= n {
                    bar += 1;
                    continue;
                }
                let ad_line = ad_lines.get(sym).unwrap();
                let ad_now = ad_line[idx];
                let ad_past = ad_line[idx - period];
                let mom = ad_now - ad_past;
                if mom > 0.0 {
                    if let Some((_, best_mom)) = &best_sym {
                        if mom > *best_mom {
                            best_sym = Some((sym.clone(), mom));
                        }
                    } else {
                        best_sym = Some((sym.clone(), mom));
                    }
                }
            }
            if let Some((sym, _)) = best_sym {
                let df = cache.get(&sym).unwrap();
                let open_ch = df.column("open").unwrap().f64().unwrap();
                let entry_idx = bar.min(df.height() - 1);
                let entry_price = open_ch.get(entry_idx).unwrap_or(0.0);
                if entry_price > 0.0 {
                    pos = Some((sym.clone(), bar, entry_price));
                }
            }
            bar += 1;
            continue;
        }

        let (sym, entry_bar, entry_px) = pos.as_ref().unwrap();
        let df = cache.get(sym).unwrap();
        let n = df.height();
        let cur_bar = bar;

        if cur_bar >= *entry_bar + hold_bars || cur_bar >= end - 1 {
            let close_ch = df.column("close").unwrap().f64().unwrap();
            let exit_idx = cur_bar.min(n - 1);
            let exit_price = close_ch.get(exit_idx).unwrap_or(0.0);
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

    if let Some((sym, _entry_bar, entry_px)) = pos {
        let df = cache.get(&sym).unwrap();
        let close_ch = df.column("close").unwrap().f64().unwrap();
        let exit_price = close_ch.get((end - 1).min(df.height() - 1)).unwrap_or(0.0);
        if entry_px > 0.0 && exit_price > 0.0 {
            let gross = (exit_price / entry_px - 1.0) - TAKER_FEE;
            equity *= 1.0 + gross;
            rets.push(gross);
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sh = if rets.len() < 2 {
        0.0
    } else {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let std =
            (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (rets.len() - 1) as f64).sqrt();
        if std == 0.0 {
            0.0
        } else {
            mean / std
        }
    };

    (ret, sh, max_dd * 100.0, trades)
}

fn compute_ad_line(df: &DataFrame) -> Vec<f64> {
    let close_ch = df.column("close").unwrap().f64().unwrap();
    let high_ch = df.column("high").unwrap().f64().unwrap();
    let low_ch = df.column("low").unwrap().f64().unwrap();
    let vol_ch = df.column("volume").unwrap().f64().unwrap();
    let n = df.height();
    let mut ad_line = Vec::with_capacity(n);
    let mut ad: f64 = 0.0;
    for i in 0..n {
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
        ad += mf * v;
        ad_line.push(ad);
    }
    ad_line
}

// ── CSV Writers ────────────────────────────────────────────────────────────────

fn write_wf_csv(results: &[ResultRow]) -> Result<()> {
    let mut lines = vec![
        "type,hold,windows,passes,pass_rate_pct,avg_ret_pct,avg_sharpe,worst_dd_pct,avg_trades"
            .to_string(),
    ];
    for r in results {
        let win_pct = if r.windows > 0 {
            r.passes as f64 / r.windows as f64 * 100.0
        } else {
            0.0
        };
        lines.push(format!(
            "wf,{},{},{},{:.1},{:.2},{:.3},{:.2},{:.1}",
            r.hold, r.windows, r.passes, win_pct, r.avg_ret, r.avg_sharpe, r.worst_dd, r.avg_trades
        ));
    }
    std::fs::write("snapshots/hold_period_sweep.csv", lines.join("\n"))?;
    Ok(())
}

fn write_uni_csv(results: &[UniverseResult], _holds: &[usize]) -> Result<()> {
    let mut csv = std::fs::read_to_string("snapshots/hold_period_sweep.csv").unwrap_or_default();
    if !csv.is_empty() && !csv.ends_with('\n') {
        csv.push('\n');
    }
    for r in results {
        csv.push_str(&format!(
            "uni,{},{},{:.1},{:.3},{:.1},{},0.0\n",
            r.universe, r.hold, r.return_pct, r.sharpe, r.max_dd_pct, r.trades
        ));
    }
    std::fs::write("snapshots/hold_period_sweep.csv", &csv)?;
    Ok(())
}

fn write_composite_csv(composite: &[(usize, f64, f64, f64)]) -> Result<()> {
    let mut lines = vec!["hold,composite_score,wf_sharpe,uni_sharpe".to_string()];
    for (h, score, wf_sh, uni_sh) in composite {
        lines.push(format!("{},{:.4},{:.4},{:.4}", h, score, wf_sh, uni_sh));
    }
    std::fs::write("snapshots/hold_period_composite.csv", lines.join("\n"))?;
    Ok(())
}

fn write_equity_csv(equity_curves: &HashMap<usize, Vec<f64>>, _syms: &[&str]) -> Result<()> {
    // Find the max length across all curves
    let max_len = equity_curves.values().map(|v| v.len()).max().unwrap_or(0);
    if max_len == 0 {
        return Ok(());
    }

    // Build header: hold_3,hold_4,...,hold_63
    let mut holds: Vec<usize> = equity_curves.keys().copied().collect();
    holds.sort_unstable();
    let header = std::iter::once("bar".to_string())
        .chain(holds.iter().map(|&h| format!("hold_{}", h)))
        .collect::<Vec<_>>()
        .join(",");
    let mut lines = vec![header];

    for i in 0..max_len {
        let mut row = vec![format!("{}", i)];
        for &h in &holds {
            if let Some(eq) = equity_curves.get(&h) {
                if i < eq.len() {
                    row.push(format!("{:.6}", eq[i]));
                } else {
                    row.push(String::new());
                }
            } else {
                row.push(String::new());
            }
        }
        lines.push(row.join(","));
    }

    std::fs::write("snapshots/hold_period_equity.csv", lines.join("\n"))?;
    Ok(())
}

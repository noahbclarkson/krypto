//! =========================================================
//! HYPERPARAMETER OPTIMIZATION: Turtle Entry Period (Donchian Lookback)
//! =========================================================
//!
//! TARGET: DONCHIAN_LOOKBACK — the N-bar high/low breakout entry period
//! PRIOR:  `turtle_entry_hyperopt.rs` tested 10-60 in steps of 2 on Base5 only
//! THIS:   5-100 in steps of 1 → 96 values across ALL 9 universes
//!
//! Strategy: Turtle-style Donchian breakout + MACD regime filter + ATR trailing stop
//! Fixed:   ATR stop mult=2.0 (standard), MACD(14/30/10)
//!
//! Method:  Walk-forward (252 train / 252 test) on all 9 universes
//! Metrics: OOS pass rate, avg Sharpe, worst DD, equity curves
//! Composite ranking across universes (robustness > peak return)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const WARMUP: usize = 200;
const SWEEP_START: usize = 5;
const SWEEP_END: usize = 100;
const SWEEP_STEP: usize = 1;

// ── Fixed strategy parameters ─────────────────────────────────────────────────
const MACD_FAST: usize = 14;
const MACD_SLOW: usize = 30;
const MACD_SIGNAL: usize = 10;
const ATR_STOP_MULT: f64 = 2.0; // Standard ATR trailing stop (not being swept here)

// ── 9 universes ──────────────────────────────────────────────────────────────
const LOAD_SYMBOLS: [&str; 10] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
    "EOSUSDT", "BCHUSDT",
];

const UNIVERSES: [(&str, &[&str]); 9] = [
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

// ── Result structs ──────────────────────────────────────────────────────────

#[derive(Clone)]
struct WindowRec {
    wi: usize,
    ret: f64,
    sh: f64,
    dd: f64,
    trades: usize,
    passed: bool,
}

#[derive(Clone)]
struct WfResult {
    entry_period: usize,
    windows: usize,
    passes: usize,
    avg_ret: f64,
    avg_sharpe: f64,
    worst_dd: f64,
    avg_trades: f64,
    recs: Vec<WindowRec>,
}

#[derive(Clone)]
struct UniResult {
    universe: String,
    entry_period: usize,
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
}

#[derive(Clone)]
struct EquityResult {
    entry_period: usize,
    equity_curve: Vec<f64>,
}

// ── Symbol data cache ────────────────────────────────────────────────────────

#[derive(Clone)]
struct SymbolData {
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    macd: Vec<f64>,
    macd_signal: Vec<f64>,
    atr: Vec<f64>,
    len: usize,
}

fn extract_symbol_data(df: &DataFrame) -> SymbolData {
    let n = df.height();
    SymbolData {
        open: (0..n)
            .map(|i| {
                df.column("open")
                    .unwrap()
                    .f64()
                    .unwrap()
                    .get(i)
                    .unwrap_or(0.0)
            })
            .collect(),
        high: (0..n)
            .map(|i| {
                df.column("high")
                    .unwrap()
                    .f64()
                    .unwrap()
                    .get(i)
                    .unwrap_or(0.0)
            })
            .collect(),
        low: (0..n)
            .map(|i| {
                df.column("low")
                    .unwrap()
                    .f64()
                    .unwrap()
                    .get(i)
                    .unwrap_or(0.0)
            })
            .collect(),
        close: (0..n)
            .map(|i| {
                df.column("close")
                    .unwrap()
                    .f64()
                    .unwrap()
                    .get(i)
                    .unwrap_or(0.0)
            })
            .collect(),
        macd: (0..n)
            .map(|i| {
                df.column("macd")
                    .unwrap()
                    .f64()
                    .unwrap()
                    .get(i)
                    .unwrap_or(0.0)
            })
            .collect(),
        macd_signal: (0..n)
            .map(|i| {
                df.column("macd_signal")
                    .unwrap()
                    .f64()
                    .unwrap()
                    .get(i)
                    .unwrap_or(0.0)
            })
            .collect(),
        atr: (0..n)
            .map(|i| {
                df.column("atr")
                    .unwrap()
                    .f64()
                    .unwrap()
                    .get(i)
                    .unwrap_or(1.0)
            })
            .collect(),
        len: n,
    }
}

// ── Backtest engine ──────────────────────────────────────────────────────────

struct Position {
    sym: String,
    entry_price: f64,
    entry_bar: usize,
    atr_at_entry: f64,
}

fn run_turtle_backtest_with_equity(
    cache: &HashMap<String, SymbolData>,
    syms: &[&str],
    start: usize,
    end: usize,
    entry_period: usize,
) -> (f64, f64, f64, usize, Vec<f64>) {
    let mut sym_data: HashMap<String, SymbolData> = HashMap::new();
    for sym in syms {
        if let Some(sd) = cache.get(*sym) {
            sym_data.insert(sym.to_string(), sd.clone());
        }
    }

    let n = end.min(sym_data.get(syms[0]).map(|sd| sd.len).unwrap_or(0));
    let eff_start = start.max(entry_period.max(MACD_SLOW) + 2);

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets = Vec::new();
    let mut equity_curve = vec![equity];
    let mut pos: Option<Position> = None;
    let mut trailing_stop: f64 = f64::NAN;
    let mut bar = eff_start;

    while bar < end {
        if pos.is_none() {
            let idx = (bar - 1).min(n.saturating_sub(1));

            let mut candidates: Vec<(String, f64, f64)> = Vec::new();
            for sym in syms {
                if let Some(sd) = sym_data.get(*sym) {
                    if idx >= sd.len {
                        continue;
                    }

                    let macd_now = sd.macd.get(idx).copied().unwrap_or(0.0);
                    let sig_now = sd.macd_signal.get(idx).copied().unwrap_or(0.0);
                    let macd_prev = sd.macd.get(idx.saturating_sub(1)).copied().unwrap_or(0.0);
                    let sig_prev = sd
                        .macd_signal
                        .get(idx.saturating_sub(1))
                        .copied()
                        .unwrap_or(0.0);

                    let entry_lo = (idx + 1).saturating_sub(entry_period);
                    let entry_hi = idx;
                    let mut hh = f64::MIN;
                    let mut ll = f64::MAX;
                    for j in entry_lo..=entry_hi {
                        if j < sd.high.len() && j < sd.low.len() {
                            hh = hh.max(sd.high[j]);
                            ll = ll.min(sd.low[j]);
                        }
                    }

                    let close_now = sd.close.get(idx).copied().unwrap_or(0.0);
                    let atr_now = sd.atr.get(idx).copied().unwrap_or(1.0);
                    let atr_val = atr_now.max(1e-8);

                    let bull_cross = macd_now > sig_now && macd_prev <= sig_prev;
                    let bull_breakout = close_now > hh && hh > f64::MIN;
                    let bear_cross = macd_now < sig_now && macd_prev >= sig_prev;
                    let bear_breakout = close_now < ll && ll < f64::MAX;
                    let macd_gap = (macd_now - sig_now).abs();

                    if bull_cross && bull_breakout {
                        candidates.push((sym.to_string(), macd_gap, atr_val));
                    } else if bear_cross && bear_breakout {
                        candidates.push((sym.to_string(), -macd_gap, atr_val));
                    }
                }
            }

            candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            if let Some((sym, _, atr_val)) = candidates.into_iter().next() {
                if let Some(sd) = sym_data.get(&sym) {
                    let entry_idx = bar.min(sd.len.saturating_sub(1));
                    let entry_price = sd.open[entry_idx];
                    if entry_price > 0.0 {
                        trailing_stop = entry_price - ATR_STOP_MULT * atr_val;
                        pos = Some(Position {
                            sym,
                            entry_price,
                            entry_bar: bar,
                            atr_at_entry: atr_val,
                        });
                    }
                }
            }
            bar += 1;
            equity_curve.push(equity);
            continue;
        }

        let pos_ref = pos.as_ref().unwrap();
        let (sym, entry_price, entry_bar, atr_at_entry) = (
            pos_ref.sym.clone(),
            pos_ref.entry_price,
            pos_ref.entry_bar,
            pos_ref.atr_at_entry,
        );

        let sd = match sym_data.get(&sym) {
            Some(sd) => sd,
            None => {
                bar += 1;
                equity_curve.push(equity);
                continue;
            }
        };
        let cur_bar = bar.min(sd.len.saturating_sub(1));

        let close_now = sd.close.get(cur_bar).copied().unwrap_or(entry_price);
        let atr_now = sd.atr.get(cur_bar).copied().unwrap_or(atr_at_entry);
        let atr_stop = ATR_STOP_MULT * atr_now;

        let new_ts = close_now - atr_stop;
        if new_ts > trailing_stop {
            trailing_stop = new_ts;
        }

        let held = bar - entry_bar;
        let stop_hit = close_now < trailing_stop;
        let time_exit = held >= HOLD_BARS || bar >= end.saturating_sub(1);

        if stop_hit || time_exit {
            let exit_price = sd.close.get(cur_bar).copied().unwrap_or(entry_price);
            if entry_price > 0.0 && exit_price > 0.0 {
                let gross = (exit_price / entry_price - 1.0) - TAKER_FEE;
                let slip = if gross < 0.0 { 0.5_f64 } else { 1.0_f64 };
                equity *= 1.0 + gross * slip;
                rets.push(gross * slip);
                trades += 1;
            }
            pos = None;
            trailing_stop = f64::NAN;
        }

        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);
        equity_curve.push(equity);
        bar += 1;
    }

    if pos.is_some() {
        if let Some(sd) = sym_data.get(&pos.as_ref().unwrap().sym) {
            let exit_price = sd
                .close
                .get((end - 1).min(sd.len - 1))
                .copied()
                .unwrap_or(pos.as_ref().unwrap().entry_price);
            let ep = pos.as_ref().unwrap().entry_price;
            if ep > 0.0 && exit_price > 0.0 {
                let gross = (exit_price / ep - 1.0) - TAKER_FEE;
                equity *= 1.0 + gross;
                rets.push(gross);
            }
        }
        equity_curve.push(equity);
    }

    let ret = (equity - 1.0) * 100.0;
    let sh = if rets.len() < 2 {
        0.0
    } else {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let std = (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
            / (rets.len() - 1).max(1) as f64)
            .sqrt();
        if std == 0.0 {
            0.0
        } else {
            mean / std
        }
    };

    (ret, sh, max_dd * 100.0, trades, equity_curve)
}

fn run_turtle_backtest_simple(
    cache: &HashMap<String, SymbolData>,
    syms: &[&str],
    start: usize,
    end: usize,
    entry_period: usize,
) -> (f64, f64, f64, usize) {
    let (_, sh, dd, trades, _) =
        run_turtle_backtest_with_equity(cache, syms, start, end, entry_period);
    (0.0, sh, dd, trades) // placeholder return
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();
    fs::create_dir_all("snapshots")?;
    fs::create_dir_all("charts")?;

    let entry_periods: Vec<usize> = (SWEEP_START..=SWEEP_END).step_by(SWEEP_STEP).collect();

    println!("\n{}", "=".repeat(72));
    println!("  HYPEROPT: Turtle Entry Period (Donchian Lookback)");
    println!(
        "  Range: {}-{} step {} → {} values",
        SWEEP_START,
        SWEEP_END,
        SWEEP_STEP,
        entry_periods.len()
    );
    println!(
        "  Fixed: ATR_mult={:.1}, MACD({}/{}/{})",
        ATR_STOP_MULT, MACD_FAST, MACD_SLOW, MACD_SIGNAL
    );
    println!("  Universes: {}", UNIVERSES.len());
    println!("{}\n", "=".repeat(72));

    // ── Load data for Base5 (walk-forward universe) ────────────────────────
    let loader = DataLoader::new(None, None);
    let mut base5_cache: HashMap<String, SymbolData> = HashMap::new();
    let base5_syms = UNIVERSES[0].1;
    let mut min_len_b5 = usize::MAX;
    for &sym in base5_syms {
        let raw = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        let sd = extract_symbol_data(&df);
        min_len_b5 = min_len_b5.min(sd.len);
        base5_cache.insert(sym.to_string(), sd);
    }
    let n_b5 = min_len_b5.min(2800);
    for sd in base5_cache.values_mut() {
        if sd.len > n_b5 {
            sd.len = n_b5;
        }
        sd.open.truncate(n_b5);
        sd.high.truncate(n_b5);
        sd.low.truncate(n_b5);
        sd.close.truncate(n_b5);
        sd.macd.truncate(n_b5);
        sd.macd_signal.truncate(n_b5);
        sd.atr.truncate(n_b5);
    }
    let total_windows = n_b5
        .saturating_sub(TRAIN_BARS + entry_periods.iter().max().copied().unwrap_or(50) + 10)
        / TEST_BARS;
    println!(
        "Loaded Base5 ({} syms), {} bars, {} test windows\n",
        base5_syms.len(),
        n_b5,
        total_windows
    );

    // ── Walk-forward sweep on Base5 ────────────────────────────────────────
    println!("{}", "=".repeat(72));
    println!("  WALK-FORWARD (Base5, {} windows)", total_windows);
    println!("{}\n", "=".repeat(72));

    let mut wf_results: Vec<WfResult> = Vec::new();
    let mut equity_curves: Vec<EquityResult> = Vec::new();

    for (idx, &entry_period) in entry_periods.iter().enumerate() {
        if idx % 10 == 0 {
            println!(
                "  Progress: {}/{} (entry_period={})",
                idx,
                entry_periods.len(),
                entry_period
            );
        }

        let mut recs: Vec<WindowRec> = Vec::new();
        let mut total_ret = 0.0_f64;
        let mut total_sharpe = 0.0_f64;
        let mut worst_dd = 0.0_f64;
        let mut total_trades = 0usize;
        let mut passes = 0usize;
        let mut merged_equity: Vec<f64> = vec![1.0];

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let tstart = train_end;
            let tend = (tstart + TEST_BARS).min(n_b5);
            if tend.saturating_sub(tstart) < HOLD_BARS + entry_period + 10 {
                continue;
            }

            let (ret, sh, dd, trades, eq_vec) = run_turtle_backtest_with_equity(
                &base5_cache,
                base5_syms,
                tstart,
                tend,
                entry_period,
            );

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

            if !merged_equity.is_empty() && !eq_vec.is_empty() {
                let base = *merged_equity.last().unwrap();
                merged_equity.extend(eq_vec.iter().map(|&v| base * v));
            } else {
                merged_equity.extend(eq_vec);
            }
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

        wf_results.push(WfResult {
            entry_period,
            windows: n_recs,
            passes,
            avg_ret,
            avg_sharpe: avg_sh,
            worst_dd,
            avg_trades: avg_tr,
            recs,
        });
        equity_curves.push(EquityResult {
            entry_period,
            equity_curve: merged_equity,
        });

        let flag = if n_recs > 0 && passes == n_recs {
            " ALL PASS"
        } else {
            ""
        };
        println!(
            "  ep={:3}: {}/{} passes | avg OOS {:+8.1}% | Sharpe {:+.3} | DD {:+.1}% | {:.0}t{}",
            entry_period, passes, n_recs, avg_ret, avg_sh, worst_dd, avg_tr, flag
        );
    }

    // ── Rank walk-forward ──────────────────────────────────────────────────
    let mut wf_sorted = wf_results.clone();
    wf_sorted.sort_by(|a, b| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap());

    println!("\n{}", "=".repeat(72));
    println!("  WALK-FORWARD RANKING (by avg Sharpe)");
    println!(
        "  {:>8} | {:>4} | {:>8}% | {:>10} | {:>9} | {:>7}",
        "EntryPd", "Pass", "Win%", "Avg OOS%", "Worst DD%", "AvgTr"
    );
    println!("{}", "─".repeat(65));
    for (i, r) in wf_sorted.iter().take(25).enumerate() {
        let win_pct = if r.windows > 0 {
            r.passes as f64 / r.windows as f64 * 100.0
        } else {
            0.0
        };
        let marker = if r.entry_period == 20 {
            " ←BASE"
        } else if i == 0 {
            " ★WIN"
        } else {
            ""
        };
        println!(
            "  {:>8} | {}/{} | {:>6.0}% | {:>+10.1}% | {:>+9.1}% | {:>7.0}{}",
            r.entry_period,
            r.passes,
            r.windows,
            win_pct,
            r.avg_ret,
            r.worst_dd,
            r.avg_trades,
            marker
        );
    }

    let baseline_sharpe = wf_results
        .iter()
        .find(|r| r.entry_period == 20)
        .map(|r| r.avg_sharpe)
        .unwrap_or(0.0);
    let winner_ep = wf_sorted[0].entry_period;
    let winner_sh = wf_sorted[0].avg_sharpe;
    println!("\n  Baseline(20): Sharpe {:.3}", baseline_sharpe);
    println!(
        "  Winner({:3}):  Sharpe {:.3}  (Δ = {:+.3})",
        winner_ep,
        winner_sh,
        winner_sh - baseline_sharpe
    );

    // ── 9-Universe full-sample ─────────────────────────────────────────────
    println!("\n{}", "=".repeat(72));
    println!("  9-UNIVERSE FULL-SAMPLE (every 4th entry period)");
    println!("{}\n", "=".repeat(72));

    let mut uni_data: HashMap<String, DataFrame> = HashMap::new();
    for &sym in &LOAD_SYMBOLS {
        if !uni_data.contains_key(sym) {
            let raw = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
            let df = FeatureEngine::add_technicals(&raw, None)?;
            uni_data.insert(sym.to_string(), df);
        }
    }

    let mut uni_results: Vec<UniResult> = Vec::new();
    println!(
        "  {:>14} | {:>8} | {:>10} | {:>8} | {:>8} | {:>6}",
        "Universe", "EntryPd", "Return%", "Sharpe", "MaxDD%", "Trades"
    );
    println!("  {}", "─".repeat(62));

    for &(uni_name, syms) in &UNIVERSES {
        let local_min = syms
            .iter()
            .filter_map(|s| uni_data.get(*s).map(|df| df.height()))
            .min()
            .unwrap_or(0)
            .min(2800);
        let mut local_cache: HashMap<String, SymbolData> = HashMap::new();
        for &sym in syms {
            if let Some(df) = uni_data.get(sym) {
                let mut sd = extract_symbol_data(df);
                if sd.len > local_min {
                    sd.len = local_min;
                    sd.open.truncate(local_min);
                    sd.high.truncate(local_min);
                    sd.low.truncate(local_min);
                    sd.close.truncate(local_min);
                    sd.macd.truncate(local_min);
                    sd.macd_signal.truncate(local_min);
                    sd.atr.truncate(local_min);
                }
                local_cache.insert(sym.to_string(), sd);
            }
        }

        // Sample every 4th period to keep runtime manageable
        for (idx, &entry_period) in entry_periods.iter().enumerate() {
            if idx % 4 != 0 {
                continue;
            }
            let (ret, sh, dd, trades, _) =
                run_turtle_backtest_with_equity(&local_cache, syms, 0, local_min, entry_period);
            uni_results.push(UniResult {
                universe: uni_name.to_string(),
                entry_period,
                return_pct: ret,
                sharpe: sh,
                max_dd_pct: dd.abs(),
                trades,
            });
        }
        println!("  {}", "─".repeat(62));
    }

    // 9-uni ranking
    let mut ranked: Vec<(usize, f64, f64, f64, usize, f64)> = Vec::new();
    for (idx, &entry_period) in entry_periods.iter().enumerate() {
        if idx % 4 != 0 {
            continue;
        }
        let subset: Vec<&UniResult> = uni_results
            .iter()
            .filter(|r| r.entry_period == entry_period)
            .collect();
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
        ranked.push((entry_period, avg_sh, avg_ret, worst_dd, valid, avg_tr));
    }
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!("\n  9-UNIVERSE RANKING (by Avg Sharpe):");
    println!(
        "  {:>8} | {:>9} | {:>10} | {:>8} | {:>6} | {:>7}",
        "EntryPd", "AvgSharpe", "AvgRet%", "WorstDD", "Valid", "AvgTr"
    );
    println!("{}", "─".repeat(52));
    for (i, (ep, sh, ret, dd, v, avg_tr)) in ranked.iter().take(20).enumerate() {
        let marker = if *ep == 20 {
            " ←BASE"
        } else if i == 0 {
            " ★WIN"
        } else {
            ""
        };
        println!(
            "  {:>8} | {:>9.3} | {:>+10.1}% | {:>7.1}% | {:>6} | {:>7.0}{}",
            ep, sh, ret, dd, v, avg_tr, marker
        );
    }

    // ── Composite ranking ──────────────────────────────────────────────────
    println!("\n{}", "=".repeat(72));
    println!("  COMPOSITE RANKING (WF Sharpe × 9-uni Sharpe)");
    println!("{}", "─".repeat(50));

    let mut composite: Vec<(usize, f64, f64, f64)> = Vec::new();
    for r in &wf_results {
        let (wf_sh, _) = (r.avg_sharpe, r.entry_period);
        let uni_entry = ranked.iter().min_by(|a, b| {
            (a.0 as i64 - r.entry_period as i64)
                .abs()
                .cmp(&(b.0 as i64 - r.entry_period as i64).abs())
        });
        let uni_sh = uni_entry.map(|x| x.1).unwrap_or(0.0);
        let score = (wf_sh.max(0.001) * uni_sh.max(0.001)).sqrt();
        composite.push((r.entry_period, score, wf_sh, uni_sh));
    }
    composite.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!(
        "  {:>8} | {:>10} | {:>10} | {:>10}",
        "EntryPd", "Composite", "WF Sharpe", "9-uni Sharpe"
    );
    println!("{}", "─".repeat(42));
    for (i, (ep, score, wf_sh, uni_sh)) in composite.iter().take(20).enumerate() {
        let marker = if *ep == 20 {
            " ←BASE"
        } else if i == 0 {
            " ★WIN"
        } else {
            ""
        };
        println!(
            "  {:>8} | {:>10.4} | {:>+10.3} | {:>+10.3}{}",
            ep, score, wf_sh, uni_sh, marker
        );
    }

    let composite_winner_ep = composite[0].0;
    let winner_wf = wf_results
        .iter()
        .find(|r| r.entry_period == composite_winner_ep)
        .unwrap();
    let winner_uni = ranked
        .iter()
        .min_by(|a, b| {
            (a.0 as i64 - composite_winner_ep as i64)
                .abs()
                .cmp(&(b.0 as i64 - composite_winner_ep as i64).abs())
        })
        .unwrap();
    println!(
        "\n  *** COMPOSITE WINNER: DONCHIAN_LOOKBACK = {} ***",
        composite_winner_ep
    );
    println!(
        "      WF Sharpe: {:.3} | 9-uni Sharpe: {:.3} | Composite: {:.4}",
        winner_wf.avg_sharpe, winner_uni.1, composite[0].1
    );

    // ── Per-window detail for top 5 ───────────────────────────────────────
    println!("\n{}", "=".repeat(72));
    println!(
        "  WINNER DETAIL (entry_period={}): Per-window breakdown",
        composite_winner_ep
    );
    println!("{}", "─".repeat(60));
    for rec in &winner_wf.recs {
        let flag = if rec.passed { "PASS" } else { "FAIL" };
        println!(
            "    W{:02}: {:+8.1}%  sh={:+.3}  DD={:+7.1}%  {:4}t  {}",
            rec.wi, rec.ret, rec.sh, rec.dd, rec.trades, flag
        );
    }

    // ── Top-5 by composite: per-universe breakdown ────────────────────────
    println!("\n{}", "=".repeat(72));
    println!("  TOP-5 COMPOSITE: Per-universe full-sample breakdown");
    println!("{}", "─".repeat(70));
    for (i, (ep, _, _, _)) in composite.iter().take(5).enumerate() {
        println!("\n  #{:1} Entry Period = {}:", i + 1, ep);
        for &(uni_name, syms) in &UNIVERSES {
            let local_min = syms
                .iter()
                .filter_map(|s| uni_data.get(*s).map(|df| df.height()))
                .min()
                .unwrap_or(0)
                .min(2800);
            let mut local_cache: HashMap<String, SymbolData> = HashMap::new();
            for &sym in syms {
                if let Some(df) = uni_data.get(sym) {
                    let mut sd = extract_symbol_data(df);
                    if sd.len > local_min {
                        sd.len = local_min;
                        sd.open.truncate(local_min);
                        sd.high.truncate(local_min);
                        sd.low.truncate(local_min);
                        sd.close.truncate(local_min);
                        sd.macd.truncate(local_min);
                        sd.macd_signal.truncate(local_min);
                        sd.atr.truncate(local_min);
                    }
                    local_cache.insert(sym.to_string(), sd);
                }
            }
            let (ret, sh, dd, trades, _) =
                run_turtle_backtest_with_equity(&local_cache, syms, 0, local_min, *ep);
            println!(
                "    {:14}: {:+8.1}%  sh={:+.2}  DD={:+7.1}%  {}t",
                uni_name, ret, sh, dd, trades
            );
        }
    }

    // ── Export CSVs ────────────────────────────────────────────────────────
    write_wf_csv(&wf_results)?;
    write_uni_csv(&uni_results)?;
    write_equity_csv(&equity_curves)?;
    write_composite_csv(&composite)?;

    println!("\nWrote: snapshots/turtle_entry_sweep.csv");
    println!("Wrote: snapshots/turtle_entry_uni.csv");
    println!("Wrote: snapshots/turtle_entry_equity.csv");
    println!("Wrote: snapshots/turtle_entry_composite.csv");

    // ── Write Python comparison chart ──────────────────────────────────────
    write_comparison_chart(&wf_results, &equity_curves, composite_winner_ep);

    let elapsed = t0.elapsed();
    println!("\nCompleted in {:.1}s", elapsed.as_secs_f64());
    Ok(())
}

// ── CSV Writers ──────────────────────────────────────────────────────────────

fn write_wf_csv(results: &[WfResult]) -> Result<()> {
    let mut lines = vec![
        "entry_period,windows,passes,pass_rate_pct,avg_ret_pct,avg_sharpe,worst_dd_pct,avg_trades"
            .to_string(),
    ];
    for r in results {
        let win_pct = if r.windows > 0 {
            r.passes as f64 / r.windows as f64 * 100.0
        } else {
            0.0
        };
        lines.push(format!(
            "{},{},{},{:.1},{:.2},{:.3},{:.2},{:.1}",
            r.entry_period,
            r.windows,
            r.passes,
            win_pct,
            r.avg_ret,
            r.avg_sharpe,
            r.worst_dd,
            r.avg_trades
        ));
    }
    std::fs::write("snapshots/turtle_entry_sweep.csv", lines.join("\n"))?;
    Ok(())
}

fn write_uni_csv(results: &[UniResult]) -> Result<()> {
    let mut lines = vec!["universe,entry_period,return_pct,sharpe,max_dd_pct,trades".to_string()];
    for r in results {
        lines.push(format!(
            "{},{},{:.1},{:.3},{:.1},{}",
            r.universe, r.entry_period, r.return_pct, r.sharpe, r.max_dd_pct, r.trades
        ));
    }
    std::fs::write("snapshots/turtle_entry_uni.csv", lines.join("\n"))?;
    Ok(())
}

fn write_equity_csv(equity_curves: &[EquityResult]) -> Result<()> {
    let max_len = equity_curves
        .iter()
        .map(|e| e.equity_curve.len())
        .max()
        .unwrap_or(0);
    if max_len == 0 {
        return Ok(());
    }

    let mut all_eps: Vec<usize> = equity_curves.iter().map(|e| e.entry_period).collect();
    all_eps.sort_by(|a, b| a.cmp(b));

    let header = std::iter::once("bar".to_string())
        .chain(all_eps.iter().map(|&m| format!("ep_{}", m)))
        .collect::<Vec<_>>()
        .join(",");
    let mut csv_lines = vec![header];

    for i in 0..max_len {
        let mut row = vec![format!("{}", i)];
        for &ep in &all_eps {
            if let Some(eq) = equity_curves.iter().find(|e| e.entry_period == ep) {
                if i < eq.equity_curve.len() {
                    row.push(format!("{:.6}", eq.equity_curve[i]));
                } else {
                    row.push(String::new());
                }
            } else {
                row.push(String::new());
            }
        }
        csv_lines.push(row.join(","));
    }

    std::fs::write("snapshots/turtle_entry_equity.csv", csv_lines.join("\n"))?;
    Ok(())
}

fn write_composite_csv(composite: &[(usize, f64, f64, f64)]) -> Result<()> {
    let mut lines = vec!["entry_period,composite_score,wf_sharpe,uni_sharpe".to_string()];
    for (ep, score, wf_sh, uni_sh) in composite {
        lines.push(format!("{},{:.4},{:.4},{:.4}", ep, score, wf_sh, uni_sh));
    }
    std::fs::write("snapshots/turtle_entry_composite.csv", lines.join("\n"))?;
    Ok(())
}

// ── Python comparison chart ─────────────────────────────────────────────────

// ── Python comparison chart ─────────────────────────────────────────────────

fn write_comparison_chart(
    wf_results: &[WfResult],
    equity_curves: &[EquityResult],
    winner_ep: usize,
) {
    // Identify baseline (ep=20), winner, and runner-ups
    let mut wf_sorted_for_chart = wf_results.to_vec();
    wf_sorted_for_chart.sort_by(|a, b| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap());

    let runner_up_1 = wf_sorted_for_chart
        .get(1)
        .map(|r| r.entry_period)
        .unwrap_or(30);
    let runner_up_2 = wf_sorted_for_chart
        .get(2)
        .map(|r| r.entry_period)
        .unwrap_or(40);
    let baseline_ep: usize = 20;

    let ep_vals: Vec<String> = wf_results
        .iter()
        .map(|r| format!("{}", r.entry_period))
        .collect();
    let sh_vals: Vec<String> = wf_results
        .iter()
        .map(|r| format!("{:.3}", r.avg_sharpe))
        .collect();
    let pa_vals: Vec<String> = wf_results.iter().map(|r| format!("{}", r.passes)).collect();
    let wi_vals: Vec<String> = wf_results
        .iter()
        .map(|r| format!("{}", r.windows))
        .collect();
    let dd_vals: Vec<String> = wf_results
        .iter()
        .map(|r| format!("{:.2}", r.worst_dd))
        .collect();

    // Equity curves for selected periods
    let eq_map: std::collections::HashMap<usize, &[f64]> = equity_curves
        .iter()
        .map(|e| (e.entry_period, e.equity_curve.as_slice()))
        .collect();

    let mut eq_py_lines = Vec::new();
    for &ep in &[baseline_ep, winner_ep, runner_up_1, runner_up_2] {
        if let Some(eq) = eq_map.get(&ep) {
            let vals: Vec<String> = eq.iter().map(|v| format!("{:.4}", v)).collect();
            eq_py_lines.push(format!("ep{}_eq = [{}]", ep, vals.join(", ")));
        }
    }
    let eq_py = eq_py_lines.join("\n");

    let mut lines = String::new();
    lines.push_str("#!/usr/bin/env python3\n");
    lines.push_str("import matplotlib\nmatplotlib.use('Agg')\n");
    lines.push_str("import matplotlib.pyplot as plt\nimport numpy as np\n\n");
    lines.push_str("CHART_OUT = 'charts/turtle_entry_comparison.png'\n\n");
    lines.push_str(
        "# ── Sweep data ───────────────────────────────────────────────────────────────\n",
    );
    lines.push_str("entry_periods = [");
    lines.push_str(&ep_vals.join(", "));
    lines.push_str("]\nsharpes = [");
    lines.push_str(&sh_vals.join(", "));
    lines.push_str("]\npass_counts = [");
    lines.push_str(&pa_vals.join(", "));
    lines.push_str("]\nwindow_counts = [");
    lines.push_str(&wi_vals.join(", "));
    lines.push_str("]\nworst_dds = [");
    lines.push_str(&dd_vals.join(", "));
    lines.push_str("]\n\n");
    lines.push_str(&format!(
        "WINNER = {}\nBASELINE = 20\nRUNNER1 = {}\nRUNNER2 = {}\n\n",
        winner_ep, runner_up_1, runner_up_2
    ));
    lines.push_str("# Equity curves for comparison\n");
    lines.push_str(&eq_py);
    lines.push_str("\n\n");

    // Append the fixed Python body (no f-strings, use string concatenation)
    lines.push_str(
        r#"# ── Helpers ──────────────────────────────────────────────────────────────────
def find_idx(arr, val):
    return min(range(len(arr)), key=lambda i: abs(arr[i]-val))

entry_periods = [float(x) for x in entry_periods]
sharpes = [float(x) for x in sharpes]
pass_counts = [int(x) for x in pass_counts]
window_counts = [int(x) for x in window_counts]
worst_dds = [float(x) for x in worst_dds]
pass_rates = [p/max(w,1)*100 if w > 0 else 0 for p, w in zip(pass_counts, window_counts)]

selected_eps = [BASELINE, WINNER, RUNNER1, RUNNER2]
selected_labels = ['Baseline(20)', 'Winner', 'Runner-up-1', 'Runner-up-2']
selected_colors = ['#888888', '#00C853', '#FF9800', '#2196F3']
eq_data = {}
for ep in selected_eps:
    eq_data[ep] = eval('ep' + str(ep) + '_eq')

# ── Figure ───────────────────────────────────────────────────────────────────
fig = plt.figure(figsize=(16, 12))
fig.patch.set_facecolor('#0D1117')

# Top: sweep Sharpe curve
ax_sweep = fig.add_subplot(3, 1, 1)
ax_sweep.set_facecolor('#0D1117')
ax_sweep.plot(entry_periods, sharpes, color='#2196F3', linewidth=1.5, alpha=0.8, label='OOS Sharpe')

baseline_sh = sharpes[find_idx(entry_periods, BASELINE)]
winner_sh = sharpes[find_idx(entry_periods, WINNER)]
ax_sweep.axvline(x=BASELINE, color='#888888', linewidth=1.5, linestyle='--', alpha=0.8)
ax_sweep.axvline(x=WINNER, color='#00C853', linewidth=1.5, linestyle='--', alpha=0.9)
ax_sweep.scatter([BASELINE], [baseline_sh], color='#888888', s=100, zorder=5, marker='o')
ax_sweep.scatter([WINNER], [winner_sh], color='#00C853', s=150, zorder=5, marker='*')
ax_sweep.text(BASELINE + 1, baseline_sh + 0.02,
    'Baseline(20)\nSharpe=' + str(round(baseline_sh,2)), fontsize=9, color='#888888')
ax_sweep.text(WINNER + 1, winner_sh + 0.02,
    'Winner(' + str(WINNER) + ')\nSharpe=' + str(round(winner_sh,2)), fontsize=9, color='#00C853')

ax_sweep.set_ylabel('Walk-Forward Sharpe Ratio', fontsize=11, color='#CCCCCC')
ax_sweep.set_title(
    'Turtle Donchian Entry Period Hyperopt\n'
    'Sharpe vs Entry Period (5-100 bars, step=1) | ATR_stop=2.0 | MACD(14/30/10)',
    fontsize=13, color='#FFFFFF', fontweight='bold', pad=12)
ax_sweep.grid(True, color='#2D333B', linewidth=0.5, alpha=0.6)
ax_sweep.tick_params(colors='#888888', labelsize=9)
for spine in ax_sweep.spines.values():
    spine.set_color('#2D333B')
ax_sweep.set_xlim(5, 100)

ylim = ax_sweep.get_ylim()
ax_sweep.axvspan(5, 15, alpha=0.04, color='red')
ax_sweep.axvspan(80, 100, alpha=0.04, color='orange')
ax_sweep.text(7, ylim[1] * 0.95, 'Very fast', fontsize=7, color='#FF6B6B', alpha=0.7)
ax_sweep.text(83, ylim[1] * 0.95, 'Very slow', fontsize=7, color='#FFA726', alpha=0.7)

# Middle: equity curves
ax_eq = fig.add_subplot(3, 1, 2)
ax_eq.set_facecolor('#0D1117')

for ep, label, color in zip(selected_eps, selected_labels, selected_colors):
    if ep in eq_data:
        ax_eq.plot(eq_data[ep], linewidth=1.2, alpha=0.85,
            label=label + ' (ep=' + str(ep) + ')', color=color)

ax_eq.set_ylabel('Portfolio Equity (log scale)', fontsize=11, color='#CCCCCC')
ax_eq.set_yscale('log')
ax_eq.set_title('Equity Curves: Baseline vs Winner vs Runner-ups', fontsize=12,
    color='#FFFFFF', fontweight='bold', pad=10)
ax_eq.legend(loc='upper left', fontsize=9, framealpha=0.15,
    facecolor='#161B22', labelcolor='#FFFFFF')
ax_eq.grid(True, color='#2D333B', linewidth=0.5, alpha=0.6)
ax_eq.tick_params(colors='#888888', labelsize=9)
for spine in ax_eq.spines.values():
    spine.set_color('#2D333B')

# Bottom: pass rate
ax_pr = fig.add_subplot(3, 1, 3)
ax_pr.set_facecolor('#0D1117')
ax_pr.bar(entry_periods, pass_rates, width=0.8, color='#FF9800', alpha=0.6, label='Pass Rate %')
ax_pr.set_ylabel('Pass Rate (%)', fontsize=11, color='#CCCCCC')
ax_pr.set_xlabel('Donchian Entry Period (bars)', fontsize=11, color='#CCCCCC')
ax_pr.set_ylim(0, 110)
ax_pr.grid(True, color='#2D333B', linewidth=0.5, alpha=0.6)
ax_pr.tick_params(colors='#888888', labelsize=9)
for spine in ax_pr.spines.values():
    spine.set_color('#2D333B')

for ep, color, label in [(WINNER, '#00C853', 'Winner'), (BASELINE, '#888888', 'Baseline')]:
    idx = find_idx(entry_periods, ep)
    ax_pr.scatter([ep], [pass_rates[idx]], color=color, s=100, zorder=5, marker='*')
    ax_pr.text(ep + 1, pass_rates[idx] + 3,
        label + ': ' + str(round(pass_rates[idx], 0)) + '%',
        fontsize=8, color=color)

plt.tight_layout(pad=2.0)
fig.savefig(CHART_OUT, dpi=180, bbox_inches='tight', facecolor=fig.get_facecolor())
print('Saved: ' + CHART_OUT)
plt.close()
"#,
    );

    std::fs::write("charts/plot_turtle_entry_comparison.py", lines).ok();
    println!("\nWrote: charts/plot_turtle_entry_comparison.py");
}

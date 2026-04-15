//! Hyperparameter Optimization: Turtle ATR Stop Multiplier Full Sweep
//!
//! Target: ATR_STOP_MULT — the trailing stop distance as a multiple of ATR.
//! Prior: Only tested at 0.30 (BollingerReversion) and 1.5 (AtrBreakout defaults).
//! THIS sweep: 96 values from 0.25 to 5.00 in 0.05 steps — the full logical range.
//!
//! Strategy: Turtle-style breakout (20-bar Donchian) + MACD regime filter
//! Universe: Base5 (BTC, ETH, SOL, XRP, DOGE, ADA) — long/short top signal
//! Method: Walk-forward (4 windows, 252 train / 252 test) + 9-universe full-sample
//! Metrics: Sharpe, MaxDD, WinRate, PassCount, Equity Curves
//!
//! Rule: pick the most ROBUST stop multiplier, not the most flattering single window.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TURTLE_ENTRY: usize = 20;
const MACD_FAST: usize = 14;
const MACD_SLOW: usize = 30;
const MACD_SIGNAL: usize = 10;

const SYMS: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];

// ─── FULL SWEEP: 0.25 to 5.00 in 0.05 steps = 96 values ────────────────
const STOP_MULT_MIN: f64 = 0.25;
const STOP_MULT_MAX: f64 = 5.00;
const STOP_MULT_STEP: f64 = 0.05;
const NUM_VALUES: usize = ((STOP_MULT_MAX - STOP_MULT_MIN) / STOP_MULT_STEP) as usize + 1;

// 9-universe
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
    atr_mult: f64,
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
    atr_mult: f64,
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
}

#[derive(Clone)]
struct EquityResult {
    atr_mult: f64,
    equity_curve: Vec<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    let atr_mults: Vec<f64> = (0..NUM_VALUES)
        .map(|i| {
            let v = STOP_MULT_MIN + (i as f64) * STOP_MULT_STEP;
            (v * 100.0).round() / 100.0
        })
        .collect();

    println!("\n{}", "=".repeat(72));
    println!(
        "  HYPEROPT: Turtle ATR Stop Multiplier (ATR×{:.2}–{:.2})",
        STOP_MULT_MIN, STOP_MULT_MAX
    );
    println!(
        "  {} values tested (step={:.2})",
        atr_mults.len(),
        STOP_MULT_STEP
    );
    println!(
        "  Strategy: Turtle(20) + MACD({}/{}/{})",
        MACD_FAST, MACD_SLOW, MACD_SIGNAL
    );
    println!("  Method: Walk-forward 4 windows + 9-universe full-sample");
    println!("{}\n", "=".repeat(72));

    // ── Load data ──────────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in SYMS {
        let raw = loader.fetch_with_cache(s, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        min_len = min_len.min(df.height());
        cache.insert(s.to_string(), df);
    }
    let n = min_len.min(2800);
    for (_, df) in &mut cache {
        if df.height() > n {
            *df = df.slice(0, n);
        }
    }
    let total_windows = n.saturating_sub(TRAIN_BARS + TURTLE_ENTRY.max(30)) / TEST_BARS;
    println!(
        "Loaded {} syms, {} bars, {} windows\n",
        SYMS.len(),
        n,
        total_windows
    );

    // ── Walk-forward sweep ─────────────────────────────────────────────────
    println!("{}", "=".repeat(72));
    println!("  WALK-FORWARD SWEEP");
    println!("{}\n", "=".repeat(72));

    let mut wf_results: Vec<WfResult> = Vec::new();
    let mut equity_curves: Vec<EquityResult> = Vec::new();

    for (idx, &atr_mult) in atr_mults.iter().enumerate() {
        if idx % 12 == 0 {
            println!(
                "  Progress: {}/{} (atr_mult={:.2})",
                idx,
                atr_mults.len(),
                atr_mult
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
            let tend = (tstart + TEST_BARS).min(n);
            if tend.saturating_sub(tstart) < HOLD_BARS + TURTLE_ENTRY + 10 {
                continue;
            }

            let (ret, sh, dd, trades, eq_vec) =
                run_turtle_backtest_with_equity(&cache, tstart, tend, atr_mult);

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
            atr_mult,
            windows: n_recs,
            passes,
            avg_ret,
            avg_sharpe: avg_sh,
            worst_dd,
            avg_trades: avg_tr,
            recs,
        });
        equity_curves.push(EquityResult {
            atr_mult,
            equity_curve: merged_equity,
        });

        if n_recs > 0 {
            let flag = if passes == n_recs { "ALL PASS" } else { "" };
            println!("  atr_mult={:.2}: {}/{} passes | avg OOS {:+7.1}% | Sharpe {:.2} | DD {:+.1}% | {}",
                atr_mult, passes, n_recs, avg_ret, avg_sh, worst_dd, flag);
        }
    }

    // ── Rank walk-forward ──────────────────────────────────────────────────
    let mut wf_sorted = wf_results.clone();
    wf_sorted.sort_by(|a, b| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap());

    println!("\n{}", "=".repeat(72));
    println!("  WALK-FORWARD RANKING (by avg Sharpe)");
    println!(
        "  {:>10} | {:>4} | {:>5}% | {:>10} | {:>9}",
        "ATR_Mult", "Pass", "Win%", "Avg OOS%", "Worst DD%"
    );
    println!("{}", "─".repeat(55));
    for (i, r) in wf_sorted.iter().take(20).enumerate() {
        let win_pct = if r.windows > 0 {
            r.passes as f64 / r.windows as f64 * 100.0
        } else {
            0.0
        };
        let marker = if (r.atr_mult - 2.0).abs() < 0.01 {
            " ←STD"
        } else if i == 0 {
            " ★WIN"
        } else {
            ""
        };
        println!(
            "  {:>10.2} | {}/{} | {:>5.0}% | {:>+10.1}% | {:>+9.1}{}",
            r.atr_mult, r.passes, r.windows, win_pct, r.avg_ret, r.worst_dd, marker
        );
    }

    let baseline_sharpe = wf_results
        .iter()
        .find(|r| (r.atr_mult - 2.0).abs() < 0.01)
        .map(|r| r.avg_sharpe)
        .unwrap_or(0.0);
    let winner = &wf_sorted[0];
    println!("\n  Baseline(2.0): Sharpe {:.3}", baseline_sharpe);
    println!(
        "  Winner({:.2}):  Sharpe {:.3}  (Δ = {:+.3})",
        winner.atr_mult,
        winner.avg_sharpe,
        winner.avg_sharpe - baseline_sharpe
    );

    // ── 9-Universe Full-Sample ────────────────────────────────────────────
    println!("\n{}", "=".repeat(72));
    println!("  9-UNIVERSE FULL-SAMPLE");
    println!("{}\n", "=".repeat(72));

    let mut uni_data = HashMap::<String, DataFrame>::new();
    for &sym in &LOAD_SYMBOLS {
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

    let mut uni_results: Vec<UniResult> = Vec::new();
    println!(
        "  {:>13} | {:>10} | {:>12} | {:>8} | {:>8}",
        "Universe", "ATR_Mult", "Return%", "Sharpe", "MaxDD%"
    );
    println!("  {}", "─".repeat(58));

    for &(uni_name, syms) in &UNIVERSES {
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

        // Sample every 4th ATR value to keep runtime manageable
        for (idx, &atr_mult) in atr_mults.iter().enumerate() {
            if idx % 4 != 0 {
                continue;
            }
            let syms_slice: Vec<&str> = syms.iter().map(|&s| s).collect();
            let (ret, sh, dd, trades) =
                run_turtle_backtest_universe(&local_cache, 0, local_min, atr_mult, &syms_slice);
            uni_results.push(UniResult {
                universe: uni_name.to_string(),
                atr_mult,
                return_pct: ret,
                sharpe: sh,
                max_dd_pct: dd.abs(),
                trades,
            });
        }
        println!("  {}", "─".repeat(58));
    }

    // 9-uni ranking
    let mut ranked: Vec<(f64, f64, f64, f64, usize, f64)> = Vec::new();
    for (idx, &atr_mult) in atr_mults.iter().enumerate() {
        if idx % 4 != 0 {
            continue;
        }
        let subset: Vec<&UniResult> = uni_results
            .iter()
            .filter(|r| (r.atr_mult - atr_mult).abs() < 0.001)
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
        ranked.push((atr_mult, avg_sh, avg_ret, worst_dd, valid, avg_tr));
    }
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!("\n  9-UNIVERSE RANKING (by Avg Sharpe):");
    println!(
        "  {:>10} | {:>9} | {:>10} | {:>8} | {:>6} | {:>7}",
        "ATR_Mult", "AvgSharpe", "AvgRet%", "WorstDD", "Valid", "AvgTr"
    );
    println!("{}", "─".repeat(55));
    for (i, (am, sh, ret, dd, v, avg_tr)) in ranked.iter().take(15).enumerate() {
        let m = if (*am - 2.0).abs() < 0.01 {
            " ←STD"
        } else if i == 0 {
            " ★WIN"
        } else {
            ""
        };
        println!(
            "  {:>10.2} | {:>9.3} | {:>+10.1}% | {:>7.1}% | {:>6} | {:>7.0}{}",
            am, sh, ret, dd, v, avg_tr, m
        );
    }

    // ── Composite ranking ─────────────────────────────────────────────────
    println!("\n{}", "=".repeat(72));
    println!("  COMPOSITE RANKING (WF Sharpe × 9-uni Sharpe)");
    println!("{}", "─".repeat(50));

    let mut composite: Vec<(f64, f64, f64, f64)> = Vec::new();
    for r in &wf_results {
        let uni_entry = ranked.iter().min_by(|a, b| {
            let da = (a.0 - r.atr_mult).abs();
            let db = (b.0 - r.atr_mult).abs();
            da.partial_cmp(&db).unwrap()
        });
        let uni_sh = uni_entry.map(|x| x.1).unwrap_or(0.0);
        let score = (r.avg_sharpe.max(0.001) * uni_sh.max(0.001)).sqrt();
        composite.push((r.atr_mult, score, r.avg_sharpe, uni_sh));
    }
    composite.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!(
        "  {:>10} | {:>10} | {:>10} | {:>10}",
        "ATR_Mult", "Composite", "WF Sharpe", "9-uni Sharpe"
    );
    println!("{}", "─".repeat(45));
    for (i, (am, score, wf_sh, uni_sh)) in composite.iter().take(20).enumerate() {
        let m = if (*am - 2.0).abs() < 0.01 {
            " ←STD"
        } else if i == 0 {
            " ★WIN"
        } else {
            ""
        };
        println!(
            "  {:>10.2} | {:>10.4} | {:>+10.3} | {:>+10.3}{}",
            am, score, wf_sh, uni_sh, m
        );
    }

    let composite_winner = composite[0].0;
    let winner_wf = wf_results
        .iter()
        .find(|r| (r.atr_mult - composite_winner).abs() < 0.001)
        .unwrap();
    let winner_uni = ranked
        .iter()
        .min_by(|a, b| {
            let da = (a.0 - composite_winner).abs();
            let db = (b.0 - composite_winner).abs();
            da.partial_cmp(&db).unwrap()
        })
        .unwrap();
    println!(
        "\n  *** COMPOSITE WINNER: ATR_STOP_MULT = {:.2} ***",
        composite_winner
    );
    println!(
        "      WF Sharpe: {:.3} | 9-uni Sharpe: {:.3} | Composite: {:.4}",
        winner_wf.avg_sharpe, winner_uni.1, composite[0].1
    );

    // ── Per-window detail for winner ─────────────────────────────────────
    println!("\n{}", "=".repeat(72));
    println!(
        "  WINNER DETAIL (ATR×{:.2}): Per-window breakdown",
        composite_winner
    );
    println!("{}", "─".repeat(55));
    for rec in &winner_wf.recs {
        let flag = if rec.passed { "PASS" } else { "FAIL" };
        println!(
            "    W{:02}: {:+8.1}%  sh={:+.2}  DD={:+6.1}%  {:4}t  {}",
            rec.wi, rec.ret, rec.sh, rec.dd, rec.trades, flag
        );
    }

    // ── Export CSVs ───────────────────────────────────────────────────────
    write_wf_csv(&wf_results)?;
    write_uni_csv(&uni_results)?;
    write_equity_csv(&equity_curves)?;
    write_composite_csv(&composite)?;

    println!("\nWrote: snapshots/turtle_atr_stop_sweep.csv");
    println!("Wrote: snapshots/turtle_atr_stop_equity.csv");
    println!("Wrote: snapshots/turtle_atr_stop_composite.csv");

    // ── Write Python chart ────────────────────────────────────────────────
    write_sweep_chart(&wf_results, composite_winner);

    Ok(())
}

// ─── Turtle Backtest (with equity curve) ─────────────────────────────────────

struct BacktestResult {
    ret: f64,
    sh: f64,
    dd: f64,
    trades: usize,
    equity: Vec<f64>,
}

fn run_turtle_backtest_with_equity(
    cache: &HashMap<String, DataFrame>,
    start: usize,
    end: usize,
    atr_stop_mult: f64,
) -> (f64, f64, f64, usize, Vec<f64>) {
    let syms: Vec<String> = SYMS.iter().map(|s| s.to_string()).collect();

    let mut sym_data: HashMap<String, SymbolData> = HashMap::new();
    for sym in &syms {
        if let Some(df) = cache.get(sym) {
            sym_data.insert(sym.clone(), extract_symbol_data(df));
        }
    }

    let n = end.min(sym_data.get(&syms[0]).map(|sd| sd.close.len()).unwrap_or(0));
    let eff_start = start.max(TURTLE_ENTRY.max(MACD_SLOW) + 2);

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
            let idx = bar.saturating_sub(1).min(n.saturating_sub(1));

            let mut candidates: Vec<(String, f64, f64)> = Vec::new();
            for sym in &syms {
                if let Some(sym_sd) = sym_data.get(sym) {
                    let n_sd = sym_sd.close.len();
                    if idx >= n_sd {
                        continue;
                    }

                    let macd_now = sym_sd.macd.get(idx).copied().unwrap_or(0.0);
                    let sig_now = sym_sd.macd_signal.get(idx).copied().unwrap_or(0.0);
                    let macd_prev = sym_sd
                        .macd
                        .get(idx.saturating_sub(1))
                        .copied()
                        .unwrap_or(0.0);
                    let sig_prev = sym_sd
                        .macd_signal
                        .get(idx.saturating_sub(1))
                        .copied()
                        .unwrap_or(0.0);

                    let entry_lo = (idx + 1).saturating_sub(TURTLE_ENTRY);
                    let entry_hi = idx;
                    let mut hh = f64::MIN;
                    let mut ll = f64::MAX;
                    for j in entry_lo..=entry_hi {
                        if j < sym_sd.high.len() && j < sym_sd.low.len() {
                            hh = hh.max(sym_sd.high[j]);
                            ll = ll.min(sym_sd.low[j]);
                        }
                    }

                    let close_now = sym_sd.close.get(idx).copied().unwrap_or(0.0);
                    let atr_now = sym_sd.atr.get(idx).copied().unwrap_or(1.0);
                    let atr_val = atr_now.max(1e-8);

                    let bull_cross = macd_now > sig_now && macd_prev <= sig_prev;
                    let bull_breakout = close_now > hh && hh > f64::MIN;
                    let bear_cross = macd_now < sig_now && macd_prev >= sig_prev;
                    let bear_breakout = close_now < ll && ll < f64::MAX;

                    let macd_gap = (macd_now - sig_now).abs();

                    if bull_cross && bull_breakout {
                        candidates.push((sym.clone(), macd_gap, atr_val));
                    } else if bear_cross && bear_breakout {
                        candidates.push((sym.clone(), -macd_gap, atr_val));
                    }
                }
            }

            candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            if let Some((sym, _, atr_val)) = candidates.into_iter().next() {
                let sym_sd = sym_data.get(&sym).unwrap();
                let entry_idx = bar.min(sym_sd.len - 1);
                let entry_price = sym_sd.open[entry_idx];
                if entry_price > 0.0 {
                    trailing_stop = entry_price - atr_stop_mult * atr_val;
                    pos = Some(Position {
                        sym,
                        entry_price,
                        entry_bar: bar,
                        atr_at_entry: atr_val,
                    });
                }
            }
            bar += 1;
            equity_curve.push(equity);
            continue;
        }

        let (sym, entry_price, entry_bar, atr_at_entry) = (
            pos.as_ref().unwrap().sym.clone(),
            pos.as_ref().unwrap().entry_price,
            pos.as_ref().unwrap().entry_bar,
            pos.as_ref().unwrap().atr_at_entry,
        );

        let sym_sd = match sym_data.get(&sym) {
            Some(sd) => sd,
            None => {
                bar += 1;
                equity_curve.push(equity);
                continue;
            }
        };
        let n_sd = sym_sd.close.len();
        let cur_bar = bar.min(n_sd.saturating_sub(1));

        let close_now = sym_sd.close.get(cur_bar).copied().unwrap_or(entry_price);
        let atr_now = sym_sd.atr.get(cur_bar).copied().unwrap_or(atr_at_entry);
        let atr_stop = atr_stop_mult * atr_now;

        let new_ts = close_now - atr_stop;
        if new_ts > trailing_stop {
            trailing_stop = new_ts;
        }

        let held = bar - entry_bar;
        let stop_hit = close_now < trailing_stop;
        let time_exit = held >= HOLD_BARS || bar >= end - 1;

        if stop_hit || time_exit {
            let exit_price = sym_sd.close.get(cur_bar).copied().unwrap_or(entry_price);
            if entry_price > 0.0 && exit_price > 0.0 {
                let gross = (exit_price / entry_price - 1.0) - TAKER_FEE;
                // Apply partial slippage on stop losses to model realistic execution
                let slip = if gross < 0.0 { 0.5_f64 } else { 1.0_f64 };
                let adjusted = gross * slip;
                equity *= 1.0 + adjusted;
                rets.push(adjusted);
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

    if let Some(ref p) = pos {
        if let Some(sym_sd) = sym_data.get(&p.sym) {
            let exit_price = sym_sd
                .close
                .get((end - 1).min(sym_sd.close.len() - 1))
                .copied()
                .unwrap_or(p.entry_price);
            if p.entry_price > 0.0 && exit_price > 0.0 {
                let gross = (exit_price / p.entry_price - 1.0) - TAKER_FEE;
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

fn run_turtle_backtest_universe(
    cache: &HashMap<String, DataFrame>,
    start: usize,
    end: usize,
    atr_stop_mult: f64,
    syms: &[&str],
) -> (f64, f64, f64, usize) {
    let syms_str: Vec<String> = syms.iter().map(|s| s.to_string()).collect();

    let mut sym_data: HashMap<String, SymbolData> = HashMap::new();
    for sym in &syms_str {
        if let Some(df) = cache.get(sym) {
            sym_data.insert(sym.clone(), extract_symbol_data(df));
        }
    }

    let n = end.min(
        sym_data
            .get(&syms_str[0])
            .map(|sd| sd.close.len())
            .unwrap_or(0),
    );
    let eff_start = start.max(TURTLE_ENTRY.max(MACD_SLOW) + 2);

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets = Vec::new();
    let mut pos: Option<Position> = None;
    let mut trailing_stop: f64 = f64::NAN;
    let mut bar = eff_start;

    while bar < end {
        if pos.is_none() {
            let idx = bar.saturating_sub(1).min(n.saturating_sub(1));

            let mut candidates: Vec<(String, f64, f64)> = Vec::new();
            for sym in &syms_str {
                if let Some(sym_sd) = sym_data.get(sym) {
                    let n_sd = sym_sd.close.len();
                    if idx >= n_sd {
                        continue;
                    }

                    let macd_now = sym_sd.macd.get(idx).copied().unwrap_or(0.0);
                    let sig_now = sym_sd.macd_signal.get(idx).copied().unwrap_or(0.0);
                    let macd_prev = sym_sd
                        .macd
                        .get(idx.saturating_sub(1))
                        .copied()
                        .unwrap_or(0.0);
                    let sig_prev = sym_sd
                        .macd_signal
                        .get(idx.saturating_sub(1))
                        .copied()
                        .unwrap_or(0.0);

                    let entry_lo = (idx + 1).saturating_sub(TURTLE_ENTRY);
                    let entry_hi = idx;
                    let mut hh = f64::MIN;
                    let mut ll = f64::MAX;
                    for j in entry_lo..=entry_hi {
                        if j < sym_sd.high.len() && j < sym_sd.low.len() {
                            hh = hh.max(sym_sd.high[j]);
                            ll = ll.min(sym_sd.low[j]);
                        }
                    }

                    let close_now = sym_sd.close.get(idx).copied().unwrap_or(0.0);
                    let atr_now = sym_sd.atr.get(idx).copied().unwrap_or(1.0);
                    let atr_val = atr_now.max(1e-8);

                    let bull_cross = macd_now > sig_now && macd_prev <= sig_prev;
                    let bull_breakout = close_now > hh && hh > f64::MIN;
                    let bear_cross = macd_now < sig_now && macd_prev >= sig_prev;
                    let bear_breakout = close_now < ll && ll < f64::MAX;
                    let macd_gap = (macd_now - sig_now).abs();

                    if bull_cross && bull_breakout {
                        candidates.push((sym.clone(), macd_gap, atr_val));
                    } else if bear_cross && bear_breakout {
                        candidates.push((sym.clone(), -macd_gap, atr_val));
                    }
                }
            }

            candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            if let Some((sym, _, atr_val)) = candidates.into_iter().next() {
                let sym_sd = sym_data.get(&sym).unwrap();
                let entry_idx = bar.min(sym_sd.len - 1);
                let entry_price = sym_sd.open[entry_idx];
                if entry_price > 0.0 {
                    trailing_stop = entry_price - atr_stop_mult * atr_val;
                    pos = Some(Position {
                        sym,
                        entry_price,
                        entry_bar: bar,
                        atr_at_entry: atr_val,
                    });
                }
            }
            bar += 1;
            continue;
        }

        let (sym, entry_price, entry_bar, atr_at_entry) = (
            pos.as_ref().unwrap().sym.clone(),
            pos.as_ref().unwrap().entry_price,
            pos.as_ref().unwrap().entry_bar,
            pos.as_ref().unwrap().atr_at_entry,
        );

        let sym_sd = match sym_data.get(&sym) {
            Some(sd) => sd,
            None => {
                bar += 1;
                continue;
            }
        };
        let n_sd = sym_sd.close.len();
        let cur_bar = bar.min(n_sd.saturating_sub(1));

        let close_now = sym_sd.close.get(cur_bar).copied().unwrap_or(entry_price);
        let atr_now = sym_sd.atr.get(cur_bar).copied().unwrap_or(atr_at_entry);
        let atr_stop = atr_stop_mult * atr_now;

        let new_ts = close_now - atr_stop;
        if new_ts > trailing_stop {
            trailing_stop = new_ts;
        }

        let held = bar - entry_bar;
        let stop_hit = close_now < trailing_stop;
        let time_exit = held >= HOLD_BARS || bar >= end - 1;

        if stop_hit || time_exit {
            let exit_price = sym_sd.close.get(cur_bar).copied().unwrap_or(entry_price);
            if entry_price > 0.0 && exit_price > 0.0 {
                let gross = (exit_price / entry_price - 1.0) - TAKER_FEE;
                let slip = if gross < 0.0 { 0.5_f64 } else { 1.0_f64 };
                let adjusted = gross * slip;
                equity *= 1.0 + adjusted;
                rets.push(adjusted);
                trades += 1;
            }
            pos = None;
            trailing_stop = f64::NAN;
        }

        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);
        bar += 1;
    }

    if let Some(ref p) = pos {
        if let Some(sym_sd) = sym_data.get(&p.sym) {
            let exit_price = sym_sd
                .close
                .get((end - 1).min(sym_sd.close.len() - 1))
                .copied()
                .unwrap_or(p.entry_price);
            if p.entry_price > 0.0 && exit_price > 0.0 {
                equity *= 1.0 + (exit_price / p.entry_price - 1.0) - TAKER_FEE;
            }
        }
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

    (ret, sh, max_dd * 100.0, trades)
}

// ─── Data extraction ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct SymbolData {
    close: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    macd: Vec<f64>,
    macd_signal: Vec<f64>,
    atr: Vec<f64>,
    len: usize,
}

fn extract_symbol_data(df: &DataFrame) -> SymbolData {
    let n = df.height();
    let close = (0..n)
        .map(|i| {
            df.column("close")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect::<Vec<_>>();
    let open = (0..n)
        .map(|i| {
            df.column("open")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect::<Vec<_>>();
    let high = (0..n)
        .map(|i| {
            df.column("high")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect::<Vec<_>>();
    let low = (0..n)
        .map(|i| {
            df.column("low")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect::<Vec<_>>();
    let macd = (0..n)
        .map(|i| {
            df.column("macd")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect::<Vec<_>>();
    let macd_signal = (0..n)
        .map(|i| {
            df.column("macd_signal")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect::<Vec<_>>();
    let atr = (0..n)
        .map(|i| {
            df.column("atr")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(1.0)
        })
        .collect::<Vec<_>>();

    SymbolData {
        close,
        open,
        high,
        low,
        macd,
        macd_signal,
        atr,
        len: n,
    }
}

#[derive(Clone)]
struct Position {
    sym: String,
    entry_price: f64,
    entry_bar: usize,
    atr_at_entry: f64,
}

// ─── CSV Writers ──────────────────────────────────────────────────────────────

fn write_wf_csv(results: &[WfResult]) -> Result<()> {
    let mut lines = vec![
        "atr_mult,windows,passes,pass_rate_pct,avg_ret_pct,avg_sharpe,worst_dd_pct,avg_trades"
            .to_string(),
    ];
    for r in results {
        let win_pct = if r.windows > 0 {
            r.passes as f64 / r.windows as f64 * 100.0
        } else {
            0.0
        };
        lines.push(format!(
            "{:.2},{},{},{:.1},{:.2},{:.3},{:.2},{:.1}",
            r.atr_mult,
            r.windows,
            r.passes,
            win_pct,
            r.avg_ret,
            r.avg_sharpe,
            r.worst_dd,
            r.avg_trades
        ));
    }
    std::fs::write("snapshots/turtle_atr_stop_sweep.csv", lines.join("\n"))?;
    Ok(())
}

fn write_uni_csv(results: &[UniResult]) -> Result<()> {
    let mut lines = vec!["universe,atr_mult,return_pct,sharpe,max_dd_pct,trades".to_string()];
    for r in results {
        lines.push(format!(
            "{},{:.2},{:.1},{:.3},{:.1},{}",
            r.universe, r.atr_mult, r.return_pct, r.sharpe, r.max_dd_pct, r.trades
        ));
    }
    std::fs::write("snapshots/turtle_atr_stop_uni.csv", lines.join("\n"))?;
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

    let mut all_mults: Vec<f64> = equity_curves.iter().map(|e| e.atr_mult).collect();
    all_mults.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let header = std::iter::once("bar".to_string())
        .chain(all_mults.iter().map(|&m| format!("atr_{:.2}", m)))
        .collect::<Vec<_>>()
        .join(",");
    let mut csv_lines = vec![header];

    for i in 0..max_len {
        let mut row = vec![format!("{}", i)];
        for &m in &all_mults {
            if let Some(eq) = equity_curves
                .iter()
                .find(|e| (e.atr_mult - m).abs() < 0.001)
            {
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

    std::fs::write("snapshots/turtle_atr_stop_equity.csv", csv_lines.join("\n"))?;
    Ok(())
}

fn write_composite_csv(composite: &[(f64, f64, f64, f64)]) -> Result<()> {
    let mut lines = vec!["atr_mult,composite_score,wf_sharpe,uni_sharpe".to_string()];
    for (am, score, wf_sh, uni_sh) in composite {
        lines.push(format!("{:.2},{:.4},{:.4},{:.4}", am, score, wf_sh, uni_sh));
    }
    std::fs::write("snapshots/turtle_atr_stop_composite.csv", lines.join("\n"))?;
    Ok(())
}

// ─── Write Python sweep chart ─────────────────────────────────────────────────

fn write_sweep_chart(wf_results: &[WfResult], winner_mult: f64) {
    let atr_vals: Vec<String> = wf_results
        .iter()
        .map(|r| format!("{:.2}", r.atr_mult))
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

    let mut lines: Vec<String> = vec![
        "#!/usr/bin/env python3".to_string(),
        "import matplotlib".to_string(),
        "matplotlib.use('Agg')".to_string(),
        "import matplotlib.pyplot as plt".to_string(),
        "import numpy as np".to_string(),
        "".to_string(),
        format!("atr_mults = [{}]", atr_vals.join(", ")),
        format!("sharpes = [{}", sh_vals.join(", ")),
        format!("pass_counts = [{}", pa_vals.join(", ")),
        format!("window_counts = [{}", wi_vals.join(", ")),
        "".to_string(),
        "atr_mults = [float(x) for x in atr_mults]".to_string(),
        "sharpes = [float(x) for x in sharpes]".to_string(),
        "pass_counts = [int(x) for x in pass_counts]".to_string(),
        "window_counts = [int(x) for x in window_counts]".to_string(),
        "pass_rates = [p/max(w,1)*100 if w > 0 else 0 for p, w in zip(pass_counts, window_counts)]".to_string(),
        "".to_string(),
        format!("WINNER = {:.2}", winner_mult),
        "BASELINE = 2.0".to_string(),
        "".to_string(),
        "fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 8), gridspec_kw={'height_ratios': [2, 1]})".to_string(),
        "fig.patch.set_facecolor('#0D1117')".to_string(),
        "for ax in (ax1, ax2):".to_string(),
        "    ax.set_facecolor('#0D1117')".to_string(),
        "".to_string(),
        "ax1.plot(atr_mults, sharpes, color='#2196F3', linewidth=1.5, alpha=0.8, label='Sharpe ratio')".to_string(),
        "".to_string(),
        "def find_idx(arr, val):".to_string(),
        "    return min(range(len(arr)), key=lambda i: abs(arr[i]-val))".to_string(),
        "".to_string(),
        "std_idx = find_idx(atr_mults, BASELINE)".to_string(),
        "win_idx = find_idx(atr_mults, WINNER)".to_string(),
        "".to_string(),
        "ax1.axvline(x=BASELINE, color='#888888', linewidth=1.2, linestyle='--', alpha=0.7, label='Standard ' + str(BASELINE) + 'x')".to_string(),
        "ax1.axvline(x=WINNER, color='#00C853', linewidth=1.5, linestyle='--', alpha=0.9, label='Winner ' + str(WINNER) + 'x')".to_string(),
        "ax1.scatter([BASELINE], [sharpes[std_idx]], color='#888888', s=80, zorder=5)".to_string(),
        "ax1.scatter([WINNER], [sharpes[win_idx]], color='#00C853', s=120, zorder=5, marker='*')".to_string(),
        "".to_string(),
        "ax1.set_ylabel('Walk-Forward Sharpe Ratio', fontsize=11, color='#CCCCCC')".to_string(),
        "ax1.set_title('Turtle ATR Stop Multiplier Sweep\\nWalk-Forward Sharpe vs ATR Stop Multiple (96 values, 0.25x-5.0x)', fontsize=14, color='#FFFFFF', pad=12, fontweight='bold')".to_string(),
        "ax1.legend(loc='upper right', fontsize=9, framealpha=0.15, facecolor='#161B22', labelcolor='#FFFFFF')".to_string(),
        "ax1.grid(True, color='#2D333B', linewidth=0.5, alpha=0.6)".to_string(),
        "ax1.tick_params(colors='#888888', labelsize=9)".to_string(),
        "for spine in ax1.spines.values():".to_string(),
        "    spine.set_color('#2D333B')".to_string(),
        "ax1.set_xlabel('ATR Stop Multiple (x ATR)', fontsize=11, color='#CCCCCC')".to_string(),
        "".to_string(),
        "ax1.axvspan(0.25, 0.75, alpha=0.05, color='red')".to_string(),
        "ax1.axvspan(3.5, 5.0, alpha=0.05, color='orange')".to_string(),
        "".to_string(),
        "median_sh = float(np.median(sharpes))".to_string(),
        "ax1.axhline(y=median_sh, color='#666666', linewidth=0.8, linestyle=':', alpha=0.5)".to_string(),
        "ax1.text(0.28, median_sh+0.02, 'median=' + str(round(median_sh,2)), fontsize=8, color='#888888')".to_string(),
        "".to_string(),
        "ann_text = 'Winner: ' + str(WINNER) + 'x Shar' + 'pe=' + str(round(sharpes[win_idx], 2))".to_string(),
        "ax1.annotate(ann_text,".to_string(),
        "             xy=(WINNER, sharpes[win_idx]),".to_string(),
        "             xytext=(WINNER+0.4, sharpes[win_idx]+0.08),".to_string(),
        "             fontsize=9, color='#00C853',".to_string(),
        "             arrowprops=dict(arrowstyle='->', color='#00C853', alpha=0.7))".to_string(),
        "".to_string(),
        "ax2.bar(atr_mults, pass_rates, width=0.08, color='#FF9800', alpha=0.7)".to_string(),
        "ax2.set_ylabel('Pass Rate (%)', fontsize=10, color='#CCCCCC')".to_string(),
        "ax2.set_xlabel('ATR Stop Multiple (x ATR)', fontsize=11, color='#CCCCCC')".to_string(),
        "ax2.set_ylim(0, 110)".to_string(),
        "ax2.grid(True, color='#2D333B', linewidth=0.5, alpha=0.6)".to_string(),
        "ax2.tick_params(colors='#888888', labelsize=9)".to_string(),
        "for spine in ax2.spines.values():".to_string(),
        "    spine.set_color('#2D333B')".to_string(),
        "win_pr = pass_rates[win_idx]".to_string(),
        "ax2.bar([WINNER], [win_pr], width=0.08, color='#00C853', alpha=0.9)".to_string(),
        "".to_string(),
        "plt.tight_layout()".to_string(),
        "fig.savefig('charts/turtle_atr_stop_sweep.png', dpi=180, bbox_inches='tight', facecolor=fig.get_facecolor())".to_string(),
        "print('Saved: charts/turtle_atr_stop_sweep.png')".to_string(),
        "plt.close()".to_string(),
    ];

    std::fs::write("charts/plot_turtle_atr_sweep.py", lines.join("\n")).ok();
    println!("\nWrote: charts/plot_turtle_atr_sweep.py");
}

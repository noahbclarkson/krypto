//! T78: ATR_RANK_THRESHOLD Extensive Sweep — Live-Compatible Path
//!
//! PURPOSE: Extensively sweep ATR_RANK_THRESHOLD (T) using the Turtle-only live exit
//! path (matching src/live/bot.rs). This is the ONE parameter that has NOT been
//! sweep-tested at full resolution with the current production regime setup (AP=17, LB=41).
//!
//! PRIOR: T=5.0 was set from a coarse 21-value grid (0,5,10,...,100) with AP=12/LB=42.
//! This harness tests T with the CURRENT production regime: AP=17/LB=41.
//!
//! HYPOTHESIS: T=5.0 may not be optimal. The coarse sweep may have missed a better
//! robustness point at higher T values (more filtering) or at T=0 (less filtering).
//!
//! RANGE: T ∈ [0..=100 step 1] — 101 values × 9 universes × 7 WF windows = 6,363 runs
//! PARAMS: EP=21, ATR(24,2.0), HM=12, CAP=3, ATR_RANK(AP=17,LB=41,T=<swept>)
//! EXIT:   Turtle-only (highest_high - ATR_MULT*ATR) — matches live bot exactly
//! OVERLAY: USDT hedge (HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.40, HEDGE_ATR_PERIOD=38)
//!
//! EXPORTS:
//!   snapshots/t78_threshold_sweep.csv      — all per-window results (6,363 rows)
//!   snapshots/t78_threshold_summary.csv   — aggregated per-threshold (101 rows)
//!   snapshots/t78_equity_T000.csv        — equity for T=0 baseline
//!   snapshots/t78_equity_T005.csv        — equity for T=5 current
//!   snapshots/t78_equity_T010.csv        — equity for T=10 runner-up
//!   snapshots/t78_equity_T020.csv        — equity for T=20 runner-up
//!   snapshots/t78_equity_T040.csv        — equity for T=40 runner-up
//!   snapshots/t78_equity_T060.csv        — equity for T=60 runner-up
//!   snapshots/t78_equity_T080.csv        — equity for T=80 runner-up
//!   snapshots/t78_equity_T100.csv        — equity for T=100
//!   charts/comparison_chart.png           — Python matplotlib chart

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 92;

// Production regime params (AP=17, LB=41 confirmed via T75/T75 held-out)
const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 41;

// USDT hedge overlay (confirmed via T75)
const HEDGE_ATR_PERIOD: usize = 38;
const HEDGE_ATR_PCT: f64 = 0.45;
const HEDGE_SIZE_MULT: f64 = 0.40;
const HEDGE_LOOKBACK: usize = 252;

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

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    vals[idx.saturating_sub(window - 1)..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if atr_mult > 0.0 {
            let atr = atr_at(high, low, close, atr_period, idx);
            if close[idx] < max_close + atr * atr_mult {
                return false;
            }
        }
        return true;
    }
    false
}

fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    let mut hist = Vec::with_capacity(lookback);
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = 1.0;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
    pass: bool,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_rank_t: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };
        
        if btc_pct < atr_rank_t {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let mut size_mult = 1.0;
                        
                        // USDT hedge overlay
                        if let Some(b) = btc {
                            if bar >= HEDGE_LOOKBACK + HEDGE_ATR_PERIOD {
                                let mut trs = Vec::with_capacity(HEDGE_ATR_PERIOD);
                                for i in (bar + 1 - HEDGE_ATR_PERIOD)..=bar {
                                    let h = b.high[i];
                                    let l = b.low[i];
                                    let c0 = b.close[i.saturating_sub(1)];
                                    trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
                                }
                                let hedge_atr = trs.iter().sum::<f64>() / HEDGE_ATR_PERIOD as f64;
                                let mut hist = Vec::with_capacity(HEDGE_LOOKBACK);
                                for j in (bar + 1 - HEDGE_LOOKBACK)..=bar {
                                    let h = b.high[j];
                                    let l = b.low[j];
                                    let c0 = b.close[j.saturating_sub(1)];
                                    hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
                                }
                                hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
                                let pct_threshold = hist[pct_idx.min(hist.len().saturating_sub(1))];
                                if hedge_atr > pct_threshold {
                                    size_mult = HEDGE_SIZE_MULT;
                                }
                            }
                        }

                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        
                        let mut atr_buf: std::collections::VecDeque<f64> = std::collections::VecDeque::new();
                        
                        for b in entry_bar_next..=max_bar {
                            if sd.high[b] > highest_high { highest_high = sd.high[b]; }
                            let c0 = sd.close[b.saturating_sub(1)];
                            let tr = (sd.high[b] - sd.low[b]).max((sd.high[b] - c0).abs()).max((sd.low[b] - c0).abs());
                            atr_buf.push_back(tr);
                            if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.pop_front(); }
                            
                            if atr_buf.len() == TURTLE_ATR_PERIOD {
                                let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                                let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                                if sd.low[b] <= turtle_stop {
                                    exit_bar = b;
                                    break;
                                }
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let pct_ret = exit / entry - 1.0;
                            let gross_ret = pct_ret * size_mult;
                            
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            if equity > peak { peak = equity; }
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break;
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
    
    let sharpe = annualised_sharpe(&daily_rets);
    WfResult {
        equity,
        sharpe,
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
        pass: total_trades >= MIN_TRADES && sharpe > 0.0,
    }
}

fn main() -> Result<()> {
    println!("Loading data...");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols = UNIVERSES.iter().flat_map(|(_, s)| s.iter()).map(|&s| s).collect::<std::collections::HashSet<_>>();
    rt.block_on(async {
        for &sym in &all_symbols {
            let df = loader.fetch_data(sym, "1d", CANDLES).await.unwrap();
            let close = df.column("close").unwrap().f64().unwrap().into_no_null_iter().collect::<Vec<_>>();
            let high = df.column("high").unwrap().f64().unwrap().into_no_null_iter().collect::<Vec<_>>();
            let low = df.column("low").unwrap().f64().unwrap().into_no_null_iter().collect::<Vec<_>>();
            let vol = df.column("volume").unwrap().f64().unwrap().into_no_null_iter().collect::<Vec<_>>();
            if close.len() < min_len { min_len = close.len(); }
            sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
        }
    });

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    if windows == 0 { return Ok(()); }
    println!("Windows: {}, min_len: {}", windows, min_len);

    // ── Full sweep: T ∈ [0..=100 step 1] ─────────────────────────────────
    let thresholds: Vec<f64> = (0..=100).map(|x| x as f64).collect();
    let n_thresh = thresholds.len();
    
    // Per-threshold aggregates
    let mut summary_pass = vec![0usize; n_thresh];
    let mut summary_trades = vec![0usize; n_thresh];
    let mut summary_sharpe = vec![0.0_f64; n_thresh];
    let mut summary_ret = vec![0.0_f64; n_thresh];
    let mut summary_dd = vec![0.0_f64; n_thresh];

    // Per-window results for the sweep CSV
    let mut sweep_csv = File::create("snapshots/t78_threshold_sweep.csv")?;
    writeln!(sweep_csv, "universe,window,threshold,return_pct,sharpe,max_dd_pct,trades,pass")?;

    // Equity exports for selected thresholds
    let equity_thresholds = vec![0.0_f64, 5.0_f64, 10.0_f64, 20.0_f64, 40.0_f64, 60.0_f64, 80.0_f64, 100.0_f64];
    let mut equity_files: HashMap<i32, File> = HashMap::new();
    for &t in &equity_thresholds {
        let label = format!("{:03}", t as i32);
        let path = format!("snapshots/t78_equity_T{}.csv", label);
        let mut f = File::create(&path)?;
        writeln!(f, "universe,window,equity")?;
        equity_files.insert(t as i32, f);
    }

    for (ti, &t) in thresholds.iter().enumerate() {
        let mut total_passes = 0usize;
        let mut total_trades = 0usize;
        let mut total_sharpe = 0.0_f64;
        let mut total_ret = 0.0_f64;
        let mut total_dd = 0.0_f64;
        let mut total_windows = 0usize;

        for (u_name, u_syms) in UNIVERSES {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, t);
                
                let ret_pct = (res.equity - 1.0) * 100.0;
                writeln!(sweep_csv, "{},{},{:.1},{:.4},{:.6},{:.4},{},{}",
                    u_name, w, t, ret_pct, res.sharpe, res.dd, res.trades, if res.pass { 1 } else { 0 })?;

                total_passes += if res.pass { 1 } else { 0 };
                total_trades += res.trades;
                total_sharpe += res.sharpe;
                total_ret += ret_pct;
                total_dd += res.dd;
                total_windows += 1;

                // Write equity for selected thresholds
                if equity_files.contains_key(&(t as i32)) {
                    if let Some(f) = equity_files.get_mut(&(t as i32)) {
                        writeln!(f, "{},{},{:.6}", u_name, w, res.equity)?;
                    }
                }
            }
        }

        summary_pass[ti] = total_passes;
        summary_trades[ti] = total_trades;
        summary_sharpe[ti] = total_sharpe / total_windows as f64;
        summary_ret[ti] = total_ret / total_windows as f64;
        summary_dd[ti] = total_dd / total_windows as f64;

        if (ti % 10) == 0 || t == 5.0 || t == 0.0 {
            println!("T={:3.0}: pass={:2}/{} ({:5.1}%), Sharpe={:7.3}, Ret={:+8.1}%, DD={:6.2}%, Trades={}",
                t, total_passes, total_windows,
                (total_passes as f64 / total_windows as f64) * 100.0,
                summary_sharpe[ti], summary_ret[ti], summary_dd[ti], total_trades);
        }
    }

    // ── Write summary CSV ─────────────────────────────────────────────────
    let mut summary_csv = File::create("snapshots/t78_threshold_summary.csv")?;
    writeln!(summary_csv, "threshold,pass_count,total_trades,avg_return_pct,avg_sharpe,avg_dd_pct")?;
    for (ti, &t) in thresholds.iter().enumerate() {
        writeln!(summary_csv, "{:.1},{},{},{:.4},{:.6},{:.4}",
            t, summary_pass[ti], summary_trades[ti],
            summary_ret[ti], summary_sharpe[ti], summary_dd[ti])?;
    }
    println!("\nSummary written to snapshots/t78_threshold_summary.csv");

    // ── Find winners ───────────────────────────────────────────────────────
    let mut winners: Vec<(usize, f64)> = thresholds.iter()
        .enumerate()
        .map(|(i, &t)| (i, t))
        .collect();
    winners.sort_by(|a, b| {
        let pa = summary_pass[a.0];
        let pb = summary_pass[b.0];
        let sa = summary_sharpe[a.0];
        let sb = summary_sharpe[b.0];
        // Sort by pass count desc, then by Sharpe desc
        pb.cmp(&pa).then_with(|| sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal))
    });

    println!("\n=== TOP 10 BY PASS COUNT ===");
    for &(idx, t) in winners.iter().take(10) {
        println!("T={:3.0}: pass={:2}/63 ({:5.1}%), Sharpe={:7.3}, Ret={:+8.1}%, Trades={}",
            t, summary_pass[idx], (summary_pass[idx] as f64 / 63.0) * 100.0,
            summary_sharpe[idx], summary_ret[idx], summary_trades[idx]);
    }

    // ── Write winner report ────────────────────────────────────────────────
    let best_idx = winners[0].0;
    let best_t = winners[0].1;
    let best_pass = summary_pass[best_idx];
    let best_sharpe = summary_sharpe[best_idx];
    let best_ret = summary_ret[best_idx];
    let best_dd = summary_dd[best_idx];
    let best_trades = summary_trades[best_idx];

    let md_path = "snapshots/t78_threshold_sweep.md";
    let mut f = File::create(md_path)?;
    writeln!(f, "# T78: ATR_RANK_THRESHOLD Extensive Sweep Results")?;
    writeln!(f, "")?;
    writeln!(f, "**Date:** 2026-05-06")?;
    writeln!(f, "**Harness:** live-compatible walk-forward (Turtle-only, matches `src/live/bot.rs`)")?;
    writeln!(f, "**Regime params:** AP={}, LB={} (current production)", REGIME_ATR_PERIOD, REGIME_LOOKBACK)?;
    writeln!(f, "**Range:** T ∈ [0..=100 step 1] — 101 values × 9 universes × {} windows = {} runs", windows, 101 * 9 * windows)?;
    writeln!(f, "")?;
    writeln!(f, "## Winner: T={:.0}", best_t)?;
    writeln!(f, "| Metric | Value |")?;
    writeln!(f, "|--------|-------|")?;
    writeln!(f, "| Pass Rate | {}/63 ({:.1}%) |", best_pass, (best_pass as f64 / 63.0) * 100.0)?;
    writeln!(f, "| Avg Sharpe | {:.3} |", best_sharpe)?;
    writeln!(f, "| Avg Return | {:.1}% |", best_ret)?;
    writeln!(f, "| Avg DD | {:.2}% |", best_dd)?;
    writeln!(f, "| Total Trades | {} |", best_trades)?;
    writeln!(f, "")?;
    writeln!(f, "## Top 10 by Pass Rate")?;
    writeln!(f, "| Rank | T | Pass | Pass% | Sharpe | Ret% | Trades |")?;
    writeln!(f, "|------|---|------|-------|--------|------|--------|")?;
    for (rank, &(idx, t)) in winners.iter().enumerate().take(10) {
        let p = summary_pass[idx];
        let s = summary_sharpe[idx];
        let r = summary_ret[idx];
        let tt = summary_trades[idx];
        writeln!(f, "| {} | {:.0} | {}/63 | {:.1}% | {:.3} | {:.1}% | {} |",
            rank + 1, t, p, (p as f64 / 63.0) * 100.0, s, r, tt)?;
    }
    writeln!(f, "")?;
    writeln!(f, "## Current Default: T=5.0")?;
    let t5_idx = thresholds.iter().position(|&x| x == 5.0).unwrap();
    writeln!(f, "| Metric | T=0 | T=5 | T=best | Delta (T=5 vs best) |")?;
    writeln!(f, "|--------|-----|-----|--------|----------------------|")?;
    writeln!(f, "| Pass | {}/63 | {}/63 | {}/63 | -{} |",
        summary_pass[0], summary_pass[t5_idx], best_pass, best_pass - summary_pass[t5_idx])?;
    writeln!(f, "| Sharpe | {:.3} | {:.3} | {:.3} | {:+.3} |",
        summary_sharpe[0], summary_sharpe[t5_idx], best_sharpe, summary_sharpe[t5_idx] - best_sharpe)?;
    writeln!(f, "| Return | {:.1}% | {:.1}% | {:.1}% | {:+.1}pp |",
        summary_ret[0], summary_ret[t5_idx], best_ret, summary_ret[t5_idx] - best_ret)?;
    writeln!(f, "| DD | {:.2}% | {:.2}% | {:.2}% | {:+.2}pp |",
        summary_dd[0], summary_dd[t5_idx], best_dd, summary_dd[t5_idx] - best_dd)?;
    writeln!(f, "| Trades | {} | {} | {} | {} |",
        summary_trades[0], summary_trades[t5_idx], best_trades, summary_trades[t5_idx] as i32 - best_trades as i32)?;
    writeln!(f, "")?;
    writeln!(f, "## Charts")?;
    writeln!(f, "- `charts/t78_comparison_chart.png` — pass rate, Sharpe, return vs threshold")?;
    writeln!(f, "- `charts/t78_equity_comparison.png` — equity curve comparison (selected T values)")?;

    println!("\nMarkdown report written to {}", md_path);

    Ok(())
}

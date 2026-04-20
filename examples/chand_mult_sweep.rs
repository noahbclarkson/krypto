//! CHAND_MULT Hyperparameter Sweep — Extensive Range
//!
//! Sweeps CHAND_MULT ∈ [0.50, 5.00] step 0.25 (19 values)
//! Walk-forward: 252/252 train/test on Base5 universe
//! Fixed: CP=15, EP=21, ATR=24, ATR_M=2.0, HM=45, CAP=3
//!
//! Goal: Find the CHAND_MULT that maximizes robustness (pass rate × Sharpe)
//! across all walk-forward windows, not just the best single backtest.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 15;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

// Sweep range
const M_START: f64 = 0.50;
const M_END: f64 = 5.00;
const M_STEP: f64 = 0.25;

const BASE5: &[&str] = &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];

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
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_dv(close: &[f64], vol: &[f64], lookback: usize, bar: usize) -> f64 {
    if bar < lookback.saturating_sub(1) { return close.get(bar).copied().unwrap_or(0.0) * vol.get(bar).copied().unwrap_or(0.0); }
    let start = bar + 1 - lookback;
    let mut sum = 0.0;
    for i in start..=bar {
        let c = close.get(i).copied().unwrap_or(0.0);
        let v = vol.get(i).copied().unwrap_or(0.0);
        sum += c * v;
    }
    sum / lookback as f64
}

struct WindowResult {
    trades: usize,
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    win_rate: f64,
}

fn run_window(
    sym_data: &HashMap<String, SymData>,
    symbols: &[&str],
    train_end: usize,
    test_end: usize,
    chand_mult: f64,
) -> WindowResult {
    let warmup = CHAND_PERIOD.max(TURTLE_ATR_PERIOD).max(TURTLE_ENTRY) + TURTLE_ATR_PERIOD;
    let test_start = train_end;
    let n = test_end.min(sym_data.values().next().map(|s| s.close.len()).unwrap_or(0));

    // Rank symbols by dollar volume at test_start
    let mut scores: Vec<(&str, f64)> = symbols.iter().filter_map(|s| {
        sym_data.get(*s).and_then(|sd| {
            if test_start < sd.close.len() {
                let dv = rolling_dv(&sd.close, &sd.vol, VOL_LOOKBACK, test_start);
                Some((*s, if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }))
            } else { None }
        })
    }).collect();
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let top: Vec<&str> = scores.iter().take(POSITION_CAP).map(|(s, _)| *s).collect();

    let mut trades = 0usize;
    let mut rets = Vec::new();
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;

    let mut bar = test_start.max(warmup);
    while bar < n {
        // Try entry on top-ranked symbol
        let mut entered = false;
        for &sym in &top {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    let start_idx = bar + 1 - TURTLE_ENTRY;
                    let mut max_close = f64::NEG_INFINITY;
                    for i in start_idx..bar {
                        if let Some(&c) = sd.close.get(i) { max_close = max_close.max(c); }
                    }
                    let curr_close = sd.close.get(bar).copied().unwrap_or(0.0);
                    if curr_close > max_close {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let next_bar = bar + 1;
                        let max_hold = (next_bar + HOLD_MAX).min(sd.close.len().saturating_sub(1));
                        let mut exit_bar = max_hold;
                        let mut hh_c = sd.high.get(next_bar).copied().unwrap_or(0.0);
                        let mut hh_t = sd.high.get(next_bar).copied().unwrap_or(0.0);

                        for b in next_bar..=max_hold {
                            hh_c = hh_c.max(sd.high.get(b).copied().unwrap_or(0.0));
                            let atr_c = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_c = hh_c - chand_mult * atr_c;
                            hh_t = hh_t.max(sd.high.get(b).copied().unwrap_or(0.0));
                            let atr_t = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_t = hh_t - TURTLE_ATR_MULT * atr_t;
                            if sd.close.get(b).copied().unwrap_or(0.0) < trail_c
                            || sd.close.get(b).copied().unwrap_or(0.0) < trail_t {
                                exit_bar = b; break;
                            }
                        }

                        let exit_px = sd.close.get(exit_bar).copied().unwrap_or(entry_px);
                        let exit = exit_px * (1.0 - TAKER_FEE);
                        let gross_ret = exit / entry - 1.0;
                        equity *= 1.0 + gross_ret;
                        peak = peak.max(equity);
                        let dd = (equity / peak - 1.0).min(0.0);
                        max_dd = max_dd.min(dd);
                        rets.push(gross_ret);
                        trades += 1;
                        bar = exit_bar + 1;
                        entered = true;
                        break;
                    }
                }
            }
        }
        if !entered { bar += 1; }
    }

    let sharpe = if rets.len() >= 2 {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (rets.len() - 1) as f64;
        let std = var.sqrt();
        if std > 1e-10 { mean / std * (252f64).sqrt() } else { 0.0 }
    } else { 0.0 };

    let wins = rets.iter().filter(|&&r| r > 0.0).count();
    let win_rate = if !rets.is_empty() { wins as f64 / rets.len() as f64 * 100.0 } else { 0.0 };

    WindowResult { trades, ret: equity - 1.0, sharpe, max_dd, win_rate }
}

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("=== CHAND_MULT Sweep (Extensive Range) ===");
    eprintln!("Range: M ∈ [{:.2}, {:.2}] step {:.2}", M_START, M_END, M_STEP);
    eprintln!("Universe: Base5 | CP={}, EP={}, ATR={}, ATR_M={}", CHAND_PERIOD, TURTLE_ENTRY, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT);

    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    for &sym in BASE5 {
        let df = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let c = df.column("close")?.f64()?;
        let h = df.column("high")?.f64()?;
        let l = df.column("low")?.f64()?;
        let v = df.column("volume")?.f64()?;
        sym_data.insert(sym.to_string(), SymData {
            close: c.into_iter().filter_map(|x| x).collect(),
            high:  h.into_iter().filter_map(|x| x).collect(),
            low:   l.into_iter().filter_map(|x| x).collect(),
            vol:   v.into_iter().filter_map(|x| x).collect(),
        });
    }

    let min_len = sym_data.values().map(|s| s.close.len()).min().unwrap_or(0);
    let total_windows = (min_len.saturating_sub(TRAIN_BARS + TEST_BARS)) / TEST_BARS;

    eprintln!("Data: {} bars, {} windows", min_len, total_windows);

    // Generate M values
    let mut m_values: Vec<f64> = Vec::new();
    let mut m = M_START;
    while m <= M_END + 1e-9 {
        m_values.push((m * 100.0).round() / 100.0); // avoid float drift
        m += M_STEP;
    }
    eprintln!("Testing {} CHAND_MULT values", m_values.len());

    // Sweep
    let mut all_results: Vec<(f64, usize, f64, f64, f64, f64, f64, bool)> = Vec::new(); // (M, window, ret, sharpe, dd, trades, win_rate, pass)

    for &cm in &m_values {
        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (train_end + TEST_BARS).min(min_len);
            let wr = run_window(&sym_data, BASE5, train_end, test_end, cm);
            let pass = wr.trades >= MIN_TRADES && wr.ret > 0.0;
            all_results.push((cm, wi, wr.ret * 100.0, wr.sharpe, wr.max_dd * 100.0, wr.trades as f64, wr.win_rate, pass));
        }
    }

    // Aggregate per M value
    let mut csv_out = String::from("chand_mult,avg_return,avg_sharpe,avg_max_dd,avg_trades,avg_win_rate,pass_rate,windows_passed,total_windows\n");
    let mut agg: Vec<(f64, f64, f64, f64, f64, f64, f64, usize, usize)> = Vec::new();

    for &cm in &m_values {
        let runs: Vec<_> = all_results.iter().filter(|(m, _, _, _, _, _, _, _)| (*m - cm).abs() < 1e-9).collect();
        if runs.is_empty() { continue; }
        let n = runs.len();
        let avg_ret: f64 = runs.iter().map(|(_, _, r, _, _, _, _, _)| *r).sum::<f64>() / n as f64;
        let avg_sharpe: f64 = runs.iter().map(|(_, _, _, s, _, _, _, _)| *s).sum::<f64>() / n as f64;
        let avg_dd: f64 = runs.iter().map(|(_, _, _, _, d, _, _, _)| *d).sum::<f64>() / n as f64;
        let avg_trades: f64 = runs.iter().map(|(_, _, _, _, _, t, _, _)| *t).sum::<f64>() / n as f64;
        let avg_wr: f64 = runs.iter().map(|(_, _, _, _, _, _, w, _)| *w).sum::<f64>() / n as f64;
        let passed: usize = runs.iter().filter(|(_, _, _, _, _, _, _, p)| *p).count();
        let pass_rate = passed as f64 / n as f64 * 100.0;

        csv_out.push_str(&format!("{:.2},{:.2},{:.3},{:.1},{:.1},{:.1},{:.1},{}/{}\n",
            cm, avg_ret, avg_sharpe, avg_dd, avg_trades, avg_wr, pass_rate, passed, n));
        agg.push((cm, avg_ret, avg_sharpe, avg_dd, avg_trades, avg_wr, pass_rate, passed, n));
    }

    // Write CSV
    let csv_path = "snapshots/chand_mult_sweep.csv";
    std::fs::write(csv_path, &csv_out)?;
    eprintln!("\nResults written to {}", csv_path);

    // Also write per-window detail
    let mut detail_csv = String::from("chand_mult,window,return_pct,sharpe,max_dd_pct,trades,win_rate,pass\n");
    for (cm, wi, ret, sharpe, dd, trades, wr, pass) in &all_results {
        detail_csv.push_str(&format!("{:.2},{},{:.2},{:.3},{:.1},{},{:.1},{}\n",
            cm, wi, ret, sharpe, dd, *trades as i64, wr, if *pass { "PASS" } else { "FAIL" }));
    }
    std::fs::write("snapshots/chand_mult_sweep_detail.csv", &detail_csv)?;

    // Print summary sorted by Sharpe
    let mut sorted = agg.clone();
    sorted.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());
    eprintln!("\n=== CHAND_MULT Sweep Results (sorted by avg Sharpe) ===");
    eprintln!("{:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}", "M", "AvgRet%", "AvgSharpe", "AvgDD%", "AvgTrades", "AvgWR%", "PassRate%");
    eprintln!("{}", "-".repeat(72));
    for (cm, ret, sharpe, dd, trades, wr, pr, _, _) in &sorted {
        let marker = if (*cm - 1.50).abs() < 1e-9 { " ← current" } else { "" };
        eprintln!("{:>10.2} {:>10.2} {:>10.3} {:>10.1} {:>10.1} {:>10.1} {:>10.1}%{}", cm, ret, sharpe, dd, trades, wr, pr, marker);
    }

    // Find robustness winner: highest pass_rate * Sharpe composite
    let mut robust: Vec<_> = agg.iter().map(|(cm, ret, sharpe, dd, trades, wr, pr, passed, n)| {
        let composite = *pr / 100.0 * sharpe;
        (*cm, *ret, *sharpe, *dd, *trades, *wr, *pr, *passed, *n, composite)
    }).collect();
    robust.sort_by(|a, b| b.9.partial_cmp(&a.9).unwrap());

    eprintln!("\n=== Robustness Winner (pass_rate × Sharpe composite) ===");
    for (i, (cm, ret, sharpe, dd, trades, wr, pr, _, _, comp)) in robust.iter().enumerate().take(5) {
        let marker = if (*cm - 1.50).abs() < 1e-9 { " ← current" } else if i == 0 { " ← WINNER" } else { "" };
        eprintln!("  #{}: M={:.2} Sharpe={:.3} PassRate={:.1}% Composite={:.3}{}", i+1, cm, sharpe, pr, comp, marker);
    }

    // Check if current M=1.50 is still optimal
    let current = agg.iter().find(|(cm, _, _, _, _, _, _, _, _)| (*cm - 1.50).abs() < 1e-9);
    let winner = robust.first();
    if let (Some((_, _, cur_sharpe, _, _, _, cur_pr, _, _)), Some((w_cm, _, w_sharpe, _, _, _, w_pr, _, _, _))) = (current, winner) {
        if (*w_cm - 1.50).abs() < 1e-9 {
            eprintln!("\n✅ Current CHAND_MULT=1.50 CONFIRMED as optimal (robustness winner)");
        } else {
            eprintln!("\n⚠️  CHAND_MULT={:.2} beats current M=1.50: Sharpe {:.3} vs {:.3}, PassRate {:.1}% vs {:.1}%",
                w_cm, w_sharpe, cur_sharpe, w_pr, cur_pr);
        }
    }

    println!("\nDone.");
    Ok(())
}

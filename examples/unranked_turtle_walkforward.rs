//! Unranked Turtle+Chandelier Walk-Forward
//!
//! IDENTICAL execution model to turtle_chandelier_walkforward.rs (ranked), EXCEPT:
//!   RANKED: only check top-CAP symbols by DV rank for Turtle entry
//!   UNRANKED: check ALL symbols for Turtle entry (no DV-rank gate)
//!
//! The ONLY difference is the DV-rank gate. All else (bar advancement, dual exit,
//! CAP enforcement) is identical. This is a pure test of whether the rank gate helps.
//!
//! Walk-forward: 252-bar train / 252-bar test
//! Universe: NoDOGE (BTC, ETH, SOL, XRP, DOGE)
//!
//! Usage: cargo run --profile sweep --example unranked_turtle_walkforward

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 5000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 25;
const TURTLE_ATR_MULT: f64 = 2.00;

const SYMBOLS: [&str; 5] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

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
        let c0 = *close.get(i.saturating_sub(1)).unwrap_or(&0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal(close: &[f64], _high: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        curr_close > max_close
    } else {
        false
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 252.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

/// UNRANKED: identical to turtle_chandelier_walkforward.rs execution model,
/// but checks ALL symbols for Turtle entry (not just top-CAP by DV).
/// CAP still enforced. This is a pure test of whether the DV-rank gate helps.
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut daily_rets = Vec::new();
    let mut wins = 0usize;
    let mut total_trades = 0usize;

    let mut bar = test_start;
    while bar + 2 < test_end {
        // UNRANKED: evaluate ALL symbols, not just top-CAP by DV
        // Sort by DV only for PRIORITY ordering, not to filter
        let mut candidates: Vec<(f64, &str)> = Vec::new(); // (dv, sym)
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                candidates.push((if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }, sym.as_str()));
            }
        }
        // Sort by DV descending — highest volume first (same priority logic as ranked,
        // but we DON'T restrict to top-CAP symbols)
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

        // Try to enter ALL valid signals, up to CAP (no rank gate)
        let mut entered_this_bar = false;
        for (_, sym) in candidates {
            // Already at CAP? Stop entering
            if entered_this_bar { break; }

            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // DUAL_EXIT: identical to ranked harness
                        let mut highest_high_chand = sd.high[entry_bar_next.min(n-1)];
                        let mut highest_high_turtle = sd.high[entry_bar_next.min(n-1)];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;

                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let gross_ret = exit_px / entry_px - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered_this_bar = true;
                            break; // identical to ranked: only one entry per bar
                        }
                    }
                }
            }
        }

        if !entered_this_bar {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Unranked Turtle+Chandelier Walk-Forward ====");
    eprintln!("Universe: NoDOGE (BTC, ETH, SOL, XRP, DOGE)");
    eprintln!("Strategy: Trade ALL valid Turtle signals (no DV-rank gate), cap={}", POSITION_CAP);
    eprintln!("Note: Only 1 entry per bar (same as ranked). DV gate removed = more symbols checked.");
    eprintln!("Windows: 252-train / 252-test\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for s in SYMBOLS {
        all_syms.insert(s.to_string());
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in all_syms.iter() {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
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
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let symbols: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    eprintln!("Running {} windows...\n", total_windows);

    let mut all_results: Vec<WfResult> = Vec::new();
    let mut passing = 0usize;

    for w in 0..total_windows {
        if w * TEST_BARS + TRAIN_BARS + 3 > n { break; }
        let result = run_sim(
            &sym_data_map,
            &symbols,
            w * TEST_BARS + TRAIN_BARS,
            (w * TEST_BARS + TRAIN_BARS + TEST_BARS).min(n),
        );
        all_results.push(result);
        let r = &all_results[w];

        if r.pass { passing += 1; }

        eprintln!("W{:02}: {} trades | Sharpe {:>6.2} | Return {:>+8.1}% | DD {:>6.1}% | WR {:>5.1}% | {}",
            w, r.trades, r.sharpe, r.ret, r.max_dd, r.win_rate,
            if r.pass { "PASS" } else { "FAIL" });
    }

    let avg_sharpe: f64 = all_results.iter().map(|r| r.sharpe).sum::<f64>() / total_windows as f64;
    let avg_ret: f64 = all_results.iter().map(|r| r.ret).sum::<f64>() / total_windows as f64;
    let avg_dd: f64 = all_results.iter().map(|r| r.max_dd).sum::<f64>() / total_windows as f64;
    let avg_wr: f64 = all_results.iter().map(|r| r.win_rate).sum::<f64>() / total_windows as f64;
    let total_trades: usize = all_results.iter().map(|r| r.trades).sum();
    let pass_rate = passing as f64 / total_windows as f64 * 100.0;

    eprintln!("\n=== SUMMARY ===");
    eprintln!("Pass rate: {}/{} ({:.0}%)", passing, total_windows, pass_rate);
    eprintln!("Avg Sharpe: {:>6.2}", avg_sharpe);
    eprintln!("Avg Return: {:>+8.1}%", avg_ret);
    eprintln!("Avg DD: {:>6.1}%", avg_dd);
    eprintln!("Avg WR: {:>5.1}%", avg_wr);
    eprintln!("Total trades: {}", total_trades);

    eprintln!("\n=== COMPARISON ===");
    eprintln!("                        RANKED        UNRANKED");
    eprintln!("NoDOGE pass rate:       6/6 (100%)    {}/{} ({:.0}%)", passing, total_windows, pass_rate);
    eprintln!("NoDOGE avg Sharpe:     5.46           {:>6.2}", avg_sharpe);
    eprintln!("NoDOGE avg Return:    +95.8%         {:>+8.1}%", avg_ret);
    eprintln!("NoDOGE avg DD:        48.7%          {:>6.1}%", avg_dd);

    let verdict = if pass_rate >= 83.3 {
        "UNRANKED PASSES WF — consider removing DV-rank gate"
    } else {
        "RANKED still dominant — keep DV-rank gate in production"
    };
    eprintln!("\nVerdict: {}", verdict);

    let mut csv = File::create("snapshots/unranked_turtle_wf.csv")?;
    writeln!(csv, "window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;
    for (w, r) in all_results.iter().enumerate() {
        writeln!(csv, "W{:02},{:.4},{:.4},{:.4},{},{:.4},{}",
            w, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass)?;
    }
    eprintln!("\nResults: snapshots/unranked_turtle_wf.csv");
    eprintln!("Elapsed: {:?}", t0.elapsed());

    Ok(())
}

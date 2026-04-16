//! ============================================================
//! TURTLE ENTRY MODE SWEEP — max_close vs max_high
//! ============================================================
//!
//! Background:
//!   PLAN.md notes: "Signal definition matters enormously:
//!   `close > max_close` gives 69% while `close > max_high` gives only 59%."
//!   The production harness uses `close > max_close` (close must exceed
//!   the max PRIOR CLOSE in the lookback window).
//!
//!   BUT: since high >= close always, max(high) >= max(close),
//!   so `close > max(high)` is STRICTER than `close > max(close)`.
//!   Stricter → fewer but potentially higher-quality signals.
//!
//!   PLAN.md may have these reversed (more testing needed).
//!
//! Two modes:
//!   MODE=0: close > max(close) — current implementation (easier to trigger)
//!   MODE=1: close > max(high) — stricter breakout (harder to trigger)
//!
//! Hypothesis: MODE=1 (max_high) may produce fewer but better signals,
//!   compensating for reduced trade frequency.
//!
//! Frozen params: EP=21, CHAND(28,2.0), TURTLE_ATR(25,2.0),
//!   CAP=3, HM=45, MIN_TRADES=3, FEE=20bp RT

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

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

const CSV_OUT: &str = "snapshots/turtle_entry_mode_wf.csv";
const CSV_EQUITY: &str = "snapshots/turtle_entry_mode_equity.csv";

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

// entry_mode: 0 = close > max(close), 1 = close > max(high)
fn turtle_signal(close: &[f64], high: &[f64], entry_period: usize, idx: usize, entry_mode: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    if entry_mode == 0 {
        // close > max_close
        let mut max_close = f64::NEG_INFINITY;
        for i in start..idx {
            if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
        }
        if let Some(&curr_close) = close.get(idx) {
            curr_close > max_close
        } else { false }
    } else {
        // close > max_high (true Turtle breakout — must exceed highest high)
        let mut max_high = f64::NEG_INFINITY;
        for i in start..idx {
            if let Some(&h) = high.get(i) { max_high = max_high.max(h); }
        }
        if let Some(&curr_close) = close.get(idx) {
            curr_close > max_high
        } else { false }
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
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

struct SimResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_steps: Vec<f64>,
    daily_rets: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    entry_mode: usize,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
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
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, EP, bar, entry_mode) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
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
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }
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
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    SimResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_steps: equity_curve, daily_rets }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("=== Turtle Entry Mode Sweep: max_close vs max_high ===");
    eprintln!("MODE=0: close > max(close) [current production]");
    eprintln!("MODE=1: close > max(high) [stricter breakout]");
    eprintln!();

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
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
    for sym in &all_syms {
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

    let entry_modes = [0usize, 1usize];
    let mode_labels = ["max_close", "max_high"];

    // Per-window results CSV
    let mut csv_lines = vec!["entry_mode,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    // Global stats per mode per universe
    let mut stats: HashMap<(usize, &str), (usize, usize, f64, f64, usize)> = HashMap::new();
    // (global_pass, global_total, sharpe_sum, ret_sum, trades_sum)

    for &mode in &entry_modes {
        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            let mut uni_pass = 0usize;
            let mut uni_total = 0usize;
            let mut uni_sharpe_sum = 0.0_f64;
            let mut uni_ret_sum = 0.0_f64;
            let mut uni_trades = 0usize;

            for wi in 0..total_windows {
                let test_start = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, mode);

                csv_lines.push(format!("{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    mode_labels[mode], label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass));

                uni_total += 1;
                uni_sharpe_sum += r.sharpe;
                uni_ret_sum += r.ret;
                uni_trades += r.trades;
                if r.pass { uni_pass += 1; }
            }

            stats.insert((mode, label), (uni_pass, uni_total, uni_sharpe_sum, uni_ret_sum, uni_trades));
            eprintln!("  {} mode={}: {}/{} pass ({:.0}%), avg Sharpe={:.3}",
                label, mode_labels[mode], uni_pass, uni_total,
                if uni_total > 0 { uni_pass as f64 / uni_total as f64 * 100.0 } else { 0.0 },
                if uni_total > 0 { uni_sharpe_sum / uni_total as f64 } else { 0.0 });
        }
    }

    // Summary table
    eprintln!("\n=== SUMMARY ===");
    eprintln!("{:12} | {:>10} {:>10} {:>10} {:>10} {:>10}", "",
        "PassRate", "AvgSharpe", "AvgRet%", "TotalTrades", "GlobalPass");
    eprintln!("{}", "-".repeat(72));

    let mut global_pass = [0usize; 2];
    let mut global_total = [0usize; 2];
    let mut global_sharpe = [0.0_f64; 2];
    let mut global_ret = [0.0_f64; 2];
    let mut global_trades = [0usize; 2];

    for &(label, _) in UNIVERSES {
        for mode in 0..2 {
            if let Some(&(p, t, sh, r, tr)) = stats.get(&(mode, label)) {
                global_pass[mode] += p;
                global_total[mode] += t;
                global_sharpe[mode] += sh;
                global_ret[mode] += r;
                global_trades[mode] += tr;
            }
        }
    }

    for mode in 0..2 {
        let gp = global_pass[mode];
        let gt = global_total[mode];
        let gs = global_sharpe[mode];
        let gr = global_ret[mode];
        let gt2 = global_trades[mode];
        eprintln!("{:12} | {:>10.1}% {:>10.4} {:>10.1}% {:>10} {:>10}/{}",
            format!("MODE={}", mode_labels[mode]),
            if gt > 0 { gp as f64 / gt as f64 * 100.0 } else { 0.0 },
            if gt > 0 { gs / gt as f64 } else { 0.0 },
            if gt > 0 { gr / gt as f64 } else { 0.0 },
            gt2,
            gp, gt);
    }

    // Equity curves: aggregate per mode across Base5 (all windows)
    eprintln!("\nGenerating equity curves for Base5...");
    let base5_symbols: Vec<String> = UNIVERSES[0].1.iter().map(|s| s.to_string()).collect();
    let base5_total = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

    let mut equity_mode: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
    for mode in 0..2 {
        let mut agg_equity = 1.0_f64;
        for wi in 0..base5_total {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }
            let r = run_sim(&sym_data_map, &base5_symbols, test_start, test_end, mode);
            agg_equity *= r.equity_steps.last().copied().unwrap_or(1.0);
            if wi == 0 {
                // Store first window's equity curve for charting
                equity_mode[mode] = r.equity_steps;
            }
        }
        eprintln!("  Mode {} final equity (Base5, all windows): {:.4}x", mode_labels[mode], agg_equity);
    }

    // Build comprehensive equity curve: compound across all windows, step-by-step
    // Use first window's equity curve as the representative time series
    // (Different windows have different lengths; use first window as representative)
    // For multi-window equity, compound the final values
    let mut equity_csv_lines = vec!["step,max_close_equity,max_high_equity".to_string()];
    let n_steps = equity_mode[0].len().max(equity_mode[1].len());
    for i in 0..n_steps {
        let mc = equity_mode[0].get(i).copied().unwrap_or(equity_mode[0].last().copied().unwrap_or(1.0));
        let mh = equity_mode[1].get(i).copied().unwrap_or(equity_mode[1].last().copied().unwrap_or(1.0));
        equity_csv_lines.push(format!("{},{:.6},{:.6}", i, mc, mh));
    }

    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("\nWrote: {}", CSV_OUT);

    let mut ef = File::create(CSV_EQUITY)?;
    for line in &equity_csv_lines { writeln!(ef, "{}", line)?; }
    eprintln!("Wrote: {}", CSV_EQUITY);

    // Also build multi-window compound equity for all universes, both modes
    eprintln!("\nGenerating multi-window compound equity curves...");
    let mut mw_csv_lines = vec!["window,step,mode,equity".to_string()];
    for &(label, symbols) in UNIVERSES {
        let syms: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !syms.iter().all(|s| sym_data_map.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

        for wi in 0..total_windows {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            for mode in 0..2 {
                let r = run_sim(&sym_data_map, &syms, test_start, test_end, mode);
                let final_equity = r.equity_steps.last().copied().unwrap_or(1.0);
                // Store first 200 steps of each window's equity curve
                for (step, &eq) in r.equity_steps.iter().enumerate().take(200) {
                    mw_csv_lines.push(format!("{}_{},{},{},{:.6}", label, wi, step, mode_labels[mode], eq));
                }
            }
        }
    }
    let mut mwf = File::create("snapshots/turtle_entry_mode_multi_window.csv")?;
    for line in &mw_csv_lines { writeln!(mwf, "{}", line)?; }
    eprintln!("Wrote: snapshots/turtle_entry_mode_multi_window.csv");

    eprintln!("\nDone in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

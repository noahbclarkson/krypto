//! CHAND_PERIOD Fine-Sweep Hyperopt
//!
//! CHAND_PERIOD was swept step=1 from 5-50 (done 2026-04-11). This fine-sweep
//! tests step=1 from 15 to 50 (36 values) to find the true peak vs the coarse
//! sweep winner of P=28.
//!
//! Also exports per-parameter equity curves for chart generation.
//! Parameters: TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.0, CHAND_MULT=2.15, HOLD_MAX=45, CAP=3, EP=21
//! Universe: Base5 (BTC/ETH/SOL/XRP/DOGE/ADA)
//! Windows: 6 OOS windows (252-bar train / 252-bar test)

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

// Frozen params (from production)
const EP: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const CHAND_MULT: f64 = 2.15;

// CHAND_PERIOD sweep range (fine-step, 15 to 50 inclusive, step=1)
const SWEEP_START: usize = 15;
const SWEEP_END: usize = 50;

const SYMBOLS: [&str; 6] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

const CSV_OUT: &str = "snapshots/chand_period_sweep.csv";
const MD_OUT: &str = "snapshots/chand_period_sweep.md";

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

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
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
    mn * 365.0_f64.sqrt() / sd
}

fn fmt_f64(v: f64) -> String { format!("{:.6}", v) }

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
    equity_final: f64,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    chand_period: usize,
    test_start: usize,
    test_end: usize,
    collect_equity: bool,
) -> (WfResult, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(String, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.clone(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect();

        if top_syms.is_empty() {
            if collect_equity { equity_curve.push(equity); }
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, EP, bar) {
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
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, chand_period, b);
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
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            if equity > peak { peak = equity; }
                            if collect_equity { equity_curve.push(equity); }
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }

        if !entered {
            if collect_equity { equity_curve.push(equity); }
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    let result = WfResult {
        ret, sharpe, max_dd, trades: total_trades, win_rate, pass,
        equity_final: equity,
    };
    (result, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== CHAND_PERIOD Fine-Sweep (15-50, step=1) ====");
    eprintln!("Fixed: EP={}, ATR_P={}, ATR_M={}, CHAND_M={}, HM={}, CAP={}",
              EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, CHAND_MULT, HOLD_MAX, POSITION_CAP);
    eprintln!("Sweep: CHAND_PERIOD {} to {} ({} values)\n",
              SWEEP_START, SWEEP_END, SWEEP_END - SWEEP_START + 1);

    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.to_string(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in SYMBOLS {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            let close_v: Vec<f64> = df.column("close")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let high_v: Vec<f64>  = df.column("high")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let low_v: Vec<f64>   = df.column("low")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let vol_v: Vec<f64>   = df.column("volume")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            sym_data_map.insert(sym.to_string(), SymData { close: close_v, high: high_v, low: low_v, vol: vol_v });
        }
    }
    let symbols: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    eprintln!("Windows: {}", total_windows);

    // Collect window boundaries
    let windows: Vec<(usize, usize)> = (0..total_windows)
        .map(|wi| {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            (test_start, test_end)
        })
        .filter(|(s, e)| e.saturating_sub(*s) >= 5)
        .collect();

    let n_windows = windows.len();
    let n_values = SWEEP_END - SWEEP_START + 1;

    // Results: (chand_period, global_pass, avg_sharpe, avg_ret, avg_dd, total_trades, per_window_results, equity_curves)
    let mut results: Vec<(usize, usize, f64, f64, f64, usize, Vec<WfResult>, Vec<Vec<f64>>)> = Vec::new();

    for cp in SWEEP_START..=SWEEP_END {
        let mut global_pass = 0usize;
        let mut global_trades = 0usize;
        let mut sum_sharpe = 0.0_f64;
        let mut sum_ret = 0.0_f64;
        let mut sum_dd = 0.0_f64;
        let mut per_window_results: Vec<WfResult> = Vec::new();
        let mut equity_curves: Vec<Vec<f64>> = Vec::new();

        for &(test_start, test_end) in &windows {
            let (r, eq) = run_sim(&sym_data_map, &symbols, cp, test_start, test_end, true);
            sum_sharpe += r.sharpe;
            sum_ret += r.ret;
            sum_dd += r.max_dd;
            if r.pass { global_pass += 1; }
            global_trades += r.trades;
            per_window_results.push(r);
            equity_curves.push(eq);
        }

        let avg_sharpe = sum_sharpe / n_windows as f64;
        let avg_ret = sum_ret / n_windows as f64;
        let avg_dd = sum_dd / n_windows as f64;

        eprintln!(
            "CP={:02} | pass={}/{} ({:3.0}%) | sh={:6.2} | ret={:+7.1}% | DD={:5.1}% | t={}",
            cp, global_pass, n_windows,
            global_pass as f64 / n_windows as f64 * 100.0,
            avg_sharpe, avg_ret, avg_dd, global_trades
        );

        results.push((cp, global_pass, avg_sharpe, avg_ret, avg_dd, global_trades, per_window_results, equity_curves));
    }

    // Sort by pass rate desc, then Sharpe desc
    results.sort_by(|a, b| {
        let pass_a = a.1 as f64 / n_windows as f64;
        let pass_b = b.1 as f64 / n_windows as f64;
        pass_b.partial_cmp(&pass_a).unwrap()
            .then_with(|| b.2.partial_cmp(&a.2).unwrap())
    });

    // Write CSV
    let mut f = File::create(CSV_OUT)?;
    writeln!(f, "chand_period,pass_rate,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades")?;
    for &(cp, pass, sh, ret, dd, trades, _, _) in &results {
        let pass_rate = pass as f64 / n_windows as f64 * 100.0;
        writeln!(f, "{},{:.2},{:.4},{:.2},{:.2},{}", cp, pass_rate, sh, ret, dd, trades)?;
    }

    // Write MD report
    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# CHAND_PERIOD Fine-Sweep Results")?;
    writeln!(md, "")?;
    writeln!(md, "Range: {} to {} (step=1, {} values)", SWEEP_START, SWEEP_END, n_values)?;
    {
        let syms_str = SYMBOLS.join(", ");
        writeln!(md, "Universe: Base5 ({}), {} windows (252/252)", syms_str, n_windows)?;
    }
    writeln!(md, "Fixed params: EP={}, ATR_P={}, ATR_M={}, CHAND_M={}, HM={}, CAP={}",
             EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, CHAND_MULT, HOLD_MAX, POSITION_CAP)?;
    writeln!(md, "")?;
    writeln!(md, "| CHAND_P | Pass | Pass% | Avg Sharpe | Avg Return | Avg DD | Trades |")?;
    writeln!(md, "|---|---|---|---|---|---|---|")?;
    for &(cp, pass, sh, ret, dd, trades, _, _) in &results {
        let pass_pct = pass as f64 / n_windows as f64 * 100.0;
        writeln!(md, "| **{}** | {}/{} | {:3.0}% | {:.2} | {:+.1}% | {:.1}% | {} |",
                 cp, pass, n_windows, pass_pct, sh, ret, dd, trades)?;
    }
    writeln!(md, "")?;
    if let Some(first) = results.first() {
        writeln!(md, "**Winner: CHAND_PERIOD={}** (pass={}/{}, sharpe={:.2})",
                 first.0, first.1, n_windows, first.2)?;
    }

    // Export equity curves for top-5 + baseline
    let baseline_cp = 28usize;
    let winner_cp = results.first().map(|r| r.0).unwrap_or(baseline_cp);
    let top5: Vec<usize> = results.iter().take(5).map(|r| r.0).collect();

    let eq_csv_path = "snapshots/chand_period_equity_curves.csv";
    let mut ef = File::create(eq_csv_path)?;
    writeln!(ef, "chand_period,window,bar,equity")?;
    for &(cp, _, _, _, _, _, _, ref curves) in &results {
        if cp == baseline_cp || top5.contains(&cp) || cp == winner_cp {
            for (wi, eq) in curves.iter().enumerate() {
                for (bi, &e) in eq.iter().enumerate() {
                    writeln!(ef, "{},{},{},{}", cp, wi, bi, fmt_f64(e))?;
                }
            }
        }
    }
    drop(ef);

    eprintln!("\n==== TOP 5 RESULTS ====");
    for (i, r) in results.iter().take(5).enumerate() {
        let pass_pct = r.1 as f64 / n_windows as f64 * 100.0;
        let marker = if r.0 == baseline_cp { " ← BASELINE" } else if i == 0 { " ← WINNER" } else { "" };
        eprintln!("  #{:2}: CP={:02}, pass={:2}/{} ({:3.0}%), Sharpe={:.2}, ret={:+.1}%, DD={:.1}%{}",
                 i + 1, r.0, r.1, n_windows, pass_pct, r.2, r.3, r.4, marker);
    }

    eprintln!("\nRuntime: {:?}", t0.elapsed());
    eprintln!("CSV: {}", CSV_OUT);
    eprintln!("Equity curves: {}", eq_csv_path);
    eprintln!("MD: {}", MD_OUT);

    Ok(())
}
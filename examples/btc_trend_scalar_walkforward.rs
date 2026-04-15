//! BTC Trend Scalar Position Sizing — Walk-Forward
//!
//! Hypothesis: Scale Turtle+Chandelier position size based on BTC's trend state.
//! - BTC SMA21 > SMA200: BULL → scalar = 1.0 (full allocation)
//! - BTC SMA21 in chop band (|SMA21/SMA200 - 1| < CHOP_BAND): CHOP → scalar = chop_scalar
//! - BTC SMA21 < SMA200: BEAR → scalar = bear_scalar
//!
//! Sweep bear_scalar × chop_scalar over Base5 production universe (6/6 pass baseline).
//! Goal: Pass rate stays >=6/6 AND Sharpe improves vs baseline 6.73.
//!
//! CRITICALLY: BTC trend state uses data from bar - 1 (no look-ahead).
//! Position size applied to the NEXT trade only.
//!
//! 2026-04-14: Built to investigate 2026 YTD underperformance (-22.7% vs BTC +12.7%).
//! Motivated by USDT hedge overlay (2026-04-11) which showed ~30% DD reduction in bear windows.

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
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 25;
const TURTLE_ATR_MULT: f64 = 2.00;
const SMA_FAST: usize = 21;
const SMA_SLOW: usize = 200;
const CHOP_BAND: f64 = 0.03; // ±3% band around SMA200 = chop zone

// Sweep configurations: (bear_scalar, chop_scalar)
const CONFIGS: &[(f64, f64, &str)] = &[
    (1.00, 1.00, "baseline (no scaling)"),
    (0.75, 0.875, "mild bear/chop scaling"),
    (0.50, 0.75, "moderate bear/chop scaling"),
    (0.25, 0.50, "aggressive bear/chop scaling"),
    (0.10, 0.50, "extreme bear, mod chop"),
    (0.50, 1.00, "bear-only scaling (no chop adj)"),
    (0.25, 1.00, "aggressive bear-only scaling"),
    (0.25, 0.75, "aggressive bear, mild chop"),
    (0.00, 0.50, "no trading in bear, half in chop"),
];

const BASE5: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

const CSV_OUT: &str = "snapshots/btc_trend_scalar_wf.csv";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn sma(data: &[f64], period: usize, idx: usize) -> f64 {
    if idx + 1 < period { return f64::NAN; }
    let start = idx + 1 - period;
    let slice = &data[start..=idx];
    slice.iter().sum::<f64>() / period as f64
}

/// Returns the position size scalar based on BTC trend state at bar `idx-1` (no look-ahead)
fn btc_trend_scalar(btc_close: &[f64], idx: usize, bear_scalar: f64, chop_scalar: f64) -> f64 {
    if idx == 0 { return 1.0; }
    let ref_bar = idx.saturating_sub(1); // previous bar — no look-ahead
    if ref_bar + 1 < SMA_SLOW { return 1.0; } // not enough history → assume bull
    let fast = sma(btc_close, SMA_FAST, ref_bar);
    let slow = sma(btc_close, SMA_SLOW, ref_bar);
    if fast.is_nan() || slow.is_nan() || slow == 0.0 { return 1.0; }
    let ratio = fast / slow;
    if (ratio - 1.0).abs() < CHOP_BAND {
        chop_scalar // chop zone
    } else if fast > slow {
        1.0 // bull
    } else {
        bear_scalar // bear
    }
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
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).copied().unwrap_or(0.0) > max_close
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

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    pass: bool,
    bull_frac: f64,
    chop_frac: f64,
    bear_frac: f64,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    btc_close: &[f64],
    test_start: usize,
    test_end: usize,
    bear_scalar: f64,
    chop_scalar: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut daily_rets = Vec::new();
    let mut wins = 0usize;
    let mut total_trades = 0usize;

    // Track regime distribution across entry bars
    let mut bull_bars = 0usize;
    let mut chop_bars = 0usize;
    let mut bear_bars = 0usize;

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Compute scalar for this bar
        let scalar = btc_trend_scalar(btc_close, bar, bear_scalar, chop_scalar);

        // Count regime
        if scalar == 1.0 { bull_bars += 1; }
        else if scalar == bear_scalar && bear_scalar < chop_scalar { bear_bars += 1; }
        else { chop_bars += 1; }

        // No trading in bear if scalar == 0
        if scalar == 0.0 {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Rank by dollar volume
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
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
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
                            let gross_ret = (exit / entry - 1.0) * scalar; // apply scalar to position size
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

    let total_bars = bull_bars + chop_bars + bear_bars;
    let bull_frac = if total_bars > 0 { bull_bars as f64 / total_bars as f64 } else { 0.0 };
    let chop_frac = if total_bars > 0 { chop_bars as f64 / total_bars as f64 } else { 0.0 };
    let bear_frac = if total_bars > 0 { bear_bars as f64 / total_bars as f64 } else { 0.0 };

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let _ = wins;
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, pass, bull_frac, chop_frac, bear_frac }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== BTC Trend Scalar Walk-Forward (Base5 Production Universe) ====");
    eprintln!("EP={}, Chandelier({}, {}), BTC SMA({}/{}), chop_band=±{:.0}%\n",
        TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, SMA_FAST, SMA_SLOW, CHOP_BAND * 100.0);

    let loader = DataLoader::new(None, None);
    let symbols: Vec<String> = BASE5.iter().map(|s| s.to_string()).collect();
    let all_syms: Vec<String> = symbols.clone();

    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    for sym in &all_syms {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                let n = df.height();
                macro_rules! col_vec {
                    ($name:expr) => {{
                        let chunked = df.column($name)?.f64()?;
                        chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
                    }};
                }
                sym_data_map.insert(sym.clone(), SymData {
                    close: col_vec!("close"),
                    high:  col_vec!("high"),
                    low:   col_vec!("low"),
                    vol:   col_vec!("volume"),
                });
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    // Truncate all sym_data to n
    for (_, sd) in sym_data_map.iter_mut() {
        sd.close.truncate(n);
        sd.high.truncate(n);
        sd.low.truncate(n);
        sd.vol.truncate(n);
    }

    let btc_close = sym_data_map.get("BTCUSDT").map(|sd| sd.close.clone()).unwrap_or_default();

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    eprintln!("Loaded {} symbols, {} bars, {} walk-forward windows\n", sym_data_map.len(), n, total_windows);

    if total_windows == 0 {
        eprintln!("ERROR: Not enough data for walk-forward");
        return Ok(());
    }

    let mut csv_lines = vec![
        "config,bear_scalar,chop_scalar,window,return_pct,sharpe,max_dd_pct,trades,pass,bull_frac,chop_frac,bear_frac".to_string()
    ];

    // Print header
    eprintln!("{:<45} {:>6} {:>6} {:>7} {:>7} {:>7} {:>5}",
        "Config", "Pass", "AvgRet", "AvgSh", "MedSh", "WorstDD", "Trades");
    eprintln!("{}", "─".repeat(90));

    let mut summary_rows = Vec::new();

    for &(bear_scalar, chop_scalar, label) in CONFIGS {
        let mut window_results = Vec::new();
        let mut all_sharpes = Vec::new();

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            let r = run_sim(&sym_data_map, &symbols, &btc_close, test_start, test_end, bear_scalar, chop_scalar);
            csv_lines.push(format!(
                "{},{},{},{},{:.2},{:.4},{:.2},{},{},{:.3},{:.3},{:.3}",
                label, bear_scalar, chop_scalar, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.pass,
                r.bull_frac, r.chop_frac, r.bear_frac
            ));
            all_sharpes.push(r.sharpe);
            window_results.push(r);
        }

        let passed = window_results.iter().filter(|r| r.pass).count();
        let avg_ret = window_results.iter().map(|r| r.ret).sum::<f64>() / window_results.len().max(1) as f64;
        let avg_sh = window_results.iter().map(|r| r.sharpe).sum::<f64>() / window_results.len().max(1) as f64;
        let worst_dd = window_results.iter().map(|r| r.max_dd).fold(0.0_f64, f64::max);
        let total_trades: usize = window_results.iter().map(|r| r.trades).sum();
        all_sharpes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med_sh = if all_sharpes.is_empty() { 0.0 } else { all_sharpes[all_sharpes.len() / 2] };

        eprintln!("{:<45} {:>2}/{:<3} {:>+6.1}% {:>7.2} {:>7.2} {:>7.1}% {:>5}",
            label, passed, total_windows, avg_ret, avg_sh, med_sh, worst_dd, total_trades);

        // Print per-window detail for baseline and best configs
        if bear_scalar == 1.0 || (window_results.iter().filter(|r| r.pass).count() == total_windows && avg_sh > 6.0) {
            for (wi, r) in window_results.iter().enumerate() {
                eprintln!("  W{:02} | {:+8.1}% sh={:6.2} DD={:5.1}% {:3}t {}",
                    wi, r.ret, r.sharpe, r.max_dd, r.trades, if r.pass { "PASS" } else { "FAIL" });
            }
            eprintln!();
        }

        summary_rows.push((label, bear_scalar, chop_scalar, passed, total_windows, avg_ret, avg_sh, med_sh, worst_dd, total_trades));
    }

    eprintln!("\n==== BEST CONFIGS (by avg Sharpe, Base5 6-window) ====");
    let mut sorted = summary_rows.clone();
    sorted.sort_by(|a, b| b.6.partial_cmp(&a.6).unwrap()); // sort by avg_sh desc
    for (label, bear_s, chop_s, passed, total, avg_ret, avg_sh, med_sh, worst_dd, trades) in sorted.iter().take(5) {
        eprintln!("  bear={:.2} chop={:.2} | {}/{} pass | Sharpe={:.3} med={:.3} | ret={:+.1}% DD={:.1}% t={} | {}",
            bear_s, chop_s, passed, total, avg_sh, med_sh, avg_ret, worst_dd, trades, label);
    }

    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("\nCSV: {}", CSV_OUT);
    eprintln!("Runtime: {:?}", t0.elapsed());

    Ok(())
}

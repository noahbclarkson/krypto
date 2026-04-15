//! Multi-Timeframe Trend Alignment: 4h Signal + Daily Trend Filter
//!
//! Hypothesis: A 4h MACD signal filtered by daily trend alignment captures
//! trend entries earlier while the daily filter prevents counter-trend trades.
//! This is structurally different from any single-timeframe strategy tested.
//!
//! Design:
//!   - Daily: BTC SMA(21) > SMA(55) → bull trend confirmed
//!   - 4h: MACD histogram > 0 → bullish momentum
//!   - Entry: Both conditions true at 4h bar close → long next open
//!   - Exit: Chandelier trailing stop on 4h data
//!   - Universe: BTC + ETH + SOL (top 3 by volume)
//!   - Walk-forward: 252 bars / 252 bars
//!   - Fees: 0.1% taker each side
//!
//! Track C: Genuinely different — multi-timeframe information combination.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES_4H: u32 = 3000;
const CANDLES_1D: u32 = 400;
const TRAIN_BARS: usize = 252; // 4h bars for train
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

// MACD params
const MACD_FAST: usize = 12;
const MACD_SLOW: usize = 26;
const MACD_SIGNAL: usize = 9;

// Daily trend filter
const DAILY_SMA_FAST: usize = 21;
const DAILY_SMA_SLOW: usize = 55;

// Chandelier exit on 4h
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.0;
const HOLD_MAX: usize = 120; // 4h bars = 20 days max

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT"];

const CSV_OUT: &str = "snapshots/mtf_trend_alignment_wf.csv";
const MD_OUT: &str = "snapshots/mtf_trend_alignment_wf.md";

struct SymData4h {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

struct SymData1d {
    close: Vec<f64>,
    high: Vec<f64>,
}

fn ema(data: &[f64], period: usize) -> Vec<f64> {
    if data.len() < period { return vec![0.0; data.len()]; }
    let k = 2.0 / (period as f64 + 1.0);
    let mut result = vec![0.0; data.len()];
    let mut sum = 0.0;
    for i in 0..period {
        sum += data[i];
    }
    result[period - 1] = sum / period as f64;
    for i in period..data.len() {
        result[i] = data[i] * k + result[i - 1] * (1.0 - k);
    }
    result
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

fn annualised_sharpe(returns: &[f64], bars_per_year: f64) -> f64 {
    if returns.len() < 2 { return 0.0; }
    let mn: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
    let sd = (returns.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / returns.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * bars_per_year.sqrt() / sd
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

/// Map a 4h bar index to its corresponding daily bar index.
/// Each day has 6 four-hour bars (00:00, 04:00, 08:00, 12:00, 16:00, 20:00 UTC).
fn daily_idx_from_4h(idx_4h: usize) -> usize {
    idx_4h / 6
}

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_sim(
    data_4h: &SymData4h,
    data_1d: &SymData1d,
    test_start: usize,
    test_end: usize,
) -> WfResult {
    // Pre-compute MACD on full 4h data
    let fast_ema = ema(&data_4h.close, MACD_FAST);
    let slow_ema = ema(&data_4h.close, MACD_SLOW);
    let macd_line: Vec<f64> = fast_ema.iter().zip(slow_ema.iter())
        .map(|(f, s)| f - s)
        .collect();
    let signal_line = ema(&macd_line, MACD_SIGNAL);
    let histogram: Vec<f64> = macd_line.iter().zip(signal_line.iter())
        .map(|(m, s)| m - s)
        .collect();

    // Pre-compute daily SMAs
    let daily_sma_fast = ema(&data_1d.close, DAILY_SMA_FAST);
    let daily_sma_slow = ema(&data_1d.close, DAILY_SMA_SLOW);

    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut returns = Vec::new();

    let n = data_4h.close.len();
    let mut bar = test_start;

    while bar + 2 < test_end.min(n) {
        // Daily trend filter: SMA(21) > SMA(55)
        let d_idx = daily_idx_from_4h(bar);
        let daily_bull = if d_idx < daily_sma_fast.len() && d_idx < daily_sma_slow.len() {
            daily_sma_fast[d_idx] > daily_sma_slow[d_idx]
        } else {
            false
        };

        // 4h MACD signal: histogram > 0 (bullish momentum)
        let macd_bull = if bar < histogram.len() {
            histogram[bar] > 0.0
        } else {
            false
        };

        // Combined entry: daily bull trend + 4h MACD bullish
        if daily_bull && macd_bull {
            let entry_px = data_4h.close[bar];
            let entry = entry_px * (1.0 - TAKER_FEE);
            let entry_bar_next = bar + 1;

            // Chandelier trailing stop exit
            let mut highest_high = if entry_bar_next < data_4h.high.len() {
                data_4h.high[entry_bar_next]
            } else {
                entry_px
            };
            let mut exit_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));

            for b in entry_bar_next..exit_bar.min(n.saturating_sub(1)) {
                if b < data_4h.high.len() {
                    highest_high = highest_high.max(data_4h.high[b]);
                }
                let atr_val = atr_at(&data_4h.high, &data_4h.low, &data_4h.close, CHAND_PERIOD, b);
                let trail = highest_high - CHAND_MULT * atr_val;
                if data_4h.close[b] < trail {
                    exit_bar = b;
                    break;
                }
            }

            if let Some(&exit_px) = data_4h.close.get(exit_bar) {
                let exit = exit_px * (1.0 - TAKER_FEE);
                let gross_ret = exit / entry - 1.0;
                let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                wins += if gross_ret > 0.0 { 1 } else { 0 };
                total_trades += 1;
                equity *= 1.0 + gross_ret;

                let avg_4h_ret = gross_ret / bars_held as f64;
                for _ in 0..bars_held {
                    returns.push(avg_4h_ret);
                }

                if equity > peak { peak = equity; }
                equity_curve.push(equity);
                bar = exit_bar + 1;
                continue;
            }
        }

        equity_curve.push(equity);
        bar += 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&returns, 365.0 * 6.0); // 6 bars/day * 365
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

fn run_daily_baseline(
    data_1d: &SymData1d,
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let fast_ema = ema(&data_1d.close, MACD_FAST);
    let slow_ema = ema(&data_1d.close, MACD_SLOW);
    let macd_line: Vec<f64> = fast_ema.iter().zip(slow_ema.iter())
        .map(|(f, s)| f - s).collect();
    let signal_line = ema(&macd_line, MACD_SIGNAL);
    let histogram: Vec<f64> = macd_line.iter().zip(signal_line.iter())
        .map(|(m, s)| m - s).collect();

    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let n = data_1d.close.len();

    let mut bar = test_start;
    while bar + 2 < test_end.min(n) {
        if bar < histogram.len() && histogram[bar] > 0.0 {
            let entry_px = data_1d.close[bar];
            let entry = entry_px * (1.0 - TAKER_FEE);
            let entry_bar_next = bar + 1;

            let mut highest_high = if entry_bar_next < data_1d.high.len() {
                data_1d.high[entry_bar_next]
            } else {
                entry_px
            };
            let mut exit_bar = (entry_bar_next + 30).min(n.saturating_sub(1)); // max 30 days hold

            for b in entry_bar_next..exit_bar.min(n.saturating_sub(1)) {
                if b < data_1d.high.len() {
                    highest_high = highest_high.max(data_1d.high[b]);
                }
                let atr_val = atr_at(&data_1d.high, &[], &data_1d.close, CHAND_PERIOD, b);
                let trail = highest_high - CHAND_MULT * atr_val;
                if data_1d.close[b] < trail {
                    exit_bar = b;
                    break;
                }
            }

            if let Some(&exit_px) = data_1d.close.get(exit_bar) {
                let exit = exit_px * (1.0 - TAKER_FEE);
                let gross_ret = exit / entry - 1.0;
                let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                wins += if gross_ret > 0.0 { 1 } else { 0 };
                total_trades += 1;
                equity *= 1.0 + gross_ret;

                let avg_daily = gross_ret / bars_held as f64;
                for _ in 0..bars_held { daily_rets.push(avg_daily); }

                if equity > peak { peak = equity; }
                equity_curve.push(equity);
                bar = exit_bar + 1;
                continue;
            }
        }
        equity_curve.push(equity);
        bar += 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets, 365.0);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Multi-Timeframe Trend Alignment ====");
    eprintln!("4h MACD({}, {}, {}) + Daily SMA({}, {}) filter + Chandelier({}, {})",
        MACD_FAST, MACD_SLOW, MACD_SIGNAL, DAILY_SMA_FAST, DAILY_SMA_SLOW,
        CHAND_PERIOD, CHAND_MULT);

    let loader = DataLoader::new(None, None);

    let mut data_4h: HashMap<String, SymData4h> = HashMap::new();
    let mut data_1d: HashMap<String, SymData1d> = HashMap::new();

    for &sym in SYMBOLS {
        // Load 4h data
        match loader.fetch_with_cache(sym, "4h", CANDLES_4H).await {
            Ok(df) => {
                let n = df.height();
                macro_rules! col_v {
                    ($name:expr) => {{
                        df.column($name).unwrap().f64().unwrap().into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
                    }};
                }
                data_4h.insert(sym.to_string(), SymData4h {
                    close: col_v!("close"), high: col_v!("high"),
                    low: col_v!("low"), vol: col_v!("volume"),
                });
                eprintln!("  {} 4h: {} bars", sym, n);
            }
            Err(e) => eprintln!("  {} 4h: FAILED ({})", sym, e),
        }

        // Load 1d data
        match loader.fetch_with_cache(sym, "1d", CANDLES_1D).await {
            Ok(df) => {
                let n = df.height();
                let close: Vec<f64> = df.column("close").unwrap().f64().unwrap().into_iter().filter_map(|x| x).take(n).collect();
                let high: Vec<f64> = df.column("high").unwrap().f64().unwrap().into_iter().filter_map(|x| x).take(n).collect();
                data_1d.insert(sym.to_string(), SymData1d { close, high });
                eprintln!("  {} 1d: {} bars", sym, data_1d.get(sym).map(|d| d.close.len()).unwrap_or(0));
            }
            Err(e) => eprintln!("  {} 1d: FAILED ({})", sym, e),
        }
    }

    let mut csv_lines = vec![
        "symbol,strategy,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()
    ];

    for &sym in SYMBOLS {
        let d4h = match data_4h.get(sym) {
            Some(d) => d,
            None => { eprintln!("  {} SKIPPED (no 4h data)", sym); continue; }
        };
        let d1d = match data_1d.get(sym) {
            Some(d) => d,
            None => { eprintln!("  {} SKIPPED (no 1d data)", sym); continue; }
        };

        let n_4h = d4h.close.len();
        let n_1d = d1d.close.len();

        eprintln!("\n==== {} ====", sym);
        eprintln!("4h bars: {} | 1d bars: {}", n_4h, n_1d);

        // Walk-forward windows on 4h data
        let total_windows = n_4h.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

        for wi in 0..total_windows {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n_4h);

            // Map to daily indices for the baseline
            let d_test_start = daily_idx_from_4h(test_start);
            let d_test_end = (daily_idx_from_4h(test_end)).min(n_1d);

            // Multi-timeframe
            let mtf = run_sim(d4h, d1d, test_start, test_end);

            // Daily baseline
            let base = run_daily_baseline(d1d, d_test_start, d_test_end);

            let mtf_tag = if mtf.pass { "PASS" } else { "FAIL" };
            let base_tag = if base.pass { "PASS" } else { "FAIL" };

            eprintln!("  W{:02} | MTF {:+8.1}% DD={:5.1}% {:3}t {} | 1D {:+8.1}% DD={:5.1}% {:3}t {}",
                wi, mtf.ret, mtf.max_dd, mtf.trades, mtf_tag,
                base.ret, base.max_dd, base.trades, base_tag);

            csv_lines.push(format!("{},{},{:.2},{:.4},{:.2},{},{:.2},{},{}",
                sym, "MTF_4h_1d", wi, mtf.ret, mtf.sharpe, mtf.max_dd, mtf.trades, mtf.win_rate, if mtf.pass { "true" } else { "false" }));
            csv_lines.push(format!("{},{},{:.2},{:.4},{:.2},{},{:.2},{},{}",
                sym, "Daily_MACD", wi, base.ret, base.sharpe, base.max_dd, base.trades, base.win_rate, if base.pass { "true" } else { "false" }));
        }
    }

    // Write CSV
    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }

    // Summary
    eprintln!("\n===== SUMMARY =====");
    eprintln!("CSV: {}", CSV_OUT);
    eprintln!("Runtime: {:?}", t0.elapsed());

    Ok(())
}

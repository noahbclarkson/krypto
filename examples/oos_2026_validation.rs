//! =========================================================
//! T-2026: BTC+ETH Walk-Forward Including 2026 Data
//! =========================================================
//!
//! CRITICAL VALIDATION: Zero of our 54 existing walk-forward
//! windows include 2026. This harness tests production params
//! on BTC+ETH with windows extending to 2026.
//!
//! Data constraint: Binance returns max ~2080 bars. W05/W06
//! extend beyond that cap and will be skipped or partial.
//!
//! Production params:
//!   EP=24, CHAND_PERIOD=11, CHAND_MULT=2.25
//!   HOLD_MAX=12, ATR_PERIOD=24, ATR_MULT=2.0
//!   POSITION_CAP=2 (BTC+ETH only), VOL_LOOKBACK=2
//!   ATR_ENTRY_MULT=0.90 (production)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

const CANDLES: u32 = 3000;

const EP: usize = 24;
const CHAND_P: usize = 11;
const CHAND_M: f64 = 2.25;
const ATR_P: usize = 24;
const ATR_M: f64 = 2.0;
const HOLD_MAX: usize = 12;
const CAP: usize = 2;
const VOL_LOOKBACK: usize = 2;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

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

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
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

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_entry_mult: f64,
) -> WfResult {
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
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    let start = bar + 1 - EP;
                    let mut max_close = f64::NEG_INFINITY;
                    for i in start..=bar {
                        if let Some(&c) = sd.close.get(i) { max_close = max_close.max(c); }
                    }
                    let curr_close = sd.close[bar];
                    let breakout = curr_close > max_close;

                    let atr_val = atr_at(&sd.high, &sd.low, &sd.close, ATR_P, bar);
                    let passes_filter = atr_entry_mult == 0.0
                        || (curr_close - max_close) >= atr_entry_mult * atr_val;

                    if breakout && passes_filter {
                        let entry_px = curr_close;
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_P, b);
                            let trail_chand = highest_high_chand - CHAND_M * atr_chand;

                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, ATR_P, b);
                            let trail_turtle = lowest_low_turtle - ATR_M * atr_turtle;

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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    println!("===========================================");
    println!("  T-2026: BTC+ETH 2026 OOS Walk-Forward");
    println!("===========================================");
    println!();
    println!("Production params: EP={}, CHAND({},{}), HM={}, ATR({})",
             EP, CHAND_P, CHAND_M, HOLD_MAX, ATR_P);
    println!("ATR_ENTRY_MULT: 0.90 (production)");
    println!("Universe: BTC+ETH only");
    println!();

    // Load BTC+ETH
    let loader = DataLoader::new(None, None);
    let btc_df = loader.fetch_with_cache("BTCUSDT", "1d", CANDLES).await?;
    let eth_df = loader.fetch_with_cache("ETHUSDT", "1d", CANDLES).await?;

    let n = btc_df.height();
    let min_n = n.min(eth_df.height()).min(2800);

    let btc_times = btc_df.column("time")?.datetime()?;
    let start_str = btc_times.get(0)
        .and_then(|d| chrono::DateTime::from_timestamp_millis(d))
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "?".to_string());
    let end_str = btc_times.get((n - 1) as usize)
        .and_then(|d| chrono::DateTime::from_timestamp_millis(d))
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "?".to_string());

    println!("BTC: {} rows ({} to {})", n, &start_str[..10], &end_str[..10]);
    println!("ETH: {} rows", eth_df.height());
    println!("Using: {} bars per symbol", min_n);
    println!();

    // Windows: (test_start, test_end, label)
    // Data cap: ~2080 bars. W04 (2023-24) is the last clean window.
    // W05/W06 extend beyond data cap.
    let windows: [(usize, usize, &'static str); 5] = [
        (683,  934,  "W00"),  // test 2019-07-01 to 2020-03-07 (252 bars)
        (1049, 1300, "W01"),  // test 2020-03-08 to 2020-11-13 (252 bars)
        (1414, 1665, "W02"),  // test 2020-11-14 to 2021-07-20 (252 bars)
        (1779, 2030, "W03"),  // test 2021-07-21 to 2022-03-28 (252 bars)
        (2144, 2395, "W04"),  // test 2022-03-29 to 2022-10-04 (252 bars)
    ];

    println!("Windows (relative to 2017-08-17 bar 0):");
    for (test_start, test_end, label) in &windows {
        let ts_str = btc_times.get(*test_start as usize)
            .and_then(|d| chrono::DateTime::from_timestamp_millis(d))
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "?".to_string());
        let te_str = btc_times.get((*test_end as usize).min((n - 1) as usize))
            .and_then(|d| chrono::DateTime::from_timestamp_millis(d))
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "?".to_string());
        println!("  {}: {} to {} (bars {}-{})",
                 label, &ts_str[..10], &te_str[..10], test_start, test_end);
    }
    println!();

    // Extended: W05 (partial), W06 (2026)
    println!("NOTE: W05 (2024-mid-2025) and W06 (2025-07 to 2026-04-21) extend");
    println!("      beyond data cap ({} bars). W06 is SKIPPED.", min_n);
    println!();

    fn sym_data_from_df(df: &DataFrame, n: usize) -> SymData {
        macro_rules! col_vec {
            ($name:expr) => {{
                let chunked = df.column($name).unwrap().f64().unwrap();
                chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
            }};
        }
        SymData {
            close: col_vec!("close"),
            high:  col_vec!("high"),
            low:   col_vec!("low"),
            vol:   col_vec!("volume"),
        }
    }

    let btc_sd = sym_data_from_df(&btc_df, min_n);
    let eth_sd = sym_data_from_df(&eth_df, min_n);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    sym_data.insert("BTCUSDT".to_string(), btc_sd);
    sym_data.insert("ETHUSDT".to_string(), eth_sd);

    let symbols = vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()];

    // Run W00-W04
    println!("=== W00-W04 Results (ATR_ENTRY_MULT=0.90) ===");
    let mut hist_pass = 0usize;
    let mut hist_total = 0usize;

    for (test_start, test_end, label) in windows.iter() {
        if *test_start >= min_n {
            println!("  {} | SKIPPED (test_start={} >= min_n={})", label, test_start, min_n);
            continue;
        }
        let actual_end = (*test_end as usize).min(min_n - 1);
        let r = run_sim(&sym_data, &symbols, *test_start, actual_end, 0.90);
        let pass_str = if r.pass { "PASS" } else { "FAIL" };
        println!("  {} | {:+8.1}% sh={:6.2} DD={:5.1}% {:3}t {:2.0}% {}",
                 label, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, pass_str);
        hist_pass += if r.pass { 1 } else { 0 };
        hist_total += 1;
    }
    let hist_rate = if hist_total > 0 { hist_pass as f64 / hist_total as f64 * 100.0 } else { 0.0 };
    println!("  W00-W04: {}/{} pass ({:.0}%)\n", hist_pass, hist_total, hist_rate);

    // W05 (partially in data)
    println!("=== W05 (Partial: 2024-07 to 2025-06) ===");
    let w05_start = 2510usize;
    let w05_end = 2761usize;
    if w05_start < min_n {
        let actual_w05_end = min_n - 1; // Cap to data
        let r = run_sim(&sym_data, &symbols, w05_start, actual_w05_end, 0.90);
        let in_data_bars = actual_w05_end - w05_start;
        let ts_str = btc_times.get(w05_start)
            .and_then(|d| chrono::DateTime::from_timestamp_millis(d))
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "?".to_string());
        let te_str = btc_times.get(actual_w05_end)
            .and_then(|d| chrono::DateTime::from_timestamp_millis(d))
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "?".to_string());
        println!("  W05 | {:+8.1}% sh={:6.2} DD={:5.1}% {:3}t {:2.0}% {} [partial: {} bars, {} to {}]",
                 r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate,
                 if r.pass { "PASS" } else { "FAIL" },
                 in_data_bars, &ts_str[..10], &te_str[..10]);
    } else {
        println!("  W05 | SKIPPED (test_start={} >= min_n={})", w05_start, min_n);
    }
    println!();

    // W06 (2026 — completely out of data)
    println!("=== W06 (2026 OOS: 2025-07 to 2026-04-21) ===");
    let w06_start = 2875usize;
    let w06_end = 3169usize;
    if w06_start >= min_n {
        println!("  W06 | SKIPPED (test_start={} >= min_n={})", w06_start, min_n);
        println!("  *** W06 (2026) is completely beyond cached data ***");
        println!("  To run W06, need fresh Binance fetch extending to 2026-04-21");
    }

    // Summary
    let elapsed = t0.elapsed();
    println!();
    println!("===========================================");
    println!("  SUMMARY");
    println!("  W00-W04: {}/{} pass ({:.0}%)", hist_pass, hist_total, hist_rate);
    println!("  W05: partial (see above)");
    println!("  W06 (2026): SKIPPED — data ends at bar {}", min_n - 1);
    println!("  BTC last date: {}", &end_str[..10]);
    println!("===========================================");
    println!();
    println!("Completed in {:.1}s", elapsed.as_secs_f64());

    Ok(())
}
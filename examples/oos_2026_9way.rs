//! =========================================================
//! T-2026: Walk-Forward Including 2026 OOS Data
//! =========================================================
//!
//! CRITICAL: All 54 walk-forward windows end before 2026.
//! This adds 2026 as a proper walk-forward window using the same
//! production frozen params validated on all prior windows.
//!
//! Full 9-universe × 7 windows walk-forward:
//!   W00: train ends 2019-06-30  → test 2019-07-01 to 2020-06-30
//!   W01: train ends 2020-06-30  → test 2020-07-01 to 2021-06-30
//!   W02: train ends 2021-06-30  → test 2021-07-01 to 2022-06-30
//!   W03: train ends 2022-06-30  → test 2022-07-01 to 2023-06-30
//!   W04: train ends 2023-06-30  → test 2023-07-01 to 2024-06-30
//!   W05: train ends 2024-06-30  → test 2024-07-01 to 2025-06-30
//!   W06: train ends 2025-06-30  → test 2025-07-01 to 2026-04-21 [NEW]
//!
//! Production params (frozen, never re-tuned on W06):
//!   EP=24, CHAND_PERIOD=11, CHAND_MULT=2.25
//!   HOLD_MAX=12, ATR_PERIOD=24, ATR_MULT=2.0
//!   POSITION_CAP=3, VOL_LOOKBACK=2
//!   ATR_ENTRY_MULT=0.90 (production)
//!
//! Symbols: Base5 (BTC, ETH, SOL, XRP, DOGE, ADA)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 4000;

// Full historical date range for extended data fetch
const HIST_START_MS: u64 = 1_503_014_400_000; // 2017-08-17 00:00 UTC
const HIST_END_MS: u64 = 1_745_366_400_000;   // 2026-04-21 00:00 UTC

// Production params
const EP: usize = 24;
const CHAND_P: usize = 11;
const CHAND_M: f64 = 2.25;
const ATR_P: usize = 24;
const ATR_M: f64 = 2.0;
const HOLD_MAX: usize = 12;
const CAP: usize = 3;
const VOL_LOOKBACK: usize = 2;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

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
    let mut peak = equity;
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
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    let start = bar + 1 - EP;
                    let mut max_close = f64::NEG_INFINITY;
                    for i in start..bar {
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

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

// Find bar index closest to a given date (for test_end of each window)
fn find_bar_for_date(df: &DataFrame, target_year: i32, target_month: u32, target_day: u32) -> usize {
    use chrono::Datelike;
    let times = df.column("time").unwrap().datetime().unwrap();
    let mut found = 0usize;
    let mut best_dist = i64::MAX;
    for (i, ms) in times.into_iter().enumerate() {
        if let Some(ts) = ms {
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(ts) {
                let dy = (dt.year() - target_year).abs();
                let dm = (dt.month() as i32 - target_month as i32).abs();
                let dd = (dt.day() as i32 - target_day as i32).abs();
                let dist = dy as i64 * 10000 + dm as i64 * 100 + dd as i64;
                if dist < best_dist {
                    best_dist = dist;
                    found = i;
                }
            }
        }
    }
    found
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    println!("===========================================");
    println!("  T-2026: Full Walk-Forward Including 2026");
    println!("===========================================");
    println!();
    println!("Production params: EP={}, CHAND({},{}), HM={}, ATR({})",
             EP, CHAND_P, CHAND_M, HOLD_MAX, ATR_P);
    println!("ATR_ENTRY_MULT=0.90");
    println!("Position cap: {}, Fee: {}bps", CAP, (TAKER_FEE * 10000.0) as i32);
    println!();

    // Load data
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        // Use fetch_data_in_range to get full 2017-2026 history
        // (fetch_with_cache uses limited backward fetch, capping at ~2080 bars)
        match loader.fetch_data_in_range(sym.as_str(), "1d", HIST_START_MS, HIST_END_MS).await {
            Ok(df) => {
                // Update cache for future runs
                let _ = loader.save_to_cache(sym.as_str(), "1d", &df);
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} fetch failed: {}", sym, e); }
        }
    }

    let n = min_len;
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

    // Walk-forward windows: standard 6 windows + W06 (2026)
    // Windows:
    //   W00: test 2019-07-01 to 2020-06-30 (bar ~513 to ~878)
    //   W01: test 2020-07-01 to 2021-06-30 (bar ~878 to ~1243)
    //   W02: test 2021-07-01 to 2022-06-30 (bar ~1243 to ~1608)
    //   W03: test 2022-07-01 to 2023-06-30 (bar ~1608 to ~1973)
    //   W04: test 2023-07-01 to 2024-06-30 (bar ~1973 to ~2338)
    //   W05: test 2024-07-01 to 2025-06-30 (bar ~2338 to ~2703)
    //   W06: test 2025-07-01 to 2026-04-21 (bar ~2703 to ~3169)
    //
    // Compute using BTC timestamps
    let btc_df = raw_cache.get("BTCUSDT").unwrap();
    let train_months = 18; // 18-month train, 12-month test
    let test_bars = 252;

    let mut all_records = Vec::new();
    let mut global_pass = 0usize;
    let mut global_total = 0usize;

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing data)", label);
            continue;
        }

        // Compute number of 18-month train windows that fit before 2026-04-21
        // We use the same logic as turtle_chandelier_walkforward: fixed test windows
        // But we also add W06 (2026)

        // Standard windows (6 windows: 252-bar test, with some train)
        let total_windows = n.saturating_sub(252 + 252) / 252;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (not enough data)", label);
            continue;
        }

        let mut universe_pass = 0usize;
        let mut window_results = Vec::new();

        // Standard windows (W00-W05)
        for w in 0..6 {
            let train_end = n - 252 * (6 - w);
            let test_start = train_end;
            let test_end = (test_start + 252).min(n);

            if test_end.saturating_sub(test_start) < 5 { continue; }

            let r = run_sim(&sym_data_map, &symbols, test_start, test_end, 0.90);
            let pass_str = if r.pass { "PASS" } else { "FAIL" };
            eprintln!("{:>20} W{:01} | {:+8.1}% sh={:6.2} DD={:5.1}% {:3}t {:2.0}% {}",
                     label, w, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, pass_str);

            if r.pass { universe_pass += 1; }
            window_results.push(r);
        }

        // W06: 2026 test — COMPUTE dynamically from data
        // Find bar indices for 2025-07-01 and 2026-04-21 using BTC timestamps
        let btc_times_raw = raw_cache.get("BTCUSDT").unwrap().column("time").unwrap().datetime().unwrap();
        let target_start = chrono::NaiveDate::from_ymd_opt(2025, 7, 1).unwrap()
            .and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as i64;
        let target_end = chrono::NaiveDate::from_ymd_opt(2026, 4, 21).unwrap()
            .and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as i64;
        let mut w06_test_start = 0usize;
        let mut w06_test_end = 0usize;
        for (i, ms) in btc_times_raw.into_iter().enumerate() {
            if let Some(ts) = ms {
                if ts as i64 >= target_start && w06_test_start == 0 { w06_test_start = i; }
                if ts as i64 >= target_end && w06_test_end == 0 { w06_test_end = i; }
            }
        }
        // Fallback if dates not in data: use last 252 bars
        if w06_test_start == 0 { w06_test_start = n.saturating_sub(504); }
        if w06_test_end == 0 { w06_test_end = n - 1; }
        // Also compute train start
        let w06_train_end = w06_test_start;
        let _w06_train_start = w06_train_end.saturating_sub(252).max(EP * 3);

        // Check if ALL symbols have data for the FULL W06 test period
        let shortest_sym_len = symbols.iter()
            .filter_map(|s| sym_data_map.get(s).map(|sd| sd.close.len()))
            .min()
            .unwrap_or(n);

        let all_have_data = w06_test_end < shortest_sym_len;

        if all_have_data && w06_test_start < n {
            let actual_end = w06_test_end.min(shortest_sym_len - 1);
            let r = run_sim(&sym_data_map, &symbols, w06_test_start, actual_end, 0.90);
            let pass_str = if r.pass { "PASS" } else { "FAIL" };
            let test_bars = actual_end.saturating_sub(w06_test_start);
            eprintln!("{:>20} W06 | {:+8.1}% sh={:6.2} DD={:5.1}% {:3}t {:2.0}% {} [2026 TEST, {} bars]",
                     label, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, pass_str, test_bars);

            if r.pass { universe_pass += 1; }
            window_results.push(r);
        } else {
            eprintln!("{:>20} W06 | SKIPPED (shortest={} bars, test_end={})",
                     label, shortest_sym_len, w06_test_end);
        }

        let total_w = window_results.len();
        let pass_rate = if total_w > 0 { universe_pass as f64 / total_w as f64 } else { 0.0 };
        eprintln!("{:>20}      avg {:+8.1}% {:3}/{:3} pass ({:.0}%)\n",
                 label,
                 window_results.iter().map(|r| r.ret).sum::<f64>() / total_w as f64,
                 universe_pass, total_w, pass_rate);

        for (w, r) in window_results.iter().enumerate() {
            global_pass += if r.pass { 1 } else { 0 };
            global_total += 1;
            all_records.push((label.to_string(), w, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass));
        }
    }

    eprintln!();
    eprintln!("===========================================");
    eprintln!("GLOBAL SUMMARY: {}/{} pass ({:.0}%)", global_pass, global_total,
              if global_total > 0 { global_pass as f64 / global_total as f64 * 100.0 } else { 0.0 });
    eprintln!("===========================================");

    // Write CSV
    let csv_path = "snapshots/oos_2026_9way_wf.csv";
    let mut f = File::create(csv_path)?;
    writeln!(f, "universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;
    for (label, w, ret, sh, dd, trades, wr, pass) in &all_records {
        writeln!(f, "{},W{},{:.2},{:.2},{:.2},{},{:.1},{}", label, w, ret, sh, dd, trades, wr, pass)?;
    }
    eprintln!("\nCSV written: {}", csv_path);

    let elapsed = t0.elapsed();
    eprintln!("Completed in {:.1}s", elapsed.as_secs_f64());

    Ok(())
}

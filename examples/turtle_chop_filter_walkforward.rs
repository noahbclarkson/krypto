//! Turtle Chop Filter Walk-Forward — ATR Regime Gate
//!
//! Mission: Test whether adding an ATR-regime entry gate improves Turtle+Chandelier.
//! Hypothesis: Enter Turtle only when ATR(chop_period) > median_ATR(chop_period, 252).
//!
//! Frozen params (all validated via prior hyperopt):
//!   EP=21, CHAND(28,2.0), TURTLE_ATR(25,2.0), CAP=3, HM=45
//!
//! Sweep: chop_period ∈ {5, 7, 10, 14, 20, 21, 28, 30, 40, 50, 63, 100}
//!   Median lookback = 252 (fixed reference)
//!   Baseline: no chop filter (chop_period = none)
//!
//! Output:
//!   snapshots/chop_filter_sweep.csv  — all configs × all universes
//!   snapshots/chop_filter_wf.csv   — last run config only
//!
//! Usage:
//!   cargo run --example turtle_chop_filter_walkforward --profile sweep -- batch
//!   cargo run --example turtle_chop_filter_walkforward --profile sweep -- none
//!   cargo run --example turtle_chop_filter_walkforward --profile sweep -- atr_14

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::env;
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
const MEDIAN_LOOKBACK: usize = 252;

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

const CHOP_VALUES: &[usize] = &[5, 7, 10, 14, 20, 21, 28, 30, 40, 50, 63, 100];

const CSV_OUT:      &str = "snapshots/chop_filter_wf.csv";
const COMBINED_CSV: &str = "snapshots/chop_filter_sweep.csv";

macro_rules! col_vec {
    ($df:expr, $name:expr, $n:expr) => {{
        let chunked = $df.column($name)?.f64()?;
        chunked.into_iter().filter_map(|x| x).take($n).collect::<Vec<_>>()
    }};
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

fn median_atr(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize, lookback: usize) -> f64 {
    if idx < lookback { return 0.0; }
    let mut vals = Vec::with_capacity(lookback);
    for i in (idx + 1 - lookback)..=idx {
        let v = atr_at(high, low, close, period, i);
        if v > 0.0 { vals.push(v); }
    }
    if vals.is_empty() { return 0.0; }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = vals.len() / 2;
    if vals.len() % 2 == 0 {
        (vals[mid - 1] + vals[mid]) / 2.0
    } else {
        vals[mid]
    }
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
    let mn = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from_rets(daily_rets: &[f64]) -> f64 {
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut equity = 1.0_f64;
    for &r in daily_rets {
        equity *= 1.0 + r;
        peak = peak.max(equity);
        let dd = (peak - equity) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

#[derive(Clone)]
struct OpenTrade {
    sym: String,
    entry_bar: usize,
    shares: f64,
    highest_high: f64,
}

fn run_wf_window(
    sym_data: &HashMap<String, SymData>,
    symbols: &[&str],
    test_start: usize,
    test_end: usize,
    chop_period: Option<usize>,
) -> Option<(f64, f64, f64, usize, Vec<f64>)> {
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut open_trades: Vec<OpenTrade> = Vec::new();
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    for bar in test_start..test_end {
        // Ranking by dollar volume
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for &sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym, if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let ranked: Vec<&str> = scores.into_iter().map(|(s, _)| s).take(POSITION_CAP).collect();

        // Exit check
        let mut still_open: Vec<OpenTrade> = Vec::new();
        for mut trade in open_trades {
            let sd = match sym_data.get(&trade.sym) {
                Some(s) => s,
                None => { still_open.push(trade); continue; }
            };
            if bar >= sd.close.len() { still_open.push(trade); continue; }

            let bars_held = bar.saturating_sub(trade.entry_bar);

            // Update highest high
            for b in (trade.entry_bar + 1)..=bar {
                if b < sd.high.len() {
                    trade.highest_high = trade.highest_high.max(sd.high[b]);
                }
            }

            let atr_c = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, bar);
            let atr_t = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar);
            let trail_c = trade.highest_high - CHAND_MULT * atr_c;
            let trail_t = trade.highest_high - TURTLE_ATR_MULT * atr_t;
            let curr_close = sd.close[bar];

            let hit_stop = bars_held >= 1 && (curr_close < trail_c || curr_close < trail_t);
            let hit_max_hold = bars_held >= HOLD_MAX;

            if hit_stop || hit_max_hold {
                let exit_px = curr_close * (1.0 - TAKER_FEE);
                let entry_px = sd.close[trade.entry_bar] * (1.0 + TAKER_FEE);
                equity += trade.shares * (exit_px - entry_px);
                peak = peak.max(equity);
                total_trades += 1;
            } else {
                still_open.push(trade);
            }
        }
        open_trades = still_open;

        // Entry check
        for &sym in &ranked {
            if open_trades.len() >= POSITION_CAP { break; }
            if open_trades.iter().any(|t| t.sym == sym) { continue; }

            let sd = match sym_data.get(sym) {
                Some(s) => s,
                None => continue,
            };
            if bar < TURTLE_ENTRY + 1 || bar >= sd.close.len() { continue; }

            // CHOP FILTER: skip entry when ATR <= median ATR
            if let Some(cp) = chop_period {
                let curr_atr = atr_at(&sd.high, &sd.low, &sd.close, cp, bar);
                let med_atr   = median_atr(&sd.high, &sd.low, &sd.close, cp, bar, MEDIAN_LOOKBACK);
                if med_atr <= 0.0 || curr_atr <= med_atr {
                    continue;
                }
            }

            if turtle_signal(&sd.close, &sd.high, TURTLE_ENTRY, bar) {
                let entry_px = sd.close[bar] * (1.0 + TAKER_FEE);
                let shares = equity / entry_px;
                let prev_bar = bar.saturating_sub(1);
                let highest_high = sd.high.get(prev_bar).copied().unwrap_or(entry_px);
                open_trades.push(OpenTrade {
                    sym: sym.to_string(),
                    entry_bar: bar,
                    shares,
                    highest_high,
                });
            }
        }

        // Mark-to-market
        let mut port_equity = equity;
        for trade in &open_trades {
            let sd = match sym_data.get(&trade.sym) {
                Some(s) => s,
                None => continue,
            };
            if bar >= sd.close.len() { continue; }
            let curr_close = sd.close[bar];
            let entry_px = sd.close[trade.entry_bar] * (1.0 + TAKER_FEE);
            port_equity += trade.shares * (curr_close - entry_px);
        }
        let prev_equity = equity;
        equity = port_equity;
        peak = peak.max(equity);
        if prev_equity > 0.0 {
            let daily_ret = (equity / prev_equity - 1.0).max(-1.0);
            daily_rets.push(daily_ret);
        }
    }

    // Close open trades at last bar
    for trade in open_trades {
        let sd = match sym_data.get(&trade.sym) {
            Some(s) => s,
            None => continue,
        };
        if test_end - 1 < sd.close.len() {
            let exit_px  = sd.close[test_end - 1] * (1.0 - TAKER_FEE);
            let entry_px = sd.close[trade.entry_bar] * (1.0 + TAKER_FEE);
            equity += trade.shares * (exit_px - entry_px);
        }
    }

    if total_trades < MIN_TRADES { return None; }
    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from_rets(&daily_rets);
    Some((ret, sharpe, max_dd, total_trades, daily_rets))
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let args: Vec<String> = env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("batch");

    let loader = DataLoader::new(None, None);

    // Collect all unique symbols across universes
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    // Load data
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

    // Build SymData map
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in all_syms.iter() {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!(df, "close", n_min),
                high:  col_vec!(df, "high",  n_min),
                low:   col_vec!(df, "low",   n_min),
                vol:   col_vec!(df, "volume", n_min),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Determine configs to run
    let configs: Vec<(String, Option<usize>)> = match mode {
        "batch" => {
            let mut c = vec![("baseline".to_string(), None)];
            for &v in CHOP_VALUES { c.push((format!("atr_{}", v), Some(v))); }
            c
        }
        "none" => vec![("baseline".to_string(), None)],
        other if other.starts_with("atr_") => {
            let val: usize = other[4..].parse().unwrap_or(14);
            vec![(other.to_string(), Some(val))]
        }
        _ => {
            eprintln!("Usage: turtle_chop_filter_walkforward [batch|none|atr_N]");
            return Ok(());
        }
    };

    let mut all_rows: Vec<String> = vec!["universe,window,chop_mode,chop_atr,ret,sharpe,max_dd,trades".to_string()];
    let mut csv_out = File::create(CSV_OUT)?;
    writeln!(csv_out, "{}", all_rows[0])?;

    for (label, chop_period) in &configs {
        eprintln!("=== Config: {} (chop_period={:?}) ===", label, chop_period);

        let mut total_pass = 0usize;
        let mut total_windows = 0usize;
        let mut all_sharpes = Vec::new();

        for &(uname, symbols) in UNIVERSES {
            let symbols_owned: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let sym_refs: Vec<&str> = symbols_owned.iter().map(|s| s.as_str()).collect();

            let all_loaded = symbols_owned.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded {
                eprintln!("  {:>20} SKIPPED (missing data)", uname);
                continue;
            }

            let total_win = n.saturating_sub(TRAIN_BARS) / TEST_BARS;
            if total_win == 0 {
                eprintln!("  {:>20} SKIPPED (not enough data)", uname);
                continue;
            }
            let n_windows = total_win.min(6);

            eprintln!("  {:>20}: {} windows", uname, n_windows);

            for wi in 0..n_windows {
                let test_start = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);

                if let Some((ret, sharpe, max_dd, trades, _)) =
                    run_wf_window(&sym_data_map, &sym_refs, test_start, test_end, *chop_period)
                {
                    let pass = if sharpe > 0.0 { 1 } else { 0 };
                    total_pass += pass;
                    total_windows += 1;
                    all_sharpes.push(sharpe);

                    let row = format!(
                        "{},W{},{},{},{:.2},{:.4},{:.2},{}",
                        uname, wi, label, chop_period.unwrap_or(0),
                        ret, sharpe, max_dd, trades
                    );
                    all_rows.push(row.clone());
                    writeln!(csv_out, "{}", row)?;
                }
            }
        }

        let avg_sharpe = if all_sharpes.is_empty() {
            0.0_f64
        } else {
            all_sharpes.iter().sum::<f64>() / all_sharpes.len() as f64
        };
        let pass_rate = if total_windows == 0 {
            0.0_f64
        } else {
            total_pass as f64 / total_windows as f64 * 100.0
        };
        eprintln!(
            "  --> {}: pass {}/{} ({:.1})%  avg Sharpe {:.4}\n",
            label, total_pass, total_windows, pass_rate, avg_sharpe
        );
    }

    // Write combined CSV in batch mode
    if mode == "batch" {
        let mut comb = File::create(COMBINED_CSV)?;
        for row in &all_rows {
            writeln!(comb, "{}", row)?;
        }
        eprintln!("Wrote combined: {} ({} rows)", COMBINED_CSV, all_rows.len());
    }

    eprintln!("Runtime: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

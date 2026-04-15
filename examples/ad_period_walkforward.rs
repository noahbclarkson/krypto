//! A/D Period Walk-Forward — Sweep AD_PERIOD ∈ {3..30}
//!
//! PURPOSE: Find the optimal A/D momentum lookback period via walk-forward validation.
//!
//! Signal: price rate-of-change (momentum) ranked cross-sectionally.
//! "Accumulation" = symbol with strongest recent price momentum vs universe mean.
//! This is simpler and more robust than A/D EMA which requires careful warmup.
//!
//! Run:
//!   cargo run --example ad_period_walkforward --profile sweep -- batch
//!
//! Output: snapshots/ad_period_sweep.csv

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const WARMUP: usize = 200;
const MIN_TRADES_WINDOW: usize = 3;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","ETHUSDT","XRPUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
];

// ── Symbol data ───────────────────────────────────────────────────────────────

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn col_vec(df: &DataFrame, name: &str, n: usize) -> Vec<f64> {
    let chunked = df.column(name).unwrap().f64().unwrap();
    chunked.into_iter().filter_map(|x| x).take(n).collect()
}

// ── Momentum signal ───────────────────────────────────────────────────────────

/// Price momentum: (close_now / close_{bar-N} - 1) * 100
/// Positive = price up over N bars = accumulation
fn momentum(close: &[f64], period: usize, idx: usize) -> f64 {
    let past = idx.saturating_sub(period);
    if past >= close.len() || idx >= close.len() || past >= idx { return 0.0; }
    let c0 = close[past];
    let c1 = close[idx];
    if c0 <= 0.0 || c1 <= 0.0 { return 0.0; }
    (c1 / c0 - 1.0) * 100.0
}

/// Universe mean momentum: average momentum across all symbols at bar `idx`
fn universe_mean_momentum(
    sym_data: &HashMap<String, SymData>,
    symbols: &[&str],
    period: usize,
    idx: usize,
) -> f64 {
    let mut sum = 0.0_f64;
    let mut cnt = 0usize;
    for &sym in symbols {
        if let Some(sd) = sym_data.get(sym) {
            if idx < sd.close.len() {
                sum += momentum(&sd.close, period, idx);
                cnt += 1;
            }
        }
    }
    if cnt > 0 { sum / cnt as f64 } else { 0.0 }
}

/// Mean momentum over training period (for z-score-like baseline)
fn train_mean_momentum(
    sym_data: &HashMap<String, SymData>,
    symbols: &[&str],
    period: usize,
    warmup: usize,
    train_end: usize,
) -> f64 {
    let mut sum = 0.0_f64;
    let mut cnt = 0usize;
    for j in warmup..train_end {
        let uni_mean = universe_mean_momentum(sym_data, symbols, period, j);
        sum += uni_mean;
        cnt += 1;
    }
    if cnt > 0 { sum / cnt as f64 } else { 0.0 }
}

// ── Walk-forward simulation ───────────────────────────────────────────────────

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[&str],
    test_start: usize,
    test_end: usize,
    period: usize,
    train_mean: f64,
) -> (f64, f64, f64, usize, usize) {
    let n = sym_data.values().map(|sd| sd.close.len()).min().unwrap_or(0);
    let test_end = test_end.min(n);

    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::with_capacity(test_end - test_start);

    let mut bar = test_start;
    while bar + 1 < test_end && bar >= WARMUP {
        // Rank symbols by momentum vs universe mean
        let mut scores: Vec<(String, f64)> = Vec::new();
        for &sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let mom = momentum(&sd.close, period, bar);
                scores.push((sym.to_string(), mom - train_mean));
            }
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let prev_equity = equity;
        let trade_exit_bar: Option<usize> = if let Some((sym_key, _)) = scores.first() {
            if let Some(sd) = sym_data.get(sym_key.as_str()) {
                let entry = sd.close[bar] * (1.0 + TAKER_FEE);
                let exit_bar = ((bar + HOLD_BARS).min(test_end - 1))
                    .min(sd.close.len().saturating_sub(1));
                let exit_px = sd.close[exit_bar] * (1.0 - TAKER_FEE);
                let gross_ret = (exit_px / entry - 1.0).max(-0.999);

                total_trades += 1;
                if gross_ret > 0.0 { wins += 1; }
                equity *= 1.0 + gross_ret;
                Some(exit_bar)
            } else { None }
        } else { None };

        peak = peak.max(equity);
        if equity > 0.0 && prev_equity > 0.0 {
            daily_rets.push((equity / prev_equity).ln().max(-5.0));
        }

        bar = trade_exit_bar.map(|eb| eb + 1).unwrap_or(bar + 1);
    }

    let ret = (equity - 1.0) * 100.0;
    let max_dd = if peak > 0.0 { (1.0 - equity / peak) * 100.0 } else { 0.0 };
    let sharpe = if daily_rets.len() >= 5 {
        let mn = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
        let sd = (daily_rets.iter().map(|r| (r - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
        if sd > 1e-9 { mn * 365.0 / (sd * (365.0_f64).sqrt()) } else { 0.0 }
    } else { 0.0 };

    (ret, sharpe, max_dd, total_trades, wins)
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("batch");

    let sweep_values: Vec<usize> = (3..=30).collect();
    let configs: Vec<(String, usize)> = if mode == "batch" {
        sweep_values.iter().map(|&v| (format!("period_{}", v), v)).collect()
    } else {
        let p: usize = mode.parse().unwrap_or(5);
        vec![(format!("period_{}", p), p)]
    };

    // Load all data upfront
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
                let n = df.height();
                min_len = min_len.min(n);
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut full_sym_data: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let close = col_vec(df, "close", n);
            let high  = col_vec(df, "high",  n);
            let low   = col_vec(df, "low",   n);
            let vol   = col_vec(df, "volume", n);
            if close.len() >= WARMUP + TRAIN_BARS + TEST_BARS {
                full_sym_data.insert(sym.clone(), SymData { close, high, low, vol });
            }
        }
    }

    println!("Loaded {} symbols, {} bars", full_sym_data.len(), n);

    // Batch CSV: stdout redirect
    if mode == "batch" {
        println!("universe,window,period,ret,sharpe,max_dd,trades,wins");
    }

    let mut best_per_uni: HashMap<String, (usize, f64)> = HashMap::new();

    for (label, period) in &configs {
        println!("\n=== Config: {} (period={}) ===", label, period);

        for (uni_name, symbols) in UNIVERSES {
            let t0 = Instant::now();

            let sym_data: HashMap<String, SymData> = symbols
                .iter()
                .filter_map(|&s| full_sym_data.get(s).map(|sd| (s.to_string(), SymData {
                    close: sd.close.clone(), high: sd.high.clone(),
                    low: sd.low.clone(), vol: sd.vol.clone()
                })))
                .collect();

            if sym_data.len() < 2 { continue; }
            let uni_n = sym_data.values().map(|sd| sd.close.len()).min().unwrap();
            let total_windows = (uni_n.saturating_sub(WARMUP)) / TEST_BARS;

            let mut all_sharpes = Vec::new();
            let mut pass_count = 0usize;
            let mut total_trades = 0usize;

            for wi in 0..total_windows {
                let train_end = WARMUP + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(uni_n);
                if test_end.saturating_sub(test_start) < 10 { continue; }

                let train_mean = train_mean_momentum(&sym_data, symbols, *period, WARMUP, train_end);

                let (ret, sharpe, max_dd, trades, wins) =
                    run_sim(&sym_data, symbols, test_start, test_end, *period, train_mean);

                if mode == "batch" {
                    println!("{},W{},{},{:.2},{:.4},{:.2},{},{}",
                        uni_name, wi, period, ret, sharpe, max_dd, trades, wins);
                }

                all_sharpes.push(sharpe);
                if trades >= MIN_TRADES_WINDOW && sharpe > 0.0 { pass_count += 1; }
                total_trades += trades;
            }

            let window_count = total_windows;
            let pass_rate = if window_count > 0 { pass_count as f64 / window_count as f64 * 100.0 } else { 0.0 };
            let avg_sh = if !all_sharpes.is_empty() { all_sharpes.iter().sum::<f64>() / all_sharpes.len() as f64 } else { 0.0 };

            let uni_key = uni_name.to_string();
            if !best_per_uni.contains_key(&uni_key) || avg_sh > best_per_uni.get(&uni_key).unwrap().1 {
                best_per_uni.insert(uni_key, (*period, avg_sh));
            }

            println!("  {:>18}: {} win, pass {}/{} ({}%), Sharpe {}, {} trades  ({}ms)",
                uni_name, total_windows, pass_count, window_count, pass_rate, avg_sh, total_trades, t0.elapsed().as_millis());
        }

        if mode != "batch" {
            println!("\n==> {}: done", label);
        }
    }

    if mode == "batch" {
        eprintln!("\n=== Best period per universe ===");
        let mut sorted: Vec<_> = best_per_uni.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        for (uni, (period, sharpe)) in sorted {
            eprintln!("  {}: period={}, Sharpe={:.4}", uni, period, sharpe);
        }
    }

    Ok(())
}

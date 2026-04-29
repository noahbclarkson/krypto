//! S4: ATR-Normalized Position Sizing Walk-Forward
//!
//! Current: CAP=3, equal $10K per position (equal notional)
//! S4: $10K / 21-bar ATR per position (ATR-normalized notional)
//! Mechanism: high-vol symbols get smaller positions, low-vol symbols get larger.
//! Different from failed position scaling overlays (those changed CAP scalar;
//! this adjusts per-symbol notional within CAP=3).
//!
//! Test: 3 configs × Base5 × 6 windows
//! Configs: equal_capital (baseline $10K), atr_norm_10k, atr_norm_20k

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
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;
const TURTLE_ATR_MULT: f64 = 2.00;

const ATR_NORM_BASE_10K: f64 = 10_000.0;
const ATR_NORM_BASE_20K: f64 = 20_000.0;

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

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    let start = idx + 1 - window;
    let slice = &vals[start..=idx];
    if slice.is_empty() { return 0.0; }
    slice.iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let mut max_close = f64::NEG_INFINITY;
    for i in (idx + 1 - entry_period)..idx {
        if let Some(&c) = close.get(i) {
            max_close = max_close.max(c);
        }
    }
    let entry_price = match close.get(idx) {
        Some(&p) => p,
        None => return false,
    };
    if entry_price <= max_close { return false; }
    if atr_mult > 0.0 {
        let atr = atr_at(high, low, close, atr_period, idx);
        if atr > 0.0 && entry_price < max_close + atr_mult * atr {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy)]
enum SizingMode {
    EqualCapital,
    AtrNorm10k,
    AtrNorm20k,
}

fn position_size(mode: SizingMode, atr: f64) -> f64 {
    match mode {
        SizingMode::EqualCapital => 10_000.0,
        SizingMode::AtrNorm10k => if atr > 0.0 { ATR_NORM_BASE_10K / atr } else { 10_000.0 },
        SizingMode::AtrNorm20k => if atr > 0.0 { ATR_NORM_BASE_20K / atr } else { 20_000.0 },
    }
}

fn run_window(
    sym_data: &HashMap<String, SymData>,
    top_syms: &[String],
    test_start: usize,
    test_end: usize,
    mode: SizingMode,
) -> (bool, f64, f64, f64, usize) {
    let mut equity = 10_000.0;
    let mut peak = equity;
    let mut total_trades = 0usize;

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut traded_this_bar = false;

        for sym in top_syms {
            let sd = match sym_data.get(sym) {
                Some(s) => s,
                None => continue,
            };
            if bar >= sd.close.len() { continue; }

            let atr = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar);
            if atr <= 0.0 { continue; }

            if !turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                continue;
            }

            let alloc = position_size(mode, atr);
            let entry_px = sd.close[bar];
            let entry_cost = entry_px * (1.0 + TAKER_FEE);
            let n = sd.close.len();
            let entry_bar_next = bar + 1;

            // Dual exit: Chandelier OR Turtle ATR
            let mut highest_high_chand = sd.high[entry_bar_next];
            let mut lowest_low_turtle = sd.low[entry_bar_next];
            let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
            let mut exit_bar = max_bar;
            for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                highest_high_chand = highest_high_chand.max(sd.high[b]);
                let atr_c = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                let trail_chand = highest_high_chand - CHAND_MULT * atr_c;
                lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                let atr_t = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_t;
                if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                    exit_bar = b;
                    break;
                }
            }

            if let Some(&exit_px) = sd.close.get(exit_bar) {
                let exit_cost = exit_px * (1.0 - TAKER_FEE);
                let gross_ret = exit_cost / entry_cost - 1.0;
                let pnl = alloc * gross_ret;
                total_trades += 1;
                equity += pnl;
                if equity > peak { peak = equity; }
                traded_this_bar = true;
            }
        }

        bar += 1; // always advance bar
    }

    let sharpe = if total_trades >= MIN_TRADES {
        let ret = equity / 10_000.0 - 1.0;
        if total_trades > 0 {
            let daily_mean = ret / total_trades as f64;
            let scale = (total_trades as f64).sqrt() * (252.0_f64.sqrt());
            if daily_mean > 0.0 { daily_mean * scale } else { 0.0 }
        } else { 0.0 }
    } else { 0.0 };

    let passed = total_trades >= MIN_TRADES && equity > 10_000.0;
    let ret_pct = (equity / 10_000.0 - 1.0) * 100.0;
    let dd = ((peak - equity) / peak * 100.0).max(0.0);

    (passed, sharpe, ret_pct, dd, total_trades)
}

fn rank_symbols(sym_data: &HashMap<String, SymData>, bar: usize, symbols: &[&str]) -> Vec<String> {
    let mut scores: Vec<(&str, f64)> = Vec::new();
    for &sym in symbols {
        if let Some(sd) = sym_data.get(sym) {
            if bar >= sd.close.len() { continue; }
            let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
            let price = sd.close.get(bar).copied().unwrap_or(0.0);
            let dv = rol_vol * price;
            scores.push((sym, if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
        }
    }
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect()
}

fn run_universe(
    mode: SizingMode,
    symbols: &[&str],
    sym_data: &HashMap<String, SymData>,
) -> Vec<(usize, bool, f64, f64, f64, usize)> {
    let min_len = sym_data.values().map(|s| s.close.len()).min().unwrap_or(0);
    if min_len < TRAIN_BARS + TEST_BARS + 50 {
        return vec![];
    }

    let n_windows = (min_len - TRAIN_BARS) / TEST_BARS;
    let mut results = Vec::new();

    for w in 0..n_windows {
        let test_start = TRAIN_BARS + w * TEST_BARS;
        let test_end = (test_start + TEST_BARS).min(min_len);
        let top_syms = rank_symbols(sym_data, test_start, symbols);
        let (passed, sharpe, ret, dd, trades) = run_window(sym_data, &top_syms, test_start, test_end, mode);
        results.push((w, passed, sharpe, ret, dd, trades));
    }

    results
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    eprintln!("S4: ATR-Normalized Position Sizing Walk-Forward");
    eprintln!("Base5 x 6 windows x 3 configs\n");

    let loader = DataLoader::new(None, None);

    // Load data
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for &sym in BASE5 {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.to_string(), df); }
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = raw_cache.values().map(|df| df.height()).min().unwrap_or(0).min(2800);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();

    for &sym in BASE5 {
        if let Some(df) = raw_cache.get(sym) {
            let n_rows = df.height().min(n);
            let close: Vec<f64> = df.column("close")?.f64()?.into_iter().filter_map(|x| x).take(n_rows).collect();
            let high: Vec<f64> = df.column("high")?.f64()?.into_iter().filter_map(|x| x).take(n_rows).collect();
            let low: Vec<f64> = df.column("low")?.f64()?.into_iter().filter_map(|x| x).take(n_rows).collect();
            let vol: Vec<f64> = df.column("volume")?.f64()?.into_iter().filter_map(|x| x).take(n_rows).collect();
            sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
        }
    }

    eprintln!("Loaded {} symbols, {} bars\n", sym_data.len(), n);

    let modes: [(SizingMode, &'static str); 3] = [
        (SizingMode::EqualCapital, "equal_capital_baseline"),
        (SizingMode::AtrNorm10k, "atr_norm_10k"),
        (SizingMode::AtrNorm20k, "atr_norm_20k"),
    ];

    let mut all_results: HashMap<usize, Vec<(String, bool, f64, f64, f64, usize)>> = HashMap::new();
    let mut config_summaries: Vec<(String, usize, usize, f64, f64, f64, usize)> = Vec::new();

    for (mode, name) in modes {
        eprintln!("Running {}...", name);
        let results = run_universe(mode, BASE5, &sym_data);
        let total_pass = results.iter().filter(|r| r.1).count();
        let avg_sharpe = results.iter().map(|r| r.2).sum::<f64>() / results.len() as f64;
        let avg_ret = results.iter().map(|r| r.3).sum::<f64>() / results.len() as f64;
        let avg_dd = results.iter().map(|r| r.4).sum::<f64>() / results.len() as f64;
        let total_trades: usize = results.iter().map(|r| r.5).sum();
        let n_windows = results.len();

        eprintln!("  {}/{} pass, Sharpe {}, Ret {}%, DD {}%, Trades {}",
            total_pass, n_windows, avg_sharpe, avg_ret, avg_dd, total_trades);

        config_summaries.push((name.to_string(), total_pass, n_windows, avg_sharpe, avg_ret, avg_dd, total_trades));

        for r in results {
            all_results.entry(r.0).or_default().push((name.to_string(), r.1, r.2, r.3, r.4, r.5));
        }
    }

    eprintln!("\n=== S4 RESULTS ===\n");
    for (name, tp, tw, sh, ret, dd, trades) in &config_summaries {
        let pr = (*tp as f64 / *tw as f64 * 100.0).round() as i32;
        eprintln!("{}: {}/{} pass ({}%), Sharpe {}, Ret {}%, DD {}%, Trades {}",
            name, tp, tw, pr, sh, ret, dd, trades);
    }

    // Comparison vs baseline
    eprintln!("\n--- Config Comparison (vs equal_capital_baseline) ---");
    let baseline = config_summaries.iter().find(|r| r.0 == "equal_capital_baseline").cloned();
    if let Some((_, _, _, bsh, bret, bdd, _)) = baseline {
        for (name, _, _, sh, ret, dd, _) in &config_summaries {
            if name == "equal_capital_baseline" { continue; }
            eprintln!("{} vs baseline: Sharpe {}, Ret {}pp, DD {}pp",
                name, sh - bsh, ret - bret, dd - bdd);
        }
    }

    // Winner
    let winner = config_summaries.iter().max_by(|a, b| a.3.partial_cmp(&b.3).unwrap()).map(|(n, ..)| n.clone());
    if let Some(w) = winner {
        eprintln!("\nWINNER: {} (by Sharpe)", w);
    }

    // Write CSV
    {
        let mut f = File::create("snapshots/s4_atr_norm_position_sizing.csv")?;
        writeln!(f, "config,pass_windows,total_windows,pass_rate_pct,avg_sharpe,avg_return_pct,avg_dd_pct,total_trades")?;
        for (name, tp, tw, sh, ret, dd, trades) in &config_summaries {
            let pr = (*tp as f64 / *tw as f64 * 100.0).round();
            writeln!(f, "{},{},{},{},{},{},{},{}", name, tp, tw, pr, sh, ret, dd, trades)?;
        }
    }

    // Per-window detail
    {
        let mut f = File::create("snapshots/s4_atr_norm_position_sizing_detail.csv")?;
        writeln!(f, "window,config,passed,sharpe,return_pct,dd_pct,trades")?;
        for (&w, results) in &all_results {
            for (name, passed, sharpe, ret, dd, trades) in results {
                writeln!(f, "{},{},{},{},{},{},{}", w, name, passed, sharpe, ret, dd, trades)?;
            }
        }
    }

    eprintln!("\nRuntime: {:?}", start.elapsed());

    Ok(())
}
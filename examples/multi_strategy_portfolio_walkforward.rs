//! Multi-Strategy Portfolio Walk-Forward: Turtle+Chandelier + A/D Momentum
//!
//! PURPOSE: Test whether our two viable strategies provide genuine diversification
//! when combined at the daily return level, or if they duplicate trend exposure.
//!
//! DESIGN:
//! - Turtle+Chandelier: EP=21, Chandelier(28, 2.0), CAP=3, HM=45
//! - A/D Momentum: period=47, fixed 54-bar hold, top-1 by momentum
//! - Combined: 50% Turtle + 50% A/D daily returns
//! - Walk-forward: 252/252 train/test across all 9 universes
//!
//! KEY QUESTIONS:
//! 1. Is portfolio Sharpe meaningfully higher than either strategy alone?
//! 2. Is portfolio max DD meaningfully lower?
//! 3. How correlated are the daily returns?
//! 4. How much entry overlap exists (same symbol, same bar)?

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

// ── Strategy parameters (from validated hyperopts) ────────────────────────────

// Turtle+Chandelier
const TURTLE_ENTRY: usize = 21;    // hyperopt 2026-04-10
const CHAND_PERIOD: usize = 28;    // hyperopt 2026-04-11
const CHAND_MULT: f64 = 2.00;     // hyperopt 2026-04-11
const TURTLE_CAP: usize = 3;      // hyperopt 2026-04-11
const TURTLE_HM: usize = 45;      // hyperopt 2026-04-11

// A/D Momentum
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)      // hyperopt 2026-04-11: robustness champion (87% QP)
const AD_HOLD: usize = 54;        // from full sweep

// Shared
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;

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

const CSV_OUT: &str = "snapshots/multi_strategy_portfolio_wf.csv";

// ── Data structures ───────────────────────────────────────────────────────────

struct SymData {
    close: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    volume: Vec<f64>,
    ad_line: Vec<f64>,
    ad_momentum: Vec<f64>,
}

struct DailySimResult {
    daily_rets: Vec<f64>,       // daily returns for the test window
    equity_curve: Vec<f64>,     // cumulative equity
    trades: usize,
    entry_bars: Vec<usize>,     // bar indices where entries occurred
    entry_syms: Vec<String>,    // symbols entered
}

struct WindowResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    pass: bool,
    daily_rets: Vec<f64>,
    entry_bars: Vec<usize>,
    entry_syms: Vec<String>,
}

struct PortfolioResult {
    turtle: WindowResult,
    ad: WindowResult,
    combined: WindowResult,
    entry_overlap: usize,    // number of bars where both strategies entered same symbol
    return_corr: f64,        // correlation of daily returns
}

// ── Helper functions ──────────────────────────────────────────────────────────

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let n = daily_rets.len() as f64;
    let mean: f64 = daily_rets.iter().sum::<f64>() / n;
    let var: f64 = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let sd = var.sqrt();
    if sd == 0.0 { return 0.0; }
    mean / sd * 365.0_f64.sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = 0.0_f64;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

fn correlation(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len().min(ys.len());
    if n < 5 { return 0.0; }
    let mx: f64 = xs[..n].iter().sum::<f64>() / n as f64;
    let my: f64 = ys[..n].iter().sum::<f64>() / n as f64;
    let mut cov = 0.0;
    let mut vx = 0.0;
    let mut vy = 0.0;
    for i in 0..n {
        let dx = xs[i] - mx;
        let dy = ys[i] - my;
        cov += dx * dy;
        vx += dx * dx;
        vy += dy * dy;
    }
    let denom = vx.sqrt() * vy.sqrt();
    if denom == 0.0 { return 0.0; }
    cov / denom
}

fn compute_ad_line(high: &[f64], low: &[f64], close: &[f64], volume: &[f64]) -> Vec<f64> {
    let n = high.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let h = high[i];
        let l = low[i];
        let c = close[i];
        let v = volume[i];
        let range = h - l;
        let mf = if range > 1e-9 { ((c - l) - (h - c)) / range } else { 0.0 };
        ad[i] = if i == 0 { mf * v } else { ad[i - 1] + mf * v };
    }
    ad
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

// ── Turtle+Chandelier bar-by-bar simulation ───────────────────────────────────

fn sim_turtle_chandelier(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> DailySimResult {
    let mut daily_rets = Vec::new();
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut trades = 0usize;
    let mut entry_bars = Vec::new();
    let mut entry_syms = Vec::new();

    // Active position: (symbol, entry_bar, entry_price, highest_high)
    let mut pos: Option<(String, usize, f64, f64)> = None;

    let mut bar = test_start;

    while bar + 1 < test_end {
        if let Some((ref sym, entry_bar, entry_price, ref mut hh)) = pos {
            // In position — check Chandelier exit
            if let Some(sd) = sym_data.get(sym) {
                if bar < sd.close.len() {
                    let h = sd.high.get(bar).copied().unwrap_or(0.0);
                    *hh = (*hh).max(h);
                    let atr_val = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, bar);
                    let trail = *hh - CHAND_MULT * atr_val;

                    let do_exit = sd.close[bar] < trail
                        || (bar - entry_bar) >= TURTLE_HM
                        || bar >= test_end - 1;

                    if do_exit {
                        let exit_px = sd.close[bar];
                        let gross = (exit_px * (1.0 - TAKER_FEE)) / (entry_price * (1.0 + TAKER_FEE)) - 1.0;
                        equity *= 1.0 + gross;
                        trades += 1;

                        let bars_held = bar - entry_bar;
                        let avg_daily = if bars_held > 0 { gross / bars_held as f64 } else { gross };
                        for _ in 0..bars_held.max(1) {
                            daily_rets.push(avg_daily);
                            equity_curve.push(equity);
                        }
                        pos = None;
                    } else {
                        bar += 1;
                        continue;
                    }
                }
            }
        }

        // No position — look for entry
        if pos.is_none() {
            // Rank by dollar volume, take top CAP
            let mut scores: Vec<(&str, f64)> = Vec::new();
            for sym in symbols {
                if let Some(sd) = sym_data.get(sym) {
                    if bar >= sd.close.len() { continue; }
                    let dv = sd.volume.get(bar).copied().unwrap_or(0.0)
                        * sd.close.get(bar).copied().unwrap_or(0.0);
                    scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
                }
            }
            scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            let top_syms: Vec<&str> = scores.iter().take(TURTLE_CAP).map(|(s, _)| *s).collect();

            let mut entered = false;
            for sym_str in &top_syms {
                if let Some(sd) = sym_data.get(*sym_str) {
                    if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                        if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                            let entry_px = sd.close[bar];
                            let hh = sd.high.get(bar + 1).copied().unwrap_or(entry_px);
                            pos = Some((sym_str.to_string(), bar, entry_px, hh));
                            entry_bars.push(bar);
                            entry_syms.push(sym_str.to_string());
                            entered = true;
                            break;
                        }
                    }
                }
            }

            if !entered {
                daily_rets.push(0.0);
                equity_curve.push(equity);
            }
        }

        bar += 1;
    }

    // Close any remaining position
    if let Some((sym, entry_bar, entry_price, _)) = pos {
        if let Some(sd) = sym_data.get(&sym) {
            let exit_bar = (test_end - 1).min(sd.close.len() - 1);
            let exit_px = sd.close[exit_bar];
            let gross = (exit_px * (1.0 - TAKER_FEE)) / (entry_price * (1.0 + TAKER_FEE)) - 1.0;
            equity *= 1.0 + gross;
            trades += 1;
            let bars_held = exit_bar - entry_bar;
            let avg_daily = if bars_held > 0 { gross / bars_held as f64 } else { gross };
            for _ in 0..bars_held.max(1) {
                daily_rets.push(avg_daily);
                equity_curve.push(equity);
            }
        }
    }

    DailySimResult { daily_rets, equity_curve, trades, entry_bars, entry_syms }
}

// ── A/D Momentum bar-by-bar simulation ────────────────────────────────────────

fn sim_ad_momentum(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> DailySimResult {
    let mut daily_rets = Vec::new();
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut trades = 0usize;
    let mut entry_bars = Vec::new();
    let mut entry_syms = Vec::new();

    // Active position: (symbol, entry_bar, entry_price)
    let mut pos: Option<(String, usize, f64)> = None;

    let mut bar = test_start;

    while bar + 1 < test_end {
        if let Some((ref sym, entry_bar, entry_price)) = pos {
            // In position — check hold duration
            let bars_held = bar - entry_bar;
            let do_exit = bars_held >= AD_HOLD || bar >= test_end - 1;

            if do_exit {
                if let Some(sd) = sym_data.get(sym) {
                    let exit_bar = bar.min(sd.close.len() - 1);
                    let exit_px = sd.close[exit_bar];
                    let gross = (exit_px * (1.0 - TAKER_FEE)) / (entry_price * (1.0 + TAKER_FEE)) - 1.0;
                    equity *= 1.0 + gross;
                    trades += 1;
                    let avg_daily = if bars_held > 0 { gross / bars_held as f64 } else { gross };
                    for _ in 0..bars_held.max(1) {
                        daily_rets.push(avg_daily);
                        equity_curve.push(equity);
                    }
                }
                pos = None;
            } else {
                bar += 1;
                continue;
            }
        }

        // No position — look for entry via A/D momentum ranking
        if pos.is_none() {
            let idx = bar.saturating_sub(1);
            let mut longs: Vec<(&String, f64)> = Vec::new();

            for sym in symbols {
                if let Some(sd) = sym_data.get(sym) {
                    if idx < AD_PERIOD || idx >= sd.ad_momentum.len() { continue; }
                    let mom = sd.ad_momentum[idx];
                    if mom > 0.0 {
                        longs.push((sym, mom));
                    }
                }
            }

            longs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            // Take top-1 long
            if let Some((best_sym, _)) = longs.first() {
                if let Some(sd) = sym_data.get(best_sym.as_str()) {
                    if bar < sd.open.len() {
                        let entry_price = sd.open[bar];
                        if entry_price > 0.0 {
                            pos = Some((best_sym.to_string(), bar, entry_price));
                            entry_bars.push(bar);
                            entry_syms.push(best_sym.to_string());
                            // Don't push daily_ret here — will be pushed on exit
                        }
                    }
                }
            }

            if pos.is_none() {
                daily_rets.push(0.0);
                equity_curve.push(equity);
            }
        }

        bar += 1;
    }

    // Close remaining
    if let Some((sym, entry_bar, entry_price)) = pos {
        if let Some(sd) = sym_data.get(&sym) {
            let exit_bar = (test_end - 1).min(sd.close.len() - 1);
            let exit_px = sd.close[exit_bar];
            let bars_held = exit_bar - entry_bar;
            let gross = (exit_px * (1.0 - TAKER_FEE)) / (entry_price * (1.0 + TAKER_FEE)) - 1.0;
            equity *= 1.0 + gross;
            trades += 1;
            let avg_daily = if bars_held > 0 { gross / bars_held as f64 } else { gross };
            for _ in 0..bars_held.max(1) {
                daily_rets.push(avg_daily);
                equity_curve.push(equity);
            }
        }
    }

    DailySimResult { daily_rets, equity_curve, trades, entry_bars, entry_syms }
}

// ── Combine daily returns into portfolio ───────────────────────────────────────

fn combine_portfolio(
    turtle_daily: &[f64],
    ad_daily: &[f64],
    turtle_weight: f64,
    ad_weight: f64,
) -> WindowResult {
    let n = turtle_daily.len().max(ad_daily.len());
    let mut combined_daily = Vec::with_capacity(n);
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];

    for i in 0..n {
        let tr = turtle_daily.get(i).copied().unwrap_or(0.0);
        let ar = ad_daily.get(i).copied().unwrap_or(0.0);
        let cr = turtle_weight * tr + ad_weight * ar;
        combined_daily.push(cr);
        equity *= 1.0 + cr;
        equity_curve.push(equity);
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&combined_daily);
    let max_dd = max_dd_from(&equity_curve);

    WindowResult {
        ret, sharpe, max_dd,
        trades: 0, // portfolio doesn't have a single trade count
        pass: ret > 0.0,
        daily_rets: combined_daily,
        entry_bars: Vec::new(),
        entry_syms: Vec::new(),
    }
}

fn to_window_result(sim: DailySimResult) -> WindowResult {
    let ret = if sim.equity_curve.is_empty() { 0.0 } else {
        (sim.equity_curve.last().copied().unwrap_or(1.0) - 1.0) * 100.0
    };
    let sharpe = annualised_sharpe(&sim.daily_rets);
    let max_dd = max_dd_from(&sim.equity_curve);
    WindowResult {
        ret, sharpe, max_dd,
        trades: sim.trades,
        pass: sim.trades >= MIN_TRADES && ret > 0.0,
        daily_rets: sim.daily_rets,
        entry_bars: sim.entry_bars,
        entry_syms: sim.entry_syms,
    }
}

fn count_entry_overlap(turtle: &WindowResult, ad: &WindowResult) -> usize {
    // Count bars where both strategies entered the same symbol
    let mut overlap = 0usize;
    for i in 0..turtle.entry_bars.len() {
        let tb = turtle.entry_bars[i];
        let ts = &turtle.entry_syms[i];
        for j in 0..ad.entry_bars.len() {
            let ab = ad.entry_bars[j];
            let as_ = &ad.entry_syms[j];
            // Same bar (within ±2 bars tolerance) and same symbol
            if (tb as i64 - ab as i64).abs() <= 2 && ts == as_ {
                overlap += 1;
            }
        }
    }
    overlap
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("═══════════════════════════════════════════════════════════════");
    eprintln!("  Multi-Strategy Portfolio Walk-Forward");
    eprintln!("  Turtle+Chandelier (EP={}, Chand({}, {}), CAP={}, HM={})", TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_CAP, TURTLE_HM);
    eprintln!("  A/D Momentum (period={}, hold={})", AD_PERIOD, AD_HOLD);
    eprintln!("  Combined: 50/50 daily returns");
    eprintln!("═══════════════════════════════════════════════════════════════\n");

    // Load data
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                let with_techs = FeatureEngine::add_technicals(&df, None).unwrap_or_else(|_| df.clone());
                min_len = min_len.min(with_techs.height());
                raw_cache.insert(sym.clone(), with_techs);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);

    // Build SymData with A/D lines
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for (sym, df) in &raw_cache {
        let n_min = df.height().min(n);
        macro_rules! col_vec {
            ($name:expr) => {{
                let chunked = df.column($name)?.f64()?;
                chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
            }};
        }
        let close: Vec<f64> = col_vec!("close");
        let open: Vec<f64> = col_vec!("open");
        let high: Vec<f64> = col_vec!("high");
        let low: Vec<f64> = col_vec!("low");
        let volume: Vec<f64> = col_vec!("volume");

        let ad_line = compute_ad_line(&high, &low, &close, &volume);
        let mut ad_momentum = vec![0.0; ad_line.len()];
        for i in AD_PERIOD..ad_line.len() {
            ad_momentum[i] = ad_line[i] - ad_line[i - AD_PERIOD];
        }

        sym_data_map.insert(sym.clone(), SymData {
            close, open, high, low, volume, ad_line, ad_momentum,
        });
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Run walk-forward across all universes
    let mut csv_lines = vec![
        "universe,window,turtle_ret,turtle_sh,turtle_dd,turtle_trades,turtle_pass,ad_ret,ad_sh,ad_dd,ad_trades,ad_pass,comb_ret,comb_sh,comb_dd,comb_pass,entry_overlap,return_corr".to_string()
    ];

    let mut global_turtle_pass = 0usize;
    let mut global_ad_pass = 0usize;
    let mut global_comb_pass = 0usize;
    let mut global_total = 0usize;

    let mut all_turtle_sharpes = Vec::new();
    let mut all_ad_sharpes = Vec::new();
    let mut all_comb_sharpes = Vec::new();
    let mut all_corrs = Vec::new();
    let mut all_overlaps = Vec::new();
    let mut total_turtle_entries = 0usize;
    let mut total_ad_entries = 0usize;

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing data)", label);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (not enough data)", label);
            continue;
        }

        eprintln!("══ {:<18} ══ {} syms, {} windows ══", label, symbols.len(), total_windows);

        let mut uni_turtle_pass = 0usize;
        let mut uni_ad_pass = 0usize;
        let mut uni_comb_pass = 0usize;
        let mut uni_turtle_sharpes = Vec::new();
        let mut uni_ad_sharpes = Vec::new();
        let mut uni_comb_sharpes = Vec::new();
        let mut uni_corrs = Vec::new();

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);

            if test_end.saturating_sub(test_start) < 5 { continue; }

            // Run both strategies
            let turtle_sim = sim_turtle_chandelier(&sym_data_map, &symbols, test_start, test_end);
            let ad_sim = sim_ad_momentum(&sym_data_map, &symbols, test_start, test_end);

            let turtle = to_window_result(turtle_sim);
            let ad = to_window_result(ad_sim);

            // Combine 50/50
            let combined = combine_portfolio(&turtle.daily_rets, &ad.daily_rets, 0.5, 0.5);

            // Measure overlap
            let overlap = count_entry_overlap(&turtle, &ad);
            let ret_corr = correlation(&turtle.daily_rets, &ad.daily_rets);

            // Print window result
            eprintln!(
                "  W{:02} | T:{:+7.1}% sh={:5.2} {:3}t {} | AD:{:+7.1}% sh={:5.2} {:3}t {} | C:{:+7.1}% sh={:5.2} {} | overlap={}/{}, corr={:.2}",
                wi,
                turtle.ret, turtle.sharpe, turtle.trades, if turtle.pass { "✓" } else { "✗" },
                ad.ret, ad.sharpe, ad.trades, if ad.pass { "✓" } else { "✗" },
                combined.ret, combined.sharpe, if combined.pass { "✓" } else { "✗" },
                overlap,
                turtle.entry_bars.len() + ad.entry_bars.len(),
                ret_corr,
            );

            csv_lines.push(format!(
                "{},{},{:.2},{:.4},{:.2},{},{},{:.2},{:.4},{:.2},{},{},{:.2},{:.4},{:.2},{},{},{:.4}",
                label, wi,
                turtle.ret, turtle.sharpe, turtle.max_dd, turtle.trades, turtle.pass,
                ad.ret, ad.sharpe, ad.max_dd, ad.trades, ad.pass,
                combined.ret, combined.sharpe, combined.max_dd, combined.pass,
                overlap, ret_corr,
            ));

            if turtle.pass { uni_turtle_pass += 1; global_turtle_pass += 1; }
            if ad.pass { uni_ad_pass += 1; global_ad_pass += 1; }
            if combined.pass { uni_comb_pass += 1; global_comb_pass += 1; }
            global_total += 1;

            uni_turtle_sharpes.push(turtle.sharpe);
            uni_ad_sharpes.push(ad.sharpe);
            uni_comb_sharpes.push(combined.sharpe);
            uni_corrs.push(ret_corr);

            all_turtle_sharpes.push(turtle.sharpe);
            all_ad_sharpes.push(ad.sharpe);
            all_comb_sharpes.push(combined.sharpe);
            all_corrs.push(ret_corr);
            all_overlaps.push(overlap);
            total_turtle_entries += turtle.entry_bars.len();
            total_ad_entries += ad.entry_bars.len();
        }

        let avg_ts: f64 = uni_turtle_sharpes.iter().sum::<f64>() / uni_turtle_sharpes.len().max(1) as f64;
        let avg_as: f64 = uni_ad_sharpes.iter().sum::<f64>() / uni_ad_sharpes.len().max(1) as f64;
        let avg_cs: f64 = uni_comb_sharpes.iter().sum::<f64>() / uni_comb_sharpes.len().max(1) as f64;
        let avg_corr: f64 = uni_corrs.iter().sum::<f64>() / uni_corrs.len().max(1) as f64;

        eprintln!(
            "  AGG | T:{}/{} sh={:.2} | AD:{}/{} sh={:.2} | C:{}/{} sh={:.2} | corr={:.2}\n",
            uni_turtle_pass, uni_turtle_sharpes.len(), avg_ts,
            uni_ad_pass, uni_ad_sharpes.len(), avg_as,
            uni_comb_pass, uni_comb_sharpes.len(), avg_cs,
            avg_corr,
        );
    }

    // Write CSV
    {
        let mut f = File::create(CSV_OUT)?;
        for line in &csv_lines { writeln!(f, "{}", line)?; }
    }

    // Global summary
    let avg_ts: f64 = all_turtle_sharpes.iter().sum::<f64>() / all_turtle_sharpes.len().max(1) as f64;
    let avg_as: f64 = all_ad_sharpes.iter().sum::<f64>() / all_ad_sharpes.len().max(1) as f64;
    let avg_cs: f64 = all_comb_sharpes.iter().sum::<f64>() / all_comb_sharpes.len().max(1) as f64;
    let avg_corr: f64 = all_corrs.iter().sum::<f64>() / all_corrs.len().max(1) as f64;
    let total_overlap: usize = all_overlaps.iter().sum();
    let total_entries = total_turtle_entries + total_ad_entries;

    eprintln!("\n═══════════════════════════════════════════════════════════════");
    eprintln!("  GLOBAL SUMMARY");
    eprintln!("═══════════════════════════════════════════════════════════════");
    eprintln!("  {} windows across {} universes", global_total, UNIVERSES.len());
    eprintln!("");
    eprintln!("  Strategy        | Pass    | Avg Sharpe | Description");
    eprintln!("  ─────────────────────────────────────────────────────────");
    eprintln!("  Turtle+Chand    | {:>3}/{:<3} | {:>8.2}  | EP=21, Chand(28,2.0), CAP=3", global_turtle_pass, global_total, avg_ts);
    eprintln!("  A/D Momentum    | {:>3}/{:<3} | {:>8.2}  | period={}, hold={}", global_ad_pass, global_total, avg_as, AD_PERIOD, AD_HOLD);
    eprintln!("  Combined 50/50  | {:>3}/{:<3} | {:>8.2}  | Portfolio", global_comb_pass, global_total, avg_cs);
    eprintln!("");
    eprintln!("  Return correlation (T vs AD):  {:.3}", avg_corr);
    eprintln!("  Entry overlap: {} / {} ({:.1}%)", total_overlap, total_entries,
        if total_entries > 0 { total_overlap as f64 / total_entries as f64 * 100.0 } else { 0.0 });
    eprintln!("");

    // Key diagnostic: does combined beat both individually?
    let comb_beats_turtle = all_comb_sharpes.iter().zip(all_turtle_sharpes.iter())
        .filter(|(c, t)| *c > *t).count();
    let comb_beats_ad = all_comb_sharpes.iter().zip(all_ad_sharpes.iter())
        .filter(|(c, a)| *c > *a).count();
    let comb_beats_both = all_comb_sharpes.iter()
        .zip(all_turtle_sharpes.iter())
        .zip(all_ad_sharpes.iter())
        .filter(|((c, t), a)| *c > *t && *c > *a)
        .count();

    eprintln!("  Combined beats Turtle: {}/{} ({:.0}%)", comb_beats_turtle, global_total, comb_beats_turtle as f64 / global_total.max(1) as f64 * 100.0);
    eprintln!("  Combined beats A/D:    {}/{} ({:.0}%)", comb_beats_ad, global_total, comb_beats_ad as f64 / global_total.max(1) as f64 * 100.0);
    eprintln!("  Combined beats BOTH:   {}/{} ({:.0}%)", comb_beats_both, global_total, comb_beats_both as f64 / global_total.max(1) as f64 * 100.0);
    eprintln!("");
    eprintln!("  CSV: {}", CSV_OUT);
    eprintln!("  Runtime: {:?}", t0.elapsed());

    Ok(())
}

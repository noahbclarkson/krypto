//! Drawdown-Triggered Position Sizing — THRESHOLD SWEEP
//!
//! Sweeps DD_THRESHOLD across {15%, 20%, 25%, 30%, 35%, 40%}
//! to find the sweet spot where the trigger helps in genuine crises
//! without destroying returns during normal dips.
//!
//! Reports per-threshold: pass rate, avg DD, avg return, W05-specific stats.

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
const HOLD_MAX: usize = 45; // hyperopt 2026-04-11: HM=45 winner
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;
const DD_LOOKBACK: usize = 7;
const COOLDOWN: usize = 21;
const SIZE_REDUCTION: f64 = 0.50;

const THRESHOLDS: &[f64] = &[0.15, 0.20, 0.25, 0.30, 0.35, 0.40];

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

const CSV_OUT: &str = "snapshots/drawdown_trigger_threshold_sweep.csv";

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
    pass: bool,
    trigger_fires: usize,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    btc_close: &[f64],
    test_start: usize,
    test_end: usize,
    dd_threshold: f64,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut trigger_active_until: usize = 0;
    let mut trigger_fire_count = 0usize;

    let neg_thresh = -dd_threshold;

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut is_triggered = false;
        if bar >= DD_LOOKBACK && bar < btc_close.len() {
            if trigger_active_until > bar {
                is_triggered = true;
            } else {
                let btc_now = btc_close.get(bar).copied().unwrap_or(0.0);
                let btc_then = btc_close.get(bar - DD_LOOKBACK).copied().unwrap_or(0.0);
                if btc_then > 0.0 {
                    let rolling_ret = (btc_now / btc_then) - 1.0;
                    if rolling_ret < neg_thresh {
                        trigger_active_until = bar + COOLDOWN;
                        trigger_fire_count += 1;
                        is_triggered = true;
                    }
                }
            }
        }
        let size_mult = if is_triggered { SIZE_REDUCTION } else { 1.0 };

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
                    if turtle_signal(&sd.close, &sd.high, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let mut exit_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        for b in entry_bar_next..exit_bar.min(n.saturating_sub(1)) {
                            highest_high = highest_high.max(sd.high[b]);
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail = highest_high - CHAND_MULT * atr_val;
                            if sd.close[b] < trail {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let sized_ret = gross_ret * size_mult;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + sized_ret;

                            let avg_daily = sized_ret / bars_held as f64;
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }

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
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    SimResult { ret, sharpe, max_dd, trades: total_trades, pass, trigger_fires: trigger_fire_count }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Drawdown Trigger Threshold Sweep ====");
    eprintln!("Thresholds: {:?}  |  Lookback: {} bars  |  Cooldown: {} bars  |  Size reduction: {}%",
        THRESHOLDS.iter().map(|t| format!("{:.0}%", t * 100.0)).collect::<Vec<_>>(),
        DD_LOOKBACK, COOLDOWN, SIZE_REDUCTION * 100.0);

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

    let btc_close: Vec<f64> = sym_data_map.get("BTCUSDT")
        .map(|sd| sd.close.clone())
        .unwrap_or_default();

    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Structure: per-threshold → per-window → result
    // Also run BASE (no trigger, threshold = infinity)

    struct ThresholdResult {
        threshold: f64,
        total_windows: usize,
        pass_count: usize,
        avg_dd: f64,
        avg_ret: f64,
        avg_sharpe: f64,
        total_trades: usize,
        windows_with_fires: usize,
        total_fires: usize,
        // W05-specific (worst DD windows)
        worst_window_dd: f64,
        worst_window_ret: f64,
        // DD in windows where trigger fires vs doesn't
        avg_dd_when_fired: f64,
        avg_dd_when_not_fired: f64,
        avg_ret_when_fired: f64,
        avg_ret_when_not_fired: f64,
        dd_helped: usize,
        dd_hurt: usize,
    }

    let mut threshold_results: Vec<ThresholdResult> = Vec::new();

    // Run BASE first (threshold = 999, effectively disabled)
    let all_thresholds: Vec<f64> = vec![999.0].into_iter().chain(THRESHOLDS.iter().copied()).collect();

    let mut csv_lines = vec!["threshold,universe,window,ret,sharpe,max_dd,trades,pass,fires".to_string()];

    for &thresh in &all_thresholds {
        let thresh_label = if thresh > 1.0 { "BASE".to_string() } else { format!("{:.0}%", thresh * 100.0) };
        eprintln!("--- Threshold: {} ---", thresh_label);

        let mut all_rets = Vec::new();
        let mut all_dds = Vec::new();
        let mut all_sharpes = Vec::new();
        let mut total_trades = 0usize;
        let mut pass_count = 0usize;
        let mut total_windows = 0usize;
        let mut windows_with_fires = 0usize;
        let mut total_fires = 0usize;

        let mut fired_dds = Vec::new();
        let mut fired_rets = Vec::new();
        let mut not_fired_dds = Vec::new();
        let mut not_fired_rets = Vec::new();
        let mut dd_helped = 0usize;
        let mut dd_hurt = 0usize;

        // Store base results for comparison
        // We'll compare against BASE at the end

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_w = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            for wi in 0..total_w {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, &btc_close, test_start, test_end, thresh);

                all_rets.push(r.ret);
                all_dds.push(r.max_dd);
                all_sharpes.push(r.sharpe);
                total_trades += r.trades;
                total_windows += 1;
                if r.pass { pass_count += 1; }

                if r.trigger_fires > 0 {
                    windows_with_fires += 1;
                    total_fires += r.trigger_fires;
                    fired_dds.push(r.max_dd);
                    fired_rets.push(r.ret);
                } else {
                    not_fired_dds.push(r.max_dd);
                    not_fired_rets.push(r.ret);
                }

                csv_lines.push(format!("{},{},{},{:.2},{:.4},{:.2},{},{},{}",
                    thresh_label, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.pass, r.trigger_fires));
            }
        }

        let avg_dd: f64 = all_dds.iter().sum::<f64>() / all_dds.len().max(1) as f64;
        let avg_ret: f64 = all_rets.iter().sum::<f64>() / all_rets.len().max(1) as f64;
        let avg_sharpe: f64 = all_sharpes.iter().sum::<f64>() / all_sharpes.len().max(1) as f64;

        // Find worst window
        let worst_idx = all_dds.iter().enumerate().max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap()).map(|(i, _)| i);
        let worst_dd = worst_idx.map(|i| all_dds[i]).unwrap_or(0.0);
        let worst_ret = worst_idx.map(|i| all_rets[i]).unwrap_or(0.0);

        let avg_dd_fired: f64 = fired_dds.iter().sum::<f64>() / fired_dds.len().max(1) as f64;
        let avg_dd_not_fired: f64 = not_fired_dds.iter().sum::<f64>() / not_fired_dds.len().max(1) as f64;
        let avg_ret_fired: f64 = fired_rets.iter().sum::<f64>() / fired_rets.len().max(1) as f64;
        let avg_ret_not_fired: f64 = not_fired_rets.iter().sum::<f64>() / not_fired_rets.len().max(1) as f64;

        threshold_results.push(ThresholdResult {
            threshold: thresh,
            total_windows,
            pass_count,
            avg_dd,
            avg_ret,
            avg_sharpe,
            total_trades,
            windows_with_fires,
            total_fires,
            worst_window_dd: worst_dd,
            worst_window_ret: worst_ret,
            avg_dd_when_fired: avg_dd_fired,
            avg_dd_when_not_fired: avg_dd_not_fired,
            avg_ret_when_fired: avg_ret_fired,
            avg_ret_when_not_fired: avg_ret_not_fired,
            dd_helped,
            dd_hurt,
        });

        eprintln!("  Pass: {}/{} ({:.0}%) | Avg DD: {:.1}% | Avg Ret: {:+.1}% | Avg Sharpe: {:.2} | Fires: {} in {} windows",
            pass_count, total_windows, pass_count as f64 / total_windows as f64 * 100.0,
            avg_dd, avg_ret, avg_sharpe, total_fires, windows_with_fires);
    }

    // Now compute dd_helped / dd_hurt by comparing each threshold to BASE
    // We need the BASE results per-window
    let base_results: Vec<(String, usize, f64, f64)> = {
        let mut results = Vec::new();
        for line in &csv_lines {
            if line.starts_with("threshold") { continue; }
            let parts: Vec<&str> = line.split(',').collect();
            if parts[0] == "BASE" {
                let uni = parts[1].to_string();
                let wi: usize = parts[2].parse().unwrap_or(0);
                let ret: f64 = parts[3].parse().unwrap_or(0.0);
                let dd: f64 = parts[5].parse().unwrap_or(0.0);
                results.push((uni, wi, ret, dd));
            }
        }
        results
    };

    eprintln!("\n===== COMPARISON TABLE =====");
    eprintln!("| Threshold | Pass | Avg DD | DD Δ vs BASE | Avg Ret | Ret Δ vs BASE | Avg Sharpe | Worst DD | Fires | Windows w/ fires |");
    eprintln!("|---|---|---|---|---|---|---|---|---|---|");

    let base_avg_dd = threshold_results.first().map(|r| r.avg_dd).unwrap_or(0.0);
    let base_avg_ret = threshold_results.first().map(|r| r.avg_ret).unwrap_or(0.0);

    for r in &threshold_results {
        let label = if r.threshold > 1.0 { "BASE".to_string() } else { format!("{:.0}%", r.threshold * 100.0) };
        let dd_delta = base_avg_dd - r.avg_dd;
        let ret_delta = r.avg_ret - base_avg_ret;
        eprintln!("| {} | {}/{} | {:.1}% | {:+.1}% | {:+.1}% | {:+.1}% | {:.2} | {:.1}% | {} | {} |",
            label, r.pass_count, r.total_windows, r.avg_dd, dd_delta, r.avg_ret, ret_delta,
            r.avg_sharpe, r.worst_window_dd, r.total_fires, r.windows_with_fires);
    }

    // Per-window DD comparison against BASE
    eprintln!("\n===== PER-THRESHOLD DD HELP/HURT vs BASE =====");
    for &thresh in THRESHOLDS {
        let thresh_label = format!("{:.0}%", thresh * 100.0);
        let mut helped = 0usize;
        let mut hurt = 0usize;
        let mut neutral = 0usize;
        let mut total_dd_help = 0.0_f64;
        let mut total_dd_hurt = 0.0_f64;

        for line in &csv_lines {
            if line.starts_with("threshold") { continue; }
            let parts: Vec<&str> = line.split(',').collect();
            if parts[0] != thresh_label { continue; }

            let uni = parts[1];
            let wi: usize = parts[2].parse().unwrap_or(0);
            let dd: f64 = parts[5].parse().unwrap_or(0.0);

            // Find BASE match
            if let Some(base) = base_results.iter().find(|(u, w, _, _)| *u == uni && *w == wi) {
                let dd_delta = base.3 - dd;
                if dd_delta > 0.5 {
                    helped += 1;
                    total_dd_help += dd_delta;
                } else if dd_delta < -0.5 {
                    hurt += 1;
                    total_dd_hurt += dd_delta.abs();
                } else {
                    neutral += 1;
                }
            }
        }

        let avg_help = if helped > 0 { total_dd_help / helped as f64 } else { 0.0 };
        let avg_hurt = if hurt > 0 { total_dd_hurt / hurt as f64 } else { 0.0 };

        eprintln!("  {}: HELPED {} (avg {:+.1}%) | HURT {} (avg {:.1}%) | NEUTRAL {}",
            thresh_label, helped, avg_help, hurt, avg_hurt, neutral);
    }

    // Write CSV
    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }

    eprintln!("\nCSV: {}", CSV_OUT);
    eprintln!("Runtime: {:?}", t0.elapsed());

    Ok(())
}

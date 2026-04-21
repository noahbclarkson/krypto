//! ATR_ENTRY_MULT Fine Sweep — Step 0.05 around current production params
//!
//! Full sweep: EM ∈ {0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95, 1.00, 1.05, 1.10} × 9 universes × all windows
//! With CURRENT production params: CHAND_PERIOD=7, CHAND_MULT=2.25, EP=24, HOLD_MAX=12
//!
//! Export equity curves for Baseline (EM=0.90), Winner, and runner-ups.
//! Usage: cargo run --example atr_entry_mult_fine_sweep --profile sweep

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
const CHAND_MULT: f64 = 2.25;
const TURTLE_ENTRY: usize = 24;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const CHAND_PERIOD: usize = 7;  // current production (2026-04-21)
const VOL_LOOKBACK: usize = 1;

// ATR_ENTRY_MULT values: step 0.05 fine sweep around 0.90 region
const ATR_EM_VALUES: &[f64] = &[
    0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95, 1.00, 1.05, 1.10
];

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

struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64> }

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

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal_with_em(close: &[f64], high: &[f64], low: &[f64],
                         entry_period: usize, atr_period: usize, atr_em: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    let Some(&curr_close) = close.get(idx) else { return false; };
    if curr_close <= max_close { return false; }
    if atr_em > 0.0 {
        let atr_val = atr_at(high, low, close, atr_period, idx);
        return curr_close >= max_close + atr_em * atr_val;
    }
    true
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
    ret: f64, sharpe: f64, max_dd: f64, trades: usize,
    win_rate: f64, pass: bool, equity_curve: Vec<f64>,
}

fn run_sim(sym_data: &HashMap<String, SymData>, symbols: &[String],
           atr_em: f64, test_start: usize, test_end: usize) -> WfResult {
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
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();
        if top_syms.is_empty() { equity_curve.push(equity); bar += 1; continue; }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal_with_em(&sd.close, &sd.high, &sd.low,
                                              TURTLE_ENTRY, TURTLE_ATR_PERIOD, atr_em, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b; break;
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
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }
                            if equity > peak { peak = equity; }
                            equity_curve.push(equity);
                            bar = exit_bar + 1; entered = true; break;
                        }
                    }
                }
            }
        }
        if !entered { equity_curve.push(equity); bar += 1; }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

fn resample_curve(curve: &[f64], target_len: usize) -> Vec<f64> {
    if curve.len() == target_len { return curve.to_vec(); }
    if curve.len() < 2 || target_len < 2 {
        let v = curve.first().copied().unwrap_or(1.0);
        return vec![v; target_len];
    }
    let mut out = Vec::with_capacity(target_len);
    for i in 0..target_len {
        let t = (i as f64) / (target_len - 1) as f64;
        let pos = t * (curve.len() - 1) as f64;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f64;
        let v = if idx + 1 < curve.len() {
            curve[idx] * (1.0 - frac) + curve[idx + 1] * frac
        } else { curve[idx] };
        out.push(v);
    }
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== ATR_ENTRY_MULT Fine Sweep: {} values × 9 universes × all windows ====", ATR_EM_VALUES.len());
    eprintln!("Params: CHAND(7,2.25), EP=24, HM=12, ATR(24,2.0), CAP=3");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES { for &s in *syms { all_syms.insert(s.to_string()); } }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw_cache.insert(sym.clone(), df); }
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
                close: col_vec!("close"), high: col_vec!("high"),
                low: col_vec!("low"), vol: col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Store results per EM value
    let mut agg_results: Vec<(f64, usize, usize, f64, f64, f64, usize)> = Vec::new();

    // Per-EM per-universe results for equity curves
    let mut global_results: HashMap<usize, Vec<WfResult>> = HashMap::new();

    for &em in ATR_EM_VALUES {
        eprintln!("--- ATR_ENTRY_MULT = {:.2} ---", em);
        let mut global_pass = 0usize;
        let mut global_total = 0usize;
        let mut all_sharpe = Vec::new();
        let mut all_ret = Vec::new();
        let mut global_trades = 0usize;

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, em, test_start, test_end);
                if r.pass { global_pass += 1; }
                global_total += 1;
                global_trades += r.trades;
                all_sharpe.push(r.sharpe);
                all_ret.push(r.ret);
            }
        }

        let avg_sharpe: f64 = all_sharpe.iter().sum::<f64>() / all_sharpe.len().max(1) as f64;
        let avg_ret: f64 = all_ret.iter().sum::<f64>() / all_ret.len().max(1) as f64;
        let avg_dd: f64 = all_ret.iter().sum::<f64>() / 0.0_f64; // placeholder
        agg_results.push((em, global_pass, global_total, avg_sharpe, avg_ret, 0.0, global_trades));
        eprintln!("  EM={:.2}: {}/{} pass ({:.1}%), Sharpe {:.3}, {} trades",
            em, global_pass, global_total, global_pass as f64/global_total.max(1) as f64*100.0, avg_sharpe, global_trades);
    }

    // Sort by Sharpe to find winner
    agg_results.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());
    let winner_em = agg_results[0].0;

    // Write summary CSV
    let mut sum_f = File::create("snapshots/atr_entry_mult_fine_summary.csv")?;
    writeln!(sum_f, "em,global_pass,global_total,pass_pct,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades")?;
    for (em, gp, gt, sh, ret, dd, trades) in &agg_results {
        let pct = *gp as f64 / (*gt).max(1) as f64 * 100.0;
        writeln!(sum_f, "{:.2},{},{},{:.2},{:.4},{:.2},{:.2},{}", em, gp, gt, pct, sh, ret, dd, trades)?;
    }

    // Generate equity curves for charting
    // For each EM: run Base5 (universe 0) all windows, compound equity per window
    let mut equity_csv_lines = vec!["em,window_idx,equity_mult,ret_pct,sharpe,max_dd_pct,trades,pass".to_string()];
    for &em in ATR_EM_VALUES {
        let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|s| s.to_string()).collect();
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        let mut window_equities: Vec<f64> = Vec::new();

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            let r = run_sim(&sym_data_map, &base5_syms, em, test_start, test_end);
            let eq_val = r.equity_curve.last().copied().unwrap_or(1.0);
            window_equities.push(eq_val);

            equity_csv_lines.push(format!(
                "{:.2},{},{:.6},{:.2},{:.4},{:.2},{},{}",
                em, wi, eq_val, r.ret, r.sharpe, r.max_dd, r.trades, r.pass
            ));
        }

        eprintln!("  EM={:.2} Base5 windows: {} total equity mult: {:.4}x",
            em, window_equities.len(),
            window_equities.iter().product::<f64>());
    }

    let mut eq_f = File::create("snapshots/atr_entry_mult_fine_equity.csv")?;
    for line in &equity_csv_lines { writeln!(eq_f, "{}", line)?; }

    eprintln!("\n=== SUMMARY ===");
    eprintln!("{:>8} | {:>5} | {:>6} | {:>7} | {:>10} | {:>+10}",
        "EM", "PASS", "TOTAL", "PASS%", "AVG_SHARPE", "AVG_RET%");
    eprintln!("{}", "-".repeat(55));
    for (em, gp, gt, sh, ret, _, _) in &agg_results {
        let pct = *gp as f64 / (*gt).max(1) as f64 * 100.0;
        let marker = if (*em - winner_em).abs() < 0.001 { " ← WINNER" } else { "" };
        eprintln!("{:>8.2}{} | {:>5} | {:>6} | {:>6.1}% | {:>10.4} | {:>+10.1}",
            em, marker, gp, gt, pct, sh, ret);
    }
    eprintln!("\nWinner: ATR_ENTRY_MULT={:.2}", winner_em);
    eprintln!("Files: snapshots/atr_entry_mult_fine_summary.csv, snapshots/atr_entry_mult_fine_equity.csv");
    eprintln!("Chart: python3 charts/plot_atr_entry_mult_fine.py");
    eprintln!("Runtime: {:?}", t0.elapsed());

    Ok(())
}

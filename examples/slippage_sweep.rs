//! Turtle+Chandelier Slippage Sensitivity Hyperopt
//!
//! PURPOSE: Audit the hardcoded SLIPPAGE_BPS=10 assumption.
//! Full sweep: 0-100 bps step=5 (21 values) × 9 universes × 54 walk-forward windows.
//! Also exports equity curves for key slippage configs for charting.
//!
//! KEY QUESTIONS:
//!   1. Does the strategy remain viable at higher slippage (20-30 bps)?
//!   2. Would strategy selection change if we assumed 0 bps vs 20 bps?
//!   3. Is the current 10 bps assumption reasonable for live execution?
//!
//! CONTEXT from MEMORY.md:
//!   "Maker-fills ~70.6% (BTC70.2/ETH68.3/SOL73.5). Fee saving ~8.8bp/trade vs backtest."
//!   Live expected slippage: ~3-5 bps on entry (limit order at bar close),
//!   ~5-10 bps on Chandelier stop exit (market sell triggered by stop).
//!   Net: ~8-15 bps per round trip = 16-30 bps one-way.
//!   Backtest uses 10 bps symmetric — conservative vs live.

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
const TURTLE_ENTRY: usize = 24;
const TURTLE_ATR_PERIOD: usize = 24;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 1;
const TURTLE_ATR_MULT: f64 = 2.00;

// --- SLIPPAGE SWEEP PARAMETERS ---
const SLIPPAGE_MIN: f64 = 0.0;
const SLIPPAGE_MAX: f64 = 100.0;
const SLIPPAGE_STEP: f64 = 5.0;

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

const CSV_OUT: &str = "snapshots/slippage_sweep.csv";
const EQUITY_OUT: &str = "snapshots/slippage_sweep_equity.csv";

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

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
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
    slip_bps: f64,
) -> (WfResult, Vec<f64>) {
    let slip_pct = slip_bps / 10000.0;
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

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        // Slippage is additional cost on top of taker fee:
                        // Entry: pay more than close (slip on buy)
                        // Exit: receive less than close (slip on sell)
                        let slip_pct = slip_bps / 10000.0;
                        let entry = entry_px * (1.0 + slip_pct) * (1.0 + TAKER_FEE);
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
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - slip_pct) * (1.0 - TAKER_FEE);
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

    (WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Turtle+Chandelier Slippage Sensitivity Hyperopt ====");
    eprintln!("Sweep: {} to {} bps step={} ({} values)", SLIPPAGE_MIN, SLIPPAGE_MAX, SLIPPAGE_STEP,
        ((SLIPPAGE_MAX - SLIPPAGE_MIN) / SLIPPAGE_STEP) as usize + 1);
    eprintln!("9 universes × {} walk-forward windows\n", (CANDLES / TEST_BARS as u32));

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
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Build slippage values
    let mut slippage_values: Vec<f64> = Vec::new();
    let mut v = SLIPPAGE_MIN;
    while v <= SLIPPAGE_MAX + 0.001 {
        slippage_values.push(v);
        v += SLIPPAGE_STEP;
    }
    let n_slip = slippage_values.len();
    eprintln!("Testing {} slippage values: {:?}", n_slip,
        slippage_values.iter().take(5).collect::<Vec<_>>());

    // Build window boundaries
    let n_windows = (n.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    eprintln!("{} walk-forward windows\n", n_windows);

    // --- KEY EQUITY CONFIGURATIONS ---
    // For charting: 0 bps (ideal), 10 bps (current default), 30 bps (pessimistic live)
    let key_configs: Vec<(f64, &str)> = vec![
        (0.0, "0_bps"),
        (10.0, "10_bps"),
        (30.0, "30_bps"),
        (50.0, "50_bps"),
        (100.0, "100_bps"),
    ];

    // Results storage: slippage × universe × window
    let mut all_records = Vec::new();
    let mut csv_lines = vec!["slippage_bps,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    // Equity storage: slip_bps -> vector of (bar, equity) for key configs
    // Only collected for key_configs (5 values), not all 21 slip values
    let _key_equity_by_slip: Vec<Vec<(usize, f64)>> = key_configs.iter()
        .map(|_| Vec::new())
        .collect();

    let mut global_pass_by_slip: Vec<(usize, usize)> = slippage_values.iter()
        .map(|_| (0, 0))
        .collect();

    for &(label, symbols) in UNIVERSES {
        eprintln!("\n--- Universe: {} ---", label);
        let sym_strs: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();

        let mut universe_pass_by_slip: Vec<(usize, usize)> = slippage_values.iter()
            .map(|_| (0, 0)).collect();

        for w in 0..n_windows {
            let test_start = TRAIN_BARS + w * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);

            if test_end <= test_start + MIN_TRADES { break; }

            for slip_idx in 0..n_slip {
                let slip_bps = slippage_values[slip_idx];
                let (result, _eq_curve) = run_sim(&sym_data_map, &sym_strs, test_start, test_end, slip_bps);

                let (ref mut passes, ref mut total) = universe_pass_by_slip[slip_idx];
                *total += 1;
                if result.pass { *passes += 1; }

                let (ref mut gpasses, ref mut gtotal) = &mut global_pass_by_slip[slip_idx];
                if result.pass { *gpasses += 1; }
                *gtotal += 1;

                csv_lines.push(format!(
                    "{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    slip_bps, label, w, result.ret, result.sharpe, result.max_dd,
                    result.trades, result.win_rate, result.pass as i32
                ));
                all_records.push((slip_bps, label, w, result));
            }
        }

        // Print per-slip pass rates for this universe
        for slip_idx in 0..n_slip {
            let slip_bps = slippage_values[slip_idx];
            let (pass, total) = universe_pass_by_slip[slip_idx];
            eprintln!("  {} bps: {}/{} pass ({:.0}%)", slip_bps, pass, total,
                if total > 0 { pass as f64 / total as f64 * 100.0 } else { 0.0 });
        }
    }

    // Compute aggregated metrics per slippage value
    let mut agg_by_slip: Vec<(f64, f64, f64, f64, f64, f64, usize, usize)> = Vec::new();
    for slip_idx in 0..n_slip {
        let slip_bps = slippage_values[slip_idx];
        let (gpass, gtotal) = global_pass_by_slip[slip_idx];

        let slip_bps_key = slip_bps;
        let records: Vec<_> = all_records.iter()
            .filter(|(sb, _, _, _)| (*sb - slip_bps_key).abs() < 0.1)
            .collect();

        let avg_sharpe: f64 = records.iter().map(|(_, _, _, r)| r.sharpe).sum::<f64>() / records.len().max(1) as f64;
        let avg_ret: f64 = records.iter().map(|(_, _, _, r)| r.ret).sum::<f64>() / records.len().max(1) as f64;
        let avg_dd: f64 = records.iter().map(|(_, _, _, r)| r.max_dd).sum::<f64>() / records.len().max(1) as f64;
        let avg_trades: f64 = records.iter().map(|(_, _, _, r)| r.trades as f64).sum::<f64>() / records.len().max(1) as f64;
        let avg_winrate: f64 = records.iter().map(|(_, _, _, r)| r.win_rate).sum::<f64>() / records.len().max(1) as f64;

        agg_by_slip.push((slip_bps, avg_sharpe, avg_ret, avg_dd, avg_trades, avg_winrate, gpass, gtotal));
    }

    // Write sweep results CSV
    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("\nWrote: {}", CSV_OUT);

    // Write equity curves for key configs
    let mut eq_f = File::create(EQUITY_OUT)?;
    writeln!(eq_f, "slip_bps,bar,equity_mean,equity_min,equity_max")?;

    for &(slip, _label) in &key_configs {
        // Aggregate equity across all universe-window runs for this slippage
        // We need per-bar mean/min/max equity
        let _slip_records: Vec<_> = all_records.iter()
            .filter(|(sb, _, _, _)| (*sb - slip).abs() < 0.1)
            .collect();

        // For each slip value, run a full equity simulation per universe and aggregate
        let mut all_eq_by_bar: Vec<Vec<f64>> = Vec::new();

        for &(_universe_label, universe_symbols) in UNIVERSES {
            let sym_strs: Vec<String> = universe_symbols.iter().map(|&s| s.to_string()).collect();

            for w in 0..n_windows {
                let test_start = TRAIN_BARS + w * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end <= test_start + MIN_TRADES { break; }

                let (_, eq) = run_sim(&sym_data_map, &sym_strs, test_start, test_end, slip);
                all_eq_by_bar.push(eq);
            }
        }

        // Find max bar count
        let max_bars = all_eq_by_bar.iter().map(|eq| eq.len()).max().unwrap_or(0);

        for bar_idx in 0..max_bars {
            let vals: Vec<f64> = all_eq_by_bar.iter()
                .filter_map(|eq| eq.get(bar_idx).copied())
                .collect();

            if vals.is_empty() { continue; }

            let mean = vals.iter().sum::<f64>() / vals.len() as f64;
            let min_val = vals.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_val = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

            writeln!(eq_f, "{},{},{:.6},{:.6},{:.6}", slip, bar_idx, mean, min_val, max_val)?;
        }
    }
    eprintln!("Wrote: {}", EQUITY_OUT);

    // Print aggregate summary table
    eprintln!("\n========== AGGREGATE RESULTS ==========");
    eprintln!("{:>8} {:>8} {:>10} {:>8} {:>8} {:>10} {:>8}", "Slip(bps)", "Pass", "Sharpe", "Ret%", "DD%", "Trades", "WR%");
    for (slip_bps, avg_sharpe, avg_ret, avg_dd, avg_trades, avg_wr, gpass, gtotal) in &agg_by_slip {
        let pass_pct = if *gtotal > 0 { *gpass as f64 / *gtotal as f64 * 100.0 } else { 0.0 };
        eprintln!("{:>8.0} {}/{} ({:>5.1}%) Sharpe={:.4} Ret={:.2}% DD={:.2}% Trades={:.1} WR={:.2}%",
            slip_bps, gpass, gtotal, pass_pct, avg_sharpe, avg_ret, avg_dd, avg_trades, avg_wr);
    }

    // Determine winner (best pass rate, then best Sharpe among top pass rate)
    let best_slip = {
        let mut best = (0.0_f64, 0_usize, f64::NEG_INFINITY);
        for (slip_bps, avg_sharpe, _, _, _, _, gpass, _) in &agg_by_slip {
            if *gpass > best.1 || (*gpass == best.1 && *avg_sharpe > best.2) {
                best = (*slip_bps, *gpass, *avg_sharpe);
            }
        }
        best.0
    };
    eprintln!("\nWINNER (best pass rate + Sharpe): {} bps", best_slip);

    // Also find best by Sharpe only (ignoring pass rate)
    let best_by_sharpe = {
        let mut best = (0.0_f64, f64::NEG_INFINITY);
        for (slip_bps, avg_sharpe, _, _, _, _, gpass, gtotal) in &agg_by_slip {
            if *gtotal > 0 {
                let pass_pct = *gpass as f64 / *gtotal as f64;
                // Only consider if pass rate >= 70%
                if pass_pct >= 0.70 && *avg_sharpe > best.1 {
                    best = (*slip_bps, *avg_sharpe);
                }
            }
        }
        best.0
    };
    eprintln!("WINNER (best Sharpe @ >=70% pass): {} bps", best_by_sharpe);

    let elapsed = t0.elapsed();
    eprintln!("\nTotal time: {:.1}s", elapsed.as_secs_f64());

    Ok(())
}

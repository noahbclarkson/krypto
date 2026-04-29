//! ATR_EMA_PERIOD Extensive Hyperopt — Full Range 1–200
//!
//! Tests EMA smoothing of Chandelier ATR values across the FULL integer range [1..200].
//! ATR naturally fluctuates daily; EMA smoothing before the Chandelier stop may reduce
//! stop-hopping noise for longer-term trend-following.
//!
//! ATR_EMA_PERIOD=1 → raw SMA ATR (baseline, no smoothing).
//! ATR_EMA_PERIOD>1 → EMA smoothing of computed ATR values.
//!
//! Current production params: EP=21, CHAND_P=7, CHAND_M=2.30, ATR_P=24,
//! ATR_M=2.0, ATR_ENTRY_MULT=0.00, HOLD_MAX=12, POSITION_CAP=3, VOL_LOOKBACK=8.
//!
//! PRIOR SWEEP: ATR_EMA 1-30 on STALE params (CHAND_P=20, CHAND_M=2.15, HM=45).
//! This sweep: ATR_EMA 1-200 on CURRENT production params + full 9-universe × 6-window.
//!
//! cargo run --example atr_ema_extensive_sweep --profile sweep

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
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;

// ATR_EMA range: 1 to 200 step 1 (200 values)
const ATR_EMA_MIN: usize = 1;
const ATR_EMA_MAX: usize = 200;

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

const SUMMARY_CSV: &str = "snapshots/atr_ema_extensive_sweep.csv";
const EQUITY_CSV: &str = "snapshots/atr_ema_extensive_equity.csv";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn true_range(high: f64, low: f64, prev_close: f64) -> f64 {
    (high - low).max((high - prev_close).abs()).max((low - prev_close).abs())
}

// Standard ATR (SMA of TR values)
fn atr_sma(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0_f64;
    for i in (idx + 1 - period)..=idx {
        let pc = close.get(i.saturating_sub(1)).copied().unwrap_or(close[i]);
        sum += true_range(high[i], low[i], pc);
    }
    sum / period as f64
}

// EMA smoothing of a vector of values
fn ema(vals: &[f64], period: usize) -> f64 {
    if vals.is_empty() { return 0.0; }
    if vals.len() == 1 { return vals[0]; }
    if period == 1 { return vals.iter().sum::<f64>() / vals.len() as f64; }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut e = vals[0];
    for &v in &vals[1..] {
        e = alpha * v + (1.0 - alpha) * e;
    }
    e
}

// Pre-compute EMA-smoothed ATR for a given EMA period across the full test series.
// For bar i, uses ATR values from (i - warmup + 1) to i, applies EMA with the given period.
fn precompute_ema_atr_series(
    high: &[f64], low: &[f64], close: &[f64],
    raw_atr_period: usize,
    ema_period: usize,
) -> Vec<f64> {
    let n = close.len();
    // First compute raw ATR for the full series (we need warmup bars)
    let mut raw_atr = vec![0.0_f64; n];
    for i in 0..n {
        raw_atr[i] = atr_sma(high, low, close, raw_atr_period, i);
    }
    // Now compute EMA-smoothed ATR at each bar
    // For bar i, we need ATR values from max(0, i - ema_period + 1) to i
    let mut ema_atr = vec![0.0_f64; n];
    for i in 0..n {
        let start = if i >= ema_period - 1 { i + 1 - ema_period } else { 0 };
        let window = &raw_atr[start..=i];
        ema_atr[i] = ema(window, ema_period);
    }
    ema_atr
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(
    close: &[f64], high: &[f64], low: &[f64],
    entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize,
) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_sma(high, low, close, atr_period, idx);
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
    final_equity: f64,
}

/// Run walk-forward for one ATR_EMA_PERIOD value, one universe, one window.
/// Uses EMA-smoothed Chandelier ATR (ema_period=atr_ema_period) for the trailing stop.
/// Turtle ATR remains raw SMA.
fn run_sim_ema_atr(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_ema_period: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    // Pre-compute EMA-ATR series for each symbol
    let mut ema_atr_series: HashMap<String, Vec<f64>> = HashMap::new();
    for sym in symbols {
        if let Some(sd) = sym_data.get(sym) {
            let test_high = &sd.high[test_start..test_end];
            let test_low  = &sd.low[test_start..test_end];
            let test_close = &sd.close[test_start..test_end];
            ema_atr_series.insert(sym.clone(),
                precompute_ema_atr_series(test_high, test_low, test_close, CHAND_PERIOD, atr_ema_period));
        }
    }

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume
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

        // Turtle breakout entry
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Precomputed EMA-ATR series for this symbol
                        let ema_atr = ema_atr_series.get(sym).map(|v| v.as_slice()).unwrap_or(&[]);

                        // DUAL_EXIT: Chandelier (EMA-ATR) OR Turtle ATR (raw) — whichever fires first
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            // Chandelier ATR: EMA-smoothed
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = *ema_atr.get(b - test_start).unwrap_or(&0.0);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            // Turtle ATR: raw SMA
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_sma(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            // Exit on EITHER stop
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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, final_equity: equity }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== ATR_EMA_PERIOD Extensive Sweep: {} to {} (step 1) ====", ATR_EMA_MIN, ATR_EMA_MAX);
    eprintln!("Production params: EP={}, CHAND_P={}, CHAND_M={}, ATR_P={}, HM={}, CAP={}",
        TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, HOLD_MAX, POSITION_CAP);
    eprintln!("9 universes × 6 windows = 54 runs per ATR_EMA value");
    eprintln!("Total: {} ATR_EMA values × 54 runs = {} runs\n",
        ATR_EMA_MAX - ATR_EMA_MIN + 1, (ATR_EMA_MAX - ATR_EMA_MIN + 1) * 54);

    // Load data
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
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
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

    // Global accumulators per ATR_EMA value
    let mut global_pass: Vec<usize> = vec![0; ATR_EMA_MAX + 1];
    let mut global_trades: Vec<usize> = vec![0; ATR_EMA_MAX + 1];
    let mut global_sharpe: Vec<f64> = vec![0.0; ATR_EMA_MAX + 1];
    let mut global_ret: Vec<f64> = vec![0.0; ATR_EMA_MAX + 1];
    let mut global_dd: Vec<f64> = vec![0.0; ATR_EMA_MAX + 1];

    // Equity tracking for selected ATR_EMA values
    // We'll identify top 3 after collecting summary stats
    // Equity for each (ema, universe, window) = final equity
    let mut equity_map: HashMap<(usize, usize, usize), f64> = HashMap::new();
    // (atr_ema_period, uni_idx, window_idx) -> final_equity

    let mut csv_lines = vec!["atr_ema_period,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass,final_equity".to_string()];

    // Progress bar
    let total_runs = (ATR_EMA_MAX - ATR_EMA_MIN + 1) * UNIVERSES.len() * 6;
    let mut completed_runs = 0usize;

    for (uni_idx, &(universe_name, symbols)) in UNIVERSES.iter().enumerate() {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing data)", universe_name);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (not enough data)", universe_name);
            continue;
        }

        eprintln!("\n==== {:<18} ====", universe_name);

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            // Run all ATR_EMA values for this universe-window
            for atr_ema in ATR_EMA_MIN..=ATR_EMA_MAX {
                let r = run_sim_ema_atr(&sym_data_map, &symbols, test_start, test_end, atr_ema);

                // Accumulate global stats
                if r.pass { global_pass[atr_ema] += 1; }
                global_trades[atr_ema] += r.trades;
                global_sharpe[atr_ema] += r.sharpe;
                global_ret[atr_ema] += r.ret;
                global_dd[atr_ema] += r.max_dd;

                // Store equity
                equity_map.insert((atr_ema, uni_idx, wi), r.final_equity);

                // CSV
                csv_lines.push(format!("{},{},{},{},{},{},{},{},{},{}",
                    atr_ema, universe_name, wi, r.ret, r.sharpe, r.max_dd, r.trades,
                    r.win_rate, if r.pass { 1 } else { 0 }, r.final_equity));

                completed_runs += 1;
                if completed_runs % 1000 == 0 {
                    eprintln!("  Progress: {}/{} runs", completed_runs, total_runs);
                }
            }

            // Print per-window stats for key ATR_EMA values
            if wi == 0 {
                eprintln!("  W{}: ema1_sharpe={:.3} pass={}, ema50_sharpe={:.3} pass={}, ema100_sharpe={:.3} pass={}",
                    wi,
                    global_sharpe[1] / ((wi + 1) * UNIVERSES.len()) as f64,
                    global_pass[1],
                    global_sharpe[50] / ((wi + 1) * UNIVERSES.len()) as f64,
                    global_pass[50],
                    global_sharpe[100] / ((wi + 1) * UNIVERSES.len()) as f64,
                    global_pass[100],
                );
            }
        }
    }

    // Global summary — rank ATR_EMA values by pass rate then Sharpe
    eprintln!("\n\n=== GLOBAL SUMMARY: TOP 20 ATR_EMA VALUES ===");
    eprintln!("{:>8} {:>6} {:>8} {:>10} {:>10}", "EMA_Period", "Pass", "Trades", "AvgSharpe", "AvgRet%");

    let n_windows_est = 6;
    let uni_count = UNIVERSES.len();
    let divisor = n_windows_est * uni_count;

    let mut ranked: Vec<(usize, usize, usize, f64)> = (ATR_EMA_MIN..=ATR_EMA_MAX)
        .map(|e| (e, global_pass[e], global_trades[e], global_sharpe[e] / divisor as f64))
        .collect();
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1) // pass rate desc
            .then_with(|| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Less)) // then Sharpe desc
    });

    for (ema, pass, trades, sharpe) in ranked.iter().take(20) {
        eprintln!("{:>8} {:>6} {:>8} {:>10.4} {:>10.4}", ema, pass, trades, sharpe, global_ret[*ema] / divisor as f64);
    }

    // Write CSVs
    {
        let mut f = File::create(SUMMARY_CSV)?;
        for line in &csv_lines { writeln!(f, "{}", line)?; }
        eprintln!("\nWrote: {}", SUMMARY_CSV);
    }

    // Equity CSV: baseline (1) + top 3 ATR_EMA values
    let top3: Vec<usize> = ranked.iter().take(3).map(|(e, _, _, _)| *e).collect();
    let equity_vals: Vec<usize> = std::iter::once(1).chain(top3.iter().copied()).collect();
    eprintln!("\nTop 3 ATR_EMA values: {:?}", top3);
    eprintln!("Equity export values: {:?}", equity_vals);

    {
        // Write: universe,window,bar_index,eq_1,eq_top1,eq_top2,eq_top3
        // We'll track average equity across symbols per window
        let mut eq_f = File::create(EQUITY_CSV)?;
        writeln!(eq_f, "universe,window,atr_ema,final_equity")?;
        for (uni_idx, &(universe_name, _)) in UNIVERSES.iter().enumerate() {
            for wi in 0..6 {
                for &e in &equity_vals {
                    if let Some(&eq) = equity_map.get(&(e, uni_idx, wi)) {
                        writeln!(eq_f, "{},{},{},{:.6}", universe_name, wi, e, eq)?;
                    }
                }
            }
        }
        eprintln!("Wrote: {}", EQUITY_CSV);
    }

    eprintln!("\nTotal runtime: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

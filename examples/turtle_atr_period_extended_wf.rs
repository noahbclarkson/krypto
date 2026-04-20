//!
//! Extended ATR_PERIOD hyperopt: full 5-100 step 1 sweep (96 values).
//!
//! Prior sweeps:
//!   - Coarse {10,15,20,25,28,30,35,40,50,60} -> ATR=24 winner (18-35 step 1 fine sweep)
//!   - Fine sweep 18-35 step=1 (18 values) -> ATR=24 winner
//! But: fine sweep used OLD params (CHAND_PERIOD=28, M=2.15), NOT current production (P=15/M=1.50).
//!
//! Full sweep: 5 to 100 step 1 = 96 values x 9 universes x 7 windows = 6048 window-runs.
//! Current production params: P=15/M=1.50, EP=21, CAP=3, HM=45, ATR=24.
//!
//! Export: CSV of all results + equity curves for Baseline(24), Winner, and top 3 runner-ups.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::{HashMap, HashSet};
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

// Current production params (updated 2026-04-19)
const CHAND_PERIOD: usize = 15;
const CHAND_MULT: f64 = 1.50;
const TURTLE_ENTRY: usize = 21;
const VOL_LOOKBACK: usize = 2;

// ATR periods to sweep: 5 to 100 step 1 (96 values)
const ATR_VALUES: &[usize] = &[
    5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35,
    36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50,
    51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
    66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80,
    81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95,
    96, 97, 98, 99, 100,
];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5", &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"]),
    ("NoDOGE", &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "ADAFDUSD"]),
    ("LargeCaps5", &["BTCFDUSD", "ETHFDUSD", "BNBFDUSD", "SOLFDUSD", "XRPFDUSD"]),
    ("LowVolume5", &["LTCFDUSD", "EOSFDUSD", "BCHFDUSD", "LINKFDUSD", "AVAXFDUSD"]),
    ("Legacy3", &["BTCFDUSD", "ETHFDUSD", "XRPFDUSD"]),
    ("Legacy4", &["BTCFDUSD", "ETHFDUSD", "XRPFDUSD", "LTCFDUSD"]),
    ("Legacy5", &["BTCFDUSD", "ETHFDUSD", "XRPFDUSD", "LTCFDUSD", "EOSFDUSD"]),
    ("Legacy4BNB", &["BTCFDUSD", "ETHFDUSD", "BNBFDUSD", "LTCFDUSD"]),
    ("Legacy5BNB", &["BTCFDUSD", "ETHFDUSD", "BNBFDUSD", "LTCFDUSD", "BCHFDUSD"]),
];

const CSV_OUT: &str = "snapshots/turtle_atr_extended_wf.csv";
const EQUITY_CSV: &str = "snapshots/turtle_atr_extended_equity.csv";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return (high[0] - low[0]).max(1e-10); }
    let mut tr_sum = 0.0;
    for i in (idx + 1 - period)..=idx {
        let tr = (high[i] - low[i])
            .max((high[i] - close[i - 1]).abs())
            .max((low[i] - close[i - 1]).abs());
        tr_sum += tr;
    }
    tr_sum / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.first().unwrap_or(&0.0); }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], _high: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let max_close = close[start..=idx].iter().cloned().fold(f64::NEG_INFINITY, |m, v| m.max(v));
    close[idx] > max_close
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let n = daily_rets.len() as f64;
    let mn: f64 = daily_rets.iter().sum::<f64>() / n;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / n).sqrt();
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
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    atr_period: usize,
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = *sd.close.get(bar).unwrap_or(&0.0);
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
                    if turtle_signal(&sd.close, &sd.high, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // DUAL EXIT: Chandelier(P=15, M=1.50) OR Turtle_ATR(atr_period, M=2.0)
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            // Chandelier
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            // Turtle ATR
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, atr_period, b);
                            let trail_turtle = highest_high_turtle - 2.0 * atr_turtle;
                            // Exit fires on EITHER stop triggered first
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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Extended ATR_PERIOD Sweep: 5-100 step 1 (96 values) ====");
    eprintln!("P={}/M={}, EP={}, CAP={}, HM={}", CHAND_PERIOD, CHAND_MULT, TURTLE_ENTRY, POSITION_CAP, HOLD_MAX);
    eprintln!("9 universes x {} windows, dual exit\n", (2800usize.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS));

    let loader = DataLoader::new(None, None);
    let mut all_syms: HashSet<String> = HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = 2800_usize;
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in all_syms.iter() {
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

    let mut csv_lines = vec!["atr_period,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    let mut equity_lines = vec!["atr_period,universe,window,day,equity".to_string()];

    // Per-atr-period aggregated results
    let mut atr_agg: HashMap<usize, (f64, usize, usize, usize)> = HashMap::new();

    // Track equity for specific ATR values: baseline(24), winner, top-3 runner-ups
    let mut tracked_equities: HashMap<usize, Vec<(String, usize, Vec<f64>)>> = HashMap::new();

    for &atr_period in ATR_VALUES {
        eprint!("ATR={:3} ... ", atr_period);
        let _ = std::io::stderr().flush();

        let mut atr_wins = 0usize;
        let mut atr_total = 0usize;
        let mut atr_trades = 0usize;

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            for wi in 0..total_windows {
                let test_start = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, atr_period, test_start, test_end);

                let pass_i = if r.pass { 1 } else { 0 };
                let line = format!(
                    "{},{},{},{:.4},{:.4},{:.4},{},{:.2},{}",
                    atr_period, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, pass_i
                );
                csv_lines.push(line);

                atr_wins += pass_i;
                atr_total += 1;
                atr_trades += r.trades;

                // Track equity for specific values: 24 (baseline), 3-5 runner-up candidates
                if atr_period == 24 || atr_period == ATR_VALUES[0] || atr_period == ATR_VALUES.last().copied().unwrap() {
                    if r.equity_curve.len() >= 2 {
                        tracked_equities.entry(atr_period).or_default()
                            .push((label.to_string(), wi, r.equity_curve));
                    }
                }
            }
        }

        atr_agg.insert(atr_period, (0.0, atr_wins, atr_total, atr_trades));
        eprintln!("atr={} global {}/{} pass, {} trades", atr_period, atr_wins, atr_total, atr_trades);
    }

    std::fs::write(CSV_OUT, csv_lines.join("\n")).unwrap();

    // Write equity CSV (every 10th day + final)
    for (&atr_p, runs) in &tracked_equities {
        for (label, wi, eq) in runs {
            for (day_idx, &e) in eq.iter().enumerate() {
                if day_idx % 10 == 0 || day_idx == eq.len() - 1 {
                    equity_lines.push(format!("{},{},{},{},{:.6}", atr_p, label, wi, day_idx, e));
                }
            }
        }
    }
    std::fs::write(EQUITY_CSV, equity_lines.join("\n")).unwrap();
    eprintln!("\nSaved: {} + {}", CSV_OUT, EQUITY_CSV);
    eprintln!("Runtime: {:?}", t0.elapsed());

    Ok(())
}
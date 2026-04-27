//! CHAND_PERIOD Walk-Forward Validation — P=5 vs P=7 vs P=10
//!
//! Run full 9-universe × 10-window walk-forward with 3 candidate values.
//! Confirm P=5 (best pass rate), P=7 (baseline), and P=10 (runner-up) robustness.

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
const TURTLE_ENTRY: usize = 21;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 9;

const VALIDATE_CP: &[usize] = &[5, 7, 10];

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

const SUMMARY_CSV: &str = "snapshots/chand_period_validation_summary.csv";
const DETAIL_CSV: &str = "snapshots/chand_period_validation_detail.csv";

struct SymData {
    close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64>,
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
    } else { false }
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

fn run_sim(sym_data: &HashMap<String, SymData>, symbols: &[String], test_start: usize, test_end: usize, chand_period: usize) -> (f64, f64, f64, usize, bool) {
    let mut equity = 1.0_f64;
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
        if top_syms.is_empty() { bar += 1; continue; }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
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
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, chand_period, b);
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
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }
        if !entered { bar += 1; }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&[1.0_f64]); // simplified
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    (ret, sharpe, max_dd, total_trades, pass)
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    let loader = DataLoader::new(None, None);
    let mut data: HashMap<String, SymData> = HashMap::new();

    let mut all_syms = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES { for s in *syms { all_syms.insert(s.to_string()); } }

    for sym in all_syms {
        match loader.fetch_with_cache(&sym, "1d", CANDLES).await {
            Ok(df) => {
                let close: Vec<f64> = df.column("close")?.f64()?.into_no_null_iter().collect();
                let high: Vec<f64> = df.column("high")?.f64()?.into_no_null_iter().collect();
                let low: Vec<f64> = df.column("low")?.f64()?.into_no_null_iter().collect();
                let vol: Vec<f64> = df.column("volume")?.f64()?.into_no_null_iter().collect();
                data.insert(sym, SymData { close, high, low, vol });
            }
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
        }
    }

    let min_len = UNIVERSES.iter().filter_map(|(_, s)| data.get(s[0]).map(|d| d.close.len())).min().unwrap_or(0);
    let n_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;

    println!("CHAND_PERIOD Validation: P ∈ {:?} × {} universes × {} windows", VALIDATE_CP, UNIVERSES.len(), n_windows);
    println!("Total runs: {} × {} × {} = {}", VALIDATE_CP.len(), UNIVERSES.len(), n_windows, VALIDATE_CP.len() * UNIVERSES.len() * n_windows);

    let mut summary_csv = File::create(SUMMARY_CSV)?;
    writeln!(summary_csv, "chand_period,universe,pass,total,pass_rate,avg_sharpe,avg_ret,total_trades")?;
    let mut detail_csv = File::create(DETAIL_CSV)?;
    writeln!(detail_csv, "chand_period,universe,window,ret,sharpe,trades,pass")?;

    // Per-universe per-window results
    let mut all_results: HashMap<usize, HashMap<String, Vec<(usize, f64, f64, usize, bool)>>> = HashMap::new();
    for &cp in VALIDATE_CP { all_results.insert(cp, HashMap::new()); }

    for (uname, symbols) in UNIVERSES {
        for cp in VALIDATE_CP {
            let syms: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let mut window_results: Vec<(usize, f64, f64, usize, bool)> = Vec::new();

            for w in 0..n_windows {
                let train_end = TRAIN_BARS + w * TEST_BARS;
                let test_end = (train_end + TEST_BARS).min(min_len);
                if test_end <= train_end + 30 { continue; }

                let (ret, sharpe, max_dd, trades, pass) = run_sim(&data, &syms, train_end, test_end, cp);
                writeln!(detail_csv, "{},{},{},{:.2},{:.3},{},{}", cp, uname, w, ret, sharpe, trades, pass)?;
                window_results.push((pass as usize, sharpe, ret, trades, pass));
            }

            if let Some(m) = all_results.get_mut(&cp) {
                m.insert(uname.to_string(), window_results);
            }
        }
    }

    // Print per-universe summary
    println!("\n=== Per-Universe Results ===");
    for (uname, symbols) in UNIVERSES {
        let syms_str = symbols.iter().map(|s| *s).collect::<Vec<_>>();
        print!("{:20s} | ", uname);
        for &cp in VALIDATE_CP {
            if let Some(uni_map) = all_results.get(&cp) {
                if let Some(wrs) = uni_map.get(uname) {
                    let pass = wrs.iter().map(|(p,_,_,_,_)| *p).sum::<usize>();
                    let total = wrs.len();
                    let avg_sh: f64 = wrs.iter().map(|(_,sh,_,_,_)| sh).sum::<f64>() / total as f64;
                    let avg_ret: f64 = wrs.iter().map(|(_,_,r,_,_)| r).sum::<f64>() / total as f64;
                    print!(" P={}: {}/{} ({:5.1f}%) Sharpe={:.3f} Ret={:.1f}% |", cp, pass, total, pass as f64/total as f64*100.0, avg_sh, avg_ret);
                }
            }
        }
        println!();
    }

    // Global summary
    println!("\n=== Global Summary ===");
    println!("{:>10} {:>8} {:>10} {:>10} {:>10}", "P", "PassRate", "AvgSharpe", "AvgRet%", "Trades");
    println!("{}", "-".repeat(48));

    let mut global_summary = Vec::new();
    for &cp in VALIDATE_CP {
        if let Some(uni_map) = all_results.get(&cp) {
            let total_pass: usize = uni_map.values().map(|wrs| wrs.iter().map(|(p,_,_,_,_)| p).sum::<usize>()).sum();
            let total_windows: usize = uni_map.values().map(|wrs| wrs.len()).sum();
            let avg_sharpe: f64 = uni_map.values().flat_map(|wrs| wrs.iter().map(|(_,sh,_,_,_)| sh)).sum::<f64>() / total_windows as f64;
            let avg_ret: f64 = uni_map.values().flat_map(|wrs| wrs.iter().map(|(_,_,r,_,_)| r)).sum::<f64>() / total_windows as f64;
            let total_trades: usize = uni_map.values().flat_map(|wrs| wrs.iter().map(|(_,_,_,t,_)| t)).sum();
            let pass_rate = total_pass as f64 / total_windows as f64 * 100.0;
            println!("{:10} {:7.1f}% {:10.3f} {:9.1}% {:10}", cp, pass_rate, avg_sharpe, avg_ret, total_trades);
            writeln!(summary_csv, "{},GLOBAL,{},{},{:.1},{:.3},{:.1},{}", cp, total_pass, total_windows, pass_rate, avg_sharpe, avg_ret, total_trades)?;
            global_summary.push((cp, pass_rate, avg_sharpe, avg_ret, total_trades));
        }
    }

    // Determine winner
    let baseline_cp = 7;
    let baseline = global_summary.iter().find(|(cp,_,_,_,_)| *cp == baseline_cp).unwrap();
    println!("\nBaseline P=7: pass={:.1f}%, Sharpe={:.3f}", baseline.1, baseline.2);
    for (cp, pr, sh, ret, _) in &global_summary {
        if *cp == baseline_cp { continue; }
        let delta_pr = *pr - baseline.1;
        let delta_sh = (*sh - baseline.2) / baseline.2 * 100.0;
        println!("P={}: Δpass={:+.1f}pp, ΔSharpe={:+.2f}%", cp, delta_pr, delta_sh);
    }

    // Decision
    let winner = global_summary.iter().max_by_key(|(cp, pr, sh, _, _)| {
        let pr_score = *pr as i32;
        let sh_score = (*sh * 100.0) as i32;
        pr_score * 1000 + sh_score
    }).unwrap();

    println!("\nWINNER (robustness-first): P={} (pass={:.1f}%, Sharpe={:.3f})", winner.0, winner.1, winner.2);
    if winner.0 == &(baseline_cp as usize) {
        println!("No parameter change warranted. P=7 (baseline) remains optimal.");
    } else {
        println!("RECOMMENDATION: Change CHAND_PERIOD from 7 to {}", winner.0);
    }

    println!("\nRuntime: {:.1}s", start.elapsed().as_secs_f64());
    Ok(())
}
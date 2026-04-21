//! ATR_ENTRY_MULT Full Walk-Forward Validation
//!
//! PRIOR RESULT (6 windows, 2026-04-21):
//!   ATR_ENTRY_MULT=1.0 → 81.5% pass, Sharpe 2.62, +113% vs baseline
//!   But only 6 windows tested — NOT production-ready
//!
//! THIS SESSION:
//!   Full 54-window validation × 9 universes × 9 ATR_ENTRY_MULT values
//!   Based on the proven hold_max_prod_sweep.rs simulation engine
//!   Production engine: CHAND(11,2.25)/EP=24/ATR(24,2.0)/HM=12

#![allow(unused)]

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
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

const CHAND_PERIOD: usize = 11;
const CHAND_MULT: f64 = 2.25;
const TURTLE_ENTRY: usize = 24;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;
const FRESHNESS_COOLDOWN: usize = 0;

// Extended sweep: coarse + fine around 1.0
const ATR_ENTRY_VALUES: &[f64] = &[
    0.0,   // baseline (no ATR filter)
    0.50,  // coarse
    0.75,  // coarse
    0.90,  // fine near winner
    1.00,  // prior winner
    1.10,  // fine near winner
    1.25,  // coarse above
    1.50,  // coarse above
    2.00,  // sanity check
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

const CSV_OUT:     &str = "snapshots/atr_entry_mult_full.csv";
const EQUITY_OUT:  &str = "snapshots/atr_entry_mult_full_equity.csv";
const SUMMARY_OUT:  &str = "snapshots/atr_entry_mult_full_summary.csv";

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
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_breakout_max_close(close: &[f64], entry_period: usize, idx: usize) -> f64 {
    if idx < entry_period + 1 { return f64::NEG_INFINITY; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    max_close
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    close.get(idx).map_or(false, |&c| {
        turtle_breakout_max_close(close, entry_period, idx) >= c || c >= turtle_breakout_max_close(close, entry_period, idx)
    })
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 252f64.sqrt() / sd
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

/// ATR entry filter for Turtle breakout.
///
/// Returns whether the Turtle entry signal passes the ATR momentum filter.
/// The filter requires: close >= (breakout_level + ATR * atr_entry_mult)
///
/// This is a momentum confirmation: the price must be extending beyond
/// the normal ATR noise band above the breakout level to qualify.
fn atr_filter_passes(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    entry_period: usize,
    atr_period: usize,
    atr_entry_mult: f64,
    idx: usize,
) -> bool {
    if atr_entry_mult == 0.0 { return true; }

    let breakout_level = turtle_breakout_max_close(close, entry_period, idx);
    if breakout_level == f64::NEG_INFINITY { return false; }

    let atr = atr_at(high, low, close, atr_period, idx);
    if atr == 0.0 { return true; }

    let current_close = match close.get(idx) {
        Some(&c) if c > 0.0 => c,
        _ => return false,
    };

    let threshold = breakout_level + atr * atr_entry_mult;
    current_close >= threshold
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_entry_mult: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut last_exit_bar: HashMap<String, isize> = symbols.iter()
        .map(|s| (s.clone(), -999isize)).collect();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Dollar-volume ranking
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

        // Entry: find first ranked symbol with Turtle signal + ATR filter
        let mut entry_sym = None;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                let breakout_signal = sd.close.get(bar).map_or(false, |&c| {
                    let bc = turtle_breakout_max_close(&sd.close, TURTLE_ENTRY, bar);
                    c >= bc
                });
                if !breakout_signal { continue; }

                // ATR entry filter
                let atr_pass = atr_filter_passes(
                    &sd.close, &sd.high, &sd.low,
                    TURTLE_ENTRY, TURTLE_ATR_PERIOD, atr_entry_mult, bar
                );
                if !atr_pass { continue; }

                // Freshness filter
                let bars_since = bar as isize - last_exit_bar.get(sym).copied().unwrap_or(-999);
                if FRESHNESS_COOLDOWN > 0 && bars_since < FRESHNESS_COOLDOWN as isize { continue; }

                entry_sym = Some(sym.clone());
                break;
            }
        }

        let entry_price = entry_sym.as_ref()
            .and_then(|s| sym_data.get(s)?.close.get(bar).copied());

        if let Some(sym) = entry_sym {
            let sd = sym_data.get(&sym).unwrap();
            let mut highest_high = sd.high.get(bar).copied().unwrap_or(entry_price.unwrap_or(0.0));
            let mut lowest_low = sd.low.get(bar).copied().unwrap_or(entry_price.unwrap_or(0.0));

            let mut in_pos = true;
            let mut bars_held = 0usize;
            let mut exit_bar: Option<usize> = None;

            let mut b = bar + 1;
            while b < test_end && in_pos {
                bars_held += 1;
                if let Some(h) = sd.high.get(b) { highest_high = highest_high.max(*h); }
                if let Some(l) = sd.low.get(b) { lowest_low = lowest_low.min(*l); }

                // Chandelier exit (current production mechanism)
                let atr = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                if atr > 0.0 {
                    let chand_stop = highest_high - CHAND_MULT * atr;
                    let turtle_stop = lowest_low - TURTLE_ATR_MULT * atr;
                    let stop = chand_stop.max(turtle_stop);
                    if let Some(lo) = sd.low.get(b) {
                        if *lo <= stop {
                            in_pos = false;
                            exit_bar = Some(b);
                        }
                    }
                }

                // HOLD_MAX safety exit
                if bars_held >= HOLD_MAX {
                    in_pos = false;
                    exit_bar = Some(b);
                }

                b += 1;
            }

            if let (Some(ex_bar), Some(en_p)) = (exit_bar, entry_price) {
                let exit_price = sd.close.get(ex_bar).copied().unwrap_or(en_p);
                let gross_ret = (exit_price - en_p) / en_p;
                let net_ret = gross_ret - TAKER_FEE * 2.0;
                equity *= 1.0 + net_ret;
                total_trades += 1;
                if net_ret > 0.0 { wins += 1; }
                daily_rets.push(net_ret);
                last_exit_bar.insert(sym.clone(), ex_bar as isize);
            }
        }

        if equity > peak { peak = equity; }
        equity_curve.push(equity);
        bar += 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && equity > 0.0 && equity <= 100.0;
    WfResult {
        ret,
        sharpe: annualised_sharpe(&daily_rets),
        max_dd: max_dd_from(&equity_curve),
        trades: total_trades,
        win_rate,
        pass,
        equity_curve,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let start_time = Instant::now();

    let loader = DataLoader::new(None, None);

    let mut all_syms: HashSet<String> = HashSet::new();
    for &(_, syms) in UNIVERSES {
        for &s in syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let len = df.height();
                min_len = min_len.min(len);
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let n_windows = (n - TRAIN_BARS) / TEST_BARS;

    // Build SymData
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    let n_min = n;
    for sym in all_syms.iter() {
        if let Some(df) = raw_cache.get(sym) {
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

    println!("Data: {} bars, {} windows, {} universes, {} ATR values",
        n, n_windows, UNIVERSES.len(), ATR_ENTRY_VALUES.len());
    let total_runs = ATR_ENTRY_VALUES.len() * UNIVERSES.len() * n_windows;
    println!("Total window-runs: {}", total_runs);

    let mut csv_file = File::create(CSV_OUT)?;
    writeln!(csv_file, "atr_entry_mult,universe,window,train_end,test_end,ret_pct,sharpe,trades,win_rate_pct,max_dd_pct,pass")?;

    let mut equity_file = File::create(EQUITY_OUT)?;
    writeln!(equity_file, "atr_entry_mult,universe,window,step,equity")?;

    let mut summary: HashMap<String, (usize, usize, f64, f64, f64, usize)> = HashMap::new();

    for &atr_em in ATR_ENTRY_VALUES {
        for &(uname, syms) in UNIVERSES {
            let symbols: Vec<String> = syms.iter().map(|s| s.to_string()).collect();

            let mut uni_pass = 0usize;
            let mut uni_ret_sum = 0.0_f64;
            let mut uni_sharpe_sum = 0.0_f64;
            let mut uni_dd_sum = 0.0_f64;
            let mut uni_trades = 0usize;

            for w in 0..n_windows {
                let train_end = TRAIN_BARS + w * TEST_BARS;
                let test_start = train_end;
                let test_end = (train_end + TEST_BARS).min(n);

                if test_end - test_start < 50 { continue; }

                let result = run_sim(&sym_data_map, &symbols, test_start, test_end, atr_em);

                let pass = if result.pass { 1 } else { 0 };
                uni_pass += pass;
                uni_ret_sum += result.ret;
                uni_sharpe_sum += result.sharpe;
                uni_dd_sum += result.max_dd;
                uni_trades += result.trades;

                writeln!(csv_file, "{:.2},{},{},{},{},{:.4},{:.4},{},{:.4},{:.4},{}",
                    atr_em, uname, w, train_end, test_end,
                    result.ret, result.sharpe, result.trades, result.win_rate, result.max_dd, pass)?;

                // Equity: write sampled points (every ~10 bars)
                let step = (result.equity_curve.len() / 100).max(1);
                for (bi, &eq) in result.equity_curve.iter().enumerate().step_by(step) {
                    writeln!(equity_file, "{:.2},{},{},{},{:.8}", atr_em, uname, w, bi, eq)?;
                }
            }

            let n_valid = n_windows;
            let entry = summary.entry(format!("{:.2}", atr_em)).or_insert((0, 0, 0.0, 0.0, 0.0, 0));
            entry.0 += uni_pass;
            entry.1 += n_valid;
            entry.2 += uni_ret_sum;
            entry.3 += uni_sharpe_sum;
            entry.4 += uni_dd_sum;
            entry.5 += uni_trades;

            let pr = uni_pass as f64 / n_valid as f64 * 100.0;
            let ar = uni_ret_sum / n_valid as f64;
            let sh = uni_sharpe_sum / n_valid as f64;
            println!("  ATR_EM={:.2} {}: {}/{} pass ({:.1}%), Sharpe={:.3}, Ret={:.1}%, DD={:.1}%",
                atr_em, uname, uni_pass, n_valid, pr, sh, ar, uni_dd_sum / n_valid as f64);
        }
    }

    // Summary
    let mut sum_file = File::create(SUMMARY_OUT)?;
    writeln!(sum_file, "atr_entry_mult,pass,total,pass_pct,avg_sharpe,avg_ret_pct,avg_dd,total_trades,vs_baseline_pct")?;

    let base = summary.get("0.00").copied().unwrap_or((0,0,0.0,0.0,0.0,0));
    let base_sharpe = base.3 / base.1.max(1) as f64;

    let mut rows: Vec<(&String, &(usize, usize, f64, f64, f64, usize))> = summary.iter().collect();
    rows.sort_by(|a, b| {
        let na = a.0.parse::<f64>().unwrap_or(0.0);
        let nb = b.0.parse::<f64>().unwrap_or(0.0);
        na.partial_cmp(&nb).unwrap()
    });

    let mut winner_mult = 0.0_f64;
    let mut winner_sharpe = 0.0_f64;

    for (mult, &(pass, total, ret_sum, sharpe_sum, dd_sum, trades)) in &rows {
        let pass_pct = pass as f64 / total as f64 * 100.0;
        let avg_sharpe = sharpe_sum / total as f64;
        let avg_ret = ret_sum / total as f64;
        let avg_dd = dd_sum / total as f64;
        let vs_base = if base_sharpe > 0.0 {
            (avg_sharpe - base_sharpe) / base_sharpe * 100.0
        } else { 0.0 };

        writeln!(sum_file, "{:.2},{},{},{:.2},{:.4},{:.2},{:.2},{},{:.2}",
            mult.parse::<f64>().unwrap_or(0.0), pass, total, pass_pct,
            avg_sharpe, avg_ret, avg_dd, trades, vs_base)?;

        if avg_sharpe > winner_sharpe {
            winner_sharpe = avg_sharpe;
            winner_mult = mult.parse().unwrap_or(0.0);
        }
    }

    let vs_base_pct = if base_sharpe > 0.0 {
        (winner_sharpe - base_sharpe) / base_sharpe * 100.0
    } else { 0.0 };

    println!("\n=== WINNER: ATR_ENTRY_MULT = {:.2} (Sharpe {:.4}, vs baseline {:.4} = {:.1}% improvement) ===",
        winner_mult, winner_sharpe, base_sharpe, vs_base_pct);
    println!("Runtime: {:.1}s", start_time.elapsed().as_secs_f64());

    Ok(())
}

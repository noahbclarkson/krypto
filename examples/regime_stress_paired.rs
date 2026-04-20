//! Paired Regime Stress Test: P=11/M=2.25 (current) vs P=28/M=2.0 (prior validated)
//!
//! Runs BOTH parameter sets on the SAME pre-2021 windows.
//! Prior validated: P=28/M=2.0 passed 21/21. If new params also pass 21/21 with
//! comparable or better Sharpe → they are robust. If they fail or have materially
//! worse Sharpe → revert to P=28/M=2.0.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const CANDLES: u32 = 3000;

// OLD params (P=28/M=2.0) — validated 21/21 pass on pre-2021
const OLD_CHAND_PERIOD: usize = 28;
const OLD_CHAND_MULT: f64 = 2.00;

// NEW params (P=11/M=2.25) — found in today's hyperopt sweeps
const NEW_CHAND_PERIOD: usize = 11;
const NEW_CHAND_MULT: f64 = 2.25;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT",
    "DOGEUSDT", "ADAUSDT", "LTCUSDT", "EOSUSDT",
    "BNBUSDT", "BCHUSDT",
];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    time: Vec<i64>,
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

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    let ann_factor = (365.25 / daily_rets.len() as f64).sqrt();
    mn / sd * ann_factor
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    for &val in equity.iter() {
        if val > peak { peak = val; }
        let dd = (peak - val) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

fn run_backtest(
    close: &[f64], high: &[f64], low: &[f64],
    test_start_idx: usize, test_end_idx: usize,
    chand_p: usize, chand_m: f64,
) -> (bool, f64, f64, f64, i64) {
    if test_end_idx <= test_start_idx { return (false, 0.0, 0.0, 0.0, 0); }

    let mut equity: f64 = 1.0;
    let mut entry_price: f64 = 0.0;
    let mut bars_in_pos: usize = 0;
    let mut active: bool = false;
    let mut trade_count: i64 = 0;
    let mut equity_curve: Vec<f64> = Vec::new();

    for idx in test_start_idx..=test_end_idx {
        if !active {
            if turtle_signal(close, TURTLE_ENTRY, idx) {
                entry_price = close[idx];
                equity *= 1.0 - TAKER_FEE;
                active = true;
                bars_in_pos = 0;
            }
            equity_curve.push(equity);
        } else {
            bars_in_pos += 1;
            let ret = (close[idx] - entry_price) / entry_price;
            equity *= (1.0 + ret) * (1.0 - TAKER_FEE);

            let should_exit = if bars_in_pos >= HOLD_MAX {
                true
            } else {
                let entry_bar = idx - bars_in_pos;
                let highest_high_chand = high[entry_bar..=idx].iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v));
                let atr_chand = atr_at(high, low, close, chand_p, idx);
                let atr_turtle = atr_at(high, low, close, TURTLE_ATR_PERIOD, idx);
                let trail_chand = highest_high_chand - chand_m * atr_chand;
                let trail_turtle = highest_high_chand - 2.0 * atr_turtle;
                close[idx] < trail_chand || close[idx] < trail_turtle
            };

            if should_exit {
                active = false;
                entry_price = 0.0;
                trade_count += 1;
            }

            equity_curve.push(equity);
        }
    }

    let total_return = (equity - 1.0) * 100.0;
    let sh = annualised_sharpe(&equity_curve);
    let dd = max_dd(&equity_curve);
    let pass = trade_count >= MIN_TRADES as i64 && sh > 0.0;
    (pass, total_return, sh, dd, trade_count)
}

fn ms_to_year(ms: i64) -> i32 {
    chrono::DateTime::from_timestamp(ms / 1000, 0)
        .map(|dt| dt.format("%Y").to_string().parse().unwrap_or(0))
        .unwrap_or(0)
}

fn ms_to_date_str(ms: i64) -> String {
    chrono::DateTime::from_timestamp(ms / 1000, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    eprintln!("=== PAIRED REGIME STRESS TEST ===");
    eprintln!("OLD: Chandelier({}, {}),  NEW: Chandelier({}, {})\n",
        OLD_CHAND_PERIOD, OLD_CHAND_MULT, NEW_CHAND_PERIOD, NEW_CHAND_MULT);
    eprintln!("EP={}, ATR={}, HM={}\n", TURTLE_ENTRY, TURTLE_ATR_PERIOD, HOLD_MAX);

    let loader = DataLoader::new(None, None);
    let all_syms: std::collections::HashSet<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();
    let mut min_len = usize::MAX;

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
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
    for sym in all_syms.iter() {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            let time_col = df.column("time")?.datetime()?;
            let time: Vec<i64> = time_col.into_iter().filter_map(|x| x).take(n_min).collect();
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                time,
            });
        }
    }

    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Results: (symbol, phase, old_pass, new_pass, old_ret, new_ret, old_sh, new_sh, old_dd, new_dd, old_trades, new_trades)
    let mut all_results: Vec<(String, String, bool, bool, f64, f64, f64, f64, f64, f64, i64, i64)> = Vec::new();
    let mut phase_summary: HashMap<&str, Vec<(bool, bool, f64, f64)>> = HashMap::new();

    for &sym in SYMBOLS {
        let data = match sym_data_map.get(sym) {
            Some(d) => d,
            None => { continue; }
        };
        let nsym = data.close.len();
        if nsym < 600 { continue; }

        let first_date = data.time.first().map(|&ms| ms_to_date_str(ms)).unwrap_or_default();
        let last_date = data.time.last().map(|&ms| ms_to_date_str(ms)).unwrap_or_default();
        eprintln!("{sym:12}  {first_date} -> {last_date}  ({nsym} bars)",);

        // P1-2020: Train ≤ 2019, Test 2020
        let year_end_2019 = data.time.iter().position(|&ms| ms_to_year(ms) >= 2020).unwrap_or(0);
        let year_end_2020 = data.time.iter().position(|&ms| ms_to_year(ms) >= 2021).unwrap_or(nsym);
        if year_end_2019 > 300 && year_end_2020 > year_end_2019 + 100 && year_end_2020 < nsym {
            let (op, or_, osh, odd, otr) = run_backtest(&data.close, &data.high, &data.low, year_end_2019, year_end_2020 - 1, OLD_CHAND_PERIOD, OLD_CHAND_MULT);
            let (np, nr, nsh, ndd, ntr) = run_backtest(&data.close, &data.high, &data.low, year_end_2019, year_end_2020 - 1, NEW_CHAND_PERIOD, NEW_CHAND_MULT);
            let delta_sh = nsh - osh;
            eprintln!("  |- P1-2020: OLD {op} SH={osh:.2} R={or_:+.1}%  |  NEW {np} SH={nsh:.2} R={nr:+.1}%  |  ΔSH={delta_sh:+.2}");
            all_results.push((sym.to_string(), "P1-2020".to_string(), op, np, or_, nr, osh, nsh, odd, ndd, otr, ntr));
            phase_summary.entry("P1-2020").or_default().push((op, np, osh, nsh));
        }

        // P2-2021: Train ≤ 2020, Test 2021
        if year_end_2020 > year_end_2019 + 100 && year_end_2020 < nsym {
            let year_end_2021 = data.time.iter().position(|&ms| ms_to_year(ms) >= 2022).unwrap_or(nsym);
            if year_end_2021 > year_end_2020 + 100 {
                let (op, or_, osh, odd, otr) = run_backtest(&data.close, &data.high, &data.low, year_end_2020, year_end_2021 - 1, OLD_CHAND_PERIOD, OLD_CHAND_MULT);
                let (np, nr, nsh, ndd, ntr) = run_backtest(&data.close, &data.high, &data.low, year_end_2020, year_end_2021 - 1, NEW_CHAND_PERIOD, NEW_CHAND_MULT);
                let delta_sh = nsh - osh;
                eprintln!("  |- P2-2021: OLD {op} SH={osh:.2} R={or_:+.1}%  |  NEW {np} SH={nsh:.2} R={nr:+.1}%  |  ΔSH={delta_sh:+.2}");
                all_results.push((sym.to_string(), "P2-2021".to_string(), op, np, or_, nr, osh, nsh, odd, ndd, otr, ntr));
                phase_summary.entry("P2-2021").or_default().push((op, np, osh, nsh));
            }
        }

        // P3-2019: Train ≤ 2018, Test 2019 (pure bear)
        let year_end_2018 = data.time.iter().position(|&ms| ms_to_year(ms) >= 2019).unwrap_or(0);
        if year_end_2018 > 300 && year_end_2019 > year_end_2018 + 100 {
            let (op, or_, osh, odd, otr) = run_backtest(&data.close, &data.high, &data.low, year_end_2018, year_end_2019 - 1, OLD_CHAND_PERIOD, OLD_CHAND_MULT);
            let (np, nr, nsh, ndd, ntr) = run_backtest(&data.close, &data.high, &data.low, year_end_2018, year_end_2019 - 1, NEW_CHAND_PERIOD, NEW_CHAND_MULT);
            let delta_sh = nsh - osh;
            eprintln!("  |- P3-2019: OLD {op} SH={osh:.2} R={or_:+.1}%  |  NEW {np} SH={nsh:.2} R={nr:+.1}%  |  ΔSH={delta_sh:+.2}");
            all_results.push((sym.to_string(), "P3-2019".to_string(), op, np, or_, nr, osh, nsh, odd, ndd, otr, ntr));
            phase_summary.entry("P3-2019").or_default().push((op, np, osh, nsh));
        }
    }

    eprintln!("\n=== SUMMARY BY PHASE ===");
    eprintln!("{:10} {:25} {:8} {:8} {:10} {:10} {:8}", "Phase", "Label", "OLD pass", "NEW pass", "OLD SH", "NEW SH", "ΔSH");
    let phases = [
        ("P1-2020", "Train<=2019 / Test 2020"),
        ("P2-2021", "Train<=2020 / Test 2021"),
        ("P3-2019", "Train<=2018 / Test 2019 (bear)"),
    ];

    let mut total_old_pass = 0;
    let mut total_new_pass = 0;
    let mut total_runs = 0;
    let mut total_old_sh = 0.0;
    let mut total_new_sh = 0.0;

    for (phase, label) in phases {
        if let Some(results) = phase_summary.get(phase) {
            if results.is_empty() { continue; }
            let old_passes = results.iter().filter(|(op, _, _, _)| *op).count();
            let new_passes = results.iter().filter(|(_, np, _, _)| *np).count();
            let old_avg_sh: f64 = results.iter().map(|(_, _, osh, _)| osh).sum::<f64>() / results.len() as f64;
            let new_avg_sh: f64 = results.iter().map(|(_, _, _, nsh)| nsh).sum::<f64>() / results.len() as f64;
            let delta_sh = new_avg_sh - old_avg_sh;
            let total = results.len();
            total_old_pass += old_passes;
            total_new_pass += new_passes;
            total_runs += total;
            total_old_sh += old_avg_sh * total as f64;
            total_new_sh += new_avg_sh * total as f64;
            eprintln!("  {phase:10} {label:25} {old_passes:2}/{total:2} ({:5.1}%)  {new_passes:2}/{total:2} ({:5.1}%)  {old_avg_sh:+.2}  {new_avg_sh:+.2}  {delta_sh:+.2}",
                100.0 * old_passes as f64 / total as f64,
                100.0 * new_passes as f64 / total as f64);
        }
    }

    let total_old_avg_sh = total_old_sh / total_runs as f64;
    let total_new_avg_sh = total_new_sh / total_runs as f64;
    let global_delta_sh = total_new_avg_sh - total_old_avg_sh;
    eprintln!("\n=== GLOBAL ===");
    eprintln!("  OLD: {total_old_pass:2}/{total_runs:2} pass ({:.0}%)  avg_sharpe={:.2}", 100.0 * total_old_pass as f64 / total_runs as f64, total_old_avg_sh);
    eprintln!("  NEW: {total_new_pass:2}/{total_runs:2} pass ({:.0}%)  avg_sharpe={:.2}", 100.0 * total_new_pass as f64 / total_runs as f64, total_new_avg_sh);
    eprintln!("  ΔSH  = {global_delta_sh:+.2}  (positive = new better)");

    // Verdict
    eprintln!("\n=== VERDICT ===");
    if total_new_pass == total_old_pass && global_delta_sh > -0.1 {
        eprintln!("  ✅ NEW params EQUIVALENT OR BETTER on pre-2021 stress.");
        eprintln!("     Pass rate unchanged ({}), Sharpe Δ = {:.2}", total_new_pass, global_delta_sh);
    } else if total_new_pass < total_old_pass {
        eprintln!("  ⚠️  NEW params FEWER PASSES on pre-2021 ({} vs {}).", total_new_pass, total_old_pass);
        eprintln!("     Sharpe Δ = {:.2}. Consider reverting.", global_delta_sh);
    } else {
        eprintln!("  ✅ NEW params MORE PASSES AND BETTER Sharpe on pre-2021.");
    }

    let mut csv = File::create("snapshots/regime_stress_paired.csv")?;
    writeln!(csv, "symbol,phase,old_pass,new_pass,old_ret_pct,new_ret_pct,old_sharpe,new_sharpe,old_dd,new_dd,old_trades,new_trades")?;
    for (sym, phase, op, np, or_, nr, osh, nsh, odd, ndd, otr, ntr) in &all_results {
        writeln!(csv, "{sym},{phase},{op},{np},{:.2},{:.2},{:.4},{:.4},{:.2},{:.2},{otr},{ntr}", or_, nr, osh, nsh, odd, ndd)?;
    }

    eprintln!("\nDone in {:.1}s. CSV: snapshots/regime_stress_paired.csv", start.elapsed().as_secs_f32());
    Ok(())
}

//! Regime Stress Test: Pre-2021 Held-Out Validation
//!
//! Tests Turtle+Chandelier (P=28, M=2.0, EP=21, CAP=3, HM=45, ATR=25)
//! against pre-optimization periods — data our parameters were NEVER trained on.

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
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 25;
const CANDLES: u32 = 3000;

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
) -> (bool, f64, f64, f64, i64) {
    if test_end_idx <= test_start_idx { return (false, 0.0, 0.0, 0.0, 0); }

    let mut equity: f64 = 1.0; // multiplicative equity
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
                let highest_high_turtle = highest_high_chand;
                let atr_chand = atr_at(high, low, close, CHAND_PERIOD, idx);
                let atr_turtle = atr_at(high, low, close, TURTLE_ATR_PERIOD, idx);
                let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                let trail_turtle = highest_high_turtle - 2.0 * atr_turtle;
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
    eprintln!("=== REGIME STRESS TEST: Pre-2021 Held-Out Validation ===\n");
    eprintln!("Params: EP={}, Chand({}, {}), ATR={}, HM={}\n",
        TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, HOLD_MAX);

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

    let mut all_results: Vec<(String, String, bool, f64, f64, f64, f64, i64)> = Vec::new();
    let mut by_phase: HashMap<&str, Vec<(bool, f64)>> = HashMap::new();

    for &sym in SYMBOLS {
        let data = match sym_data_map.get(sym) {
            Some(d) => d,
            None => { continue; }
        };
        let nsym = data.close.len();
        if nsym < 600 { continue; }

        let first_date = data.time.first().map(|&ms| ms_to_date_str(ms)).unwrap_or_default();
        let last_date = data.time.last().map(|&ms| ms_to_date_str(ms)).unwrap_or_default();
        eprintln!("{sym:12}  {first_date} -> {last_date}  ({nsym} bars)");

        // ── Phase 1: Train ≤ 2019, Test 2020 ──────────────────────────────
        let year_end_2019 = data.time.iter().position(|&ms| ms_to_year(ms) >= 2020).unwrap_or(0);
        let year_end_2020 = data.time.iter().position(|&ms| ms_to_year(ms) >= 2021).unwrap_or(nsym);

        if year_end_2019 > 300 && year_end_2020 > year_end_2019 + 100 && year_end_2020 < nsym {
            let (pass, ret, sh, dd, trades) = run_backtest(
                &data.close, &data.high, &data.low,
                year_end_2019, year_end_2020 - 1,
            );
            eprintln!("  |- P1-2020: {pass} | R={ret:+.1}% SH={sh:.2} DD={dd:.1}% trades={trades}");
            all_results.push((sym.to_string(), "P1-2020".to_string(), pass, ret, sh, dd, 0.0, trades));
            by_phase.entry("P1-2020").or_default().push((pass, sh));
        }

        // ── Phase 2: Train ≤ 2020, Test 2021 ──────────────────────────
        if year_end_2020 > year_end_2019 + 100 && year_end_2020 < nsym {
            let year_end_2021 = data.time.iter().position(|&ms| ms_to_year(ms) >= 2022).unwrap_or(nsym);
            if year_end_2021 > year_end_2020 + 100 {
                let (pass, ret, sh, dd, trades) = run_backtest(
                    &data.close, &data.high, &data.low,
                    year_end_2020, year_end_2021 - 1,
                );
                eprintln!("  |- P2-2021: {pass} | R={ret:+.1}% SH={sh:.2} DD={dd:.1}% trades={trades}");
                all_results.push((sym.to_string(), "P2-2021".to_string(), pass, ret, sh, dd, 0.0, trades));
                by_phase.entry("P2-2021").or_default().push((pass, sh));
            }
        }

        // ── Phase 3: Train ≤ 2018, Test 2019 (pure bear) ─────────────
        let year_end_2018 = data.time.iter().position(|&ms| ms_to_year(ms) >= 2019).unwrap_or(0);
        if year_end_2018 > 300 && year_end_2019 > year_end_2018 + 100 {
            let (pass, ret, sh, dd, trades) = run_backtest(
                &data.close, &data.high, &data.low,
                year_end_2018, year_end_2019 - 1,
            );
            eprintln!("  |- P3-2019: {pass} | R={ret:+.1}% SH={sh:.2} DD={dd:.1}% trades={trades}");
            all_results.push((sym.to_string(), "P3-2019".to_string(), pass, ret, sh, dd, 0.0, trades));
            by_phase.entry("P3-2019").or_default().push((pass, sh));
        }
    }

    eprintln!("\n=== SUMMARY BY PHASE ===");
    let phases = [
        ("P1-2020", "Train<=2019 / Test 2020"),
        ("P2-2021", "Train<=2020 / Test 2021"),
        ("P3-2019", "Train<=2018 / Test 2019 (bear)"),
    ];

    for (phase, label) in phases {
        if let Some(results) = by_phase.get(phase) {
            if results.is_empty() { continue; }
            let passes = results.iter().filter(|(p, _)| *p).count();
            let total = results.len();
            let avg_sharpe: f64 = results.iter().map(|(_, s)| s).sum::<f64>() / total as f64;
            let pass_pct = 100 * passes / total;
            eprintln!("  {phase:10} [{label:30}]  {passes:2}/{total:2} pass ({pass_pct:3}%)  avg_sharpe={avg_sharpe:.2}");
        }
    }

    let total_runs = all_results.len();
    let total_passes = all_results.iter().filter(|x| x.2).count();
    let avg_sharpe: f64 = all_results.iter().map(|x| x.4).sum::<f64>() / total_runs as f64;
    let avg_ret: f64 = all_results.iter().map(|x| x.3).sum::<f64>() / total_runs as f64;
    let avg_dd: f64 = all_results.iter().map(|x| x.5).sum::<f64>() / total_runs as f64;
    let pct = if total_runs > 0 { 100.0 * total_passes as f64 / total_runs as f64 } else { 0.0 };
    eprintln!("\n=== GLOBAL ===");
    eprintln!("  {total_passes:2}/{total_runs:2} pass ({pct:.0}%)  avg_sharpe={avg_sharpe:.2}  avg_return={avg_ret:.1}%  avg_DD={avg_dd:.1}%");

    let mut csv = File::create("snapshots/regime_stress_test.csv")?;
    writeln!(csv, "symbol,phase,pass,return_pct,sharpe,max_dd_pct,trades")?;
    for (sym, phase, pass, ret, sh, dd, _, trades) in &all_results {
        writeln!(csv, "{sym},{phase},{pass},{:.2},{:.4},{:.2},{trades}", ret, sh, dd)?;
    }

    eprintln!("\nDone in {:.1}s. CSV: snapshots/regime_stress_test.csv", start.elapsed().as_secs_f32());
    Ok(())
}

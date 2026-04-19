//! Equity curve comparison for top 2D sweep configurations
//!
//! Runs equity curves for:
//! - Baseline: P=20, M=2.00 (prior default)
//! - Current:  P=20, M=2.15 (current default)
//! - Winner:    P=15, M=1.50 (new winner)
//! - Runner-up: P=15, M=1.55
//! on Base5 universe, full history

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

const SYMBOLS: [&str; 6] = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];

const CONFIGS: &[(&str, usize, f64)] = &[
    ("baseline_P20_M200", 20, 2.00),
    ("current_P20_M215",  20, 2.15),
    ("winner_P15_M150",   15, 1.50),
    ("runner_P15_M155",   15, 1.55),
];

const CSV_OUT: &str = "snapshots/chand_pm_equity_comparison.csv";

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

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) { curr_close > max_close } else { false }
}

fn run_equity(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    chand_p: usize,
    chand_m: f64,
) -> Vec<f64> {
    let mut equity = 1.0_f64;
    let n = sym_data.values().next().map(|sd| sd.close.len()).unwrap_or(0);
    let mut equity_curve = Vec::with_capacity(n);
    equity_curve.push(equity);

    for bar in 0..n {
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
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if entered { break; }
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let ns = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(ns.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(ns.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, chand_p, b);
                            let trail_chand = highest_high_chand - chand_m * atr_chand;

                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;

                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            equity *= 1.0 + gross_ret;
                        }
                        entered = true;
                    }
                }
            }
        }
        equity_curve.push(equity);
    }

    equity_curve
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    let loader = DataLoader::new(None, None);

    let symbols: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();
    println!("Loading {} symbols...", symbols.len());

    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    for sym in &symbols {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(candles) => {
                let n = candles.height().min(CANDLES as usize);
                let close: Vec<f64> = candles.column("close")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                let high: Vec<f64> = candles.column("high")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                let low:  Vec<f64> = candles.column("low")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                let vol:  Vec<f64> = candles.column("volume")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                sym_data.insert(sym.clone(), SymData { close, high, low, vol });
            }
            Err(e) => { eprintln!("  WARNING: {sym} load failed: {e}"); }
        }
    }

    let n = sym_data.values().next().map(|sd| sd.close.len()).unwrap_or(0);
    println!("{n} bars loaded. Running {} configs...", CONFIGS.len());

    // Run all configs
    let mut all_equities: Vec<(String, Vec<f64>)> = Vec::new();
    for (name, chand_p, chand_m) in CONFIGS {
        println!("  Running {name} (P={chand_p}, M={chand_m})...");
        let eq = run_equity(&sym_data, &symbols, *chand_p, *chand_m);
        let final_eq = eq.last().copied().unwrap_or(1.0);
        println!("    Final equity: {:.4}x", final_eq);
        all_equities.push((name.to_string(), eq));
    }

    // Write CSV: rows = bars, cols = config names
    let min_len = all_equities.iter().map(|(_, eq)| eq.len()).min().unwrap_or(0);
    let mut csv = File::create(CSV_OUT)?;
    // Header
    let names: Vec<String> = all_equities.iter().map(|(n, _)| n.clone()).collect();
    let header = format!("bar,{}\n", names.join(","));
    csv.write_all(header.as_bytes())?;

    for i in 0..min_len {
        let mut row = format!("{}", i);
        for (_, eq) in &all_equities {
            row.push_str(&format!(",{:.6}", eq[i]));
        }
        row.push('\n');
        csv.write_all(row.as_bytes())?;
    }

    println!("\nEquity comparison CSV: {CSV_OUT}");
    println!("Complete in {:.1}s", start.elapsed().as_secs_f64());
    Ok(())
}

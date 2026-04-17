//! Held-out validation: VOL_LOOKBACK=55 vs VOL_LOOKBACK=2 on W04 and W05 only.
//! The concern: VL=55 was found by expanding a coarse sweep (step=5, range 1-100 gave VL=2)
//! to a fine sweep (step=1, range 1-100) that found VL=55. This may be overfitting.
//!
//! Test: Run Base5 universe on W04 and W05 with VL=55 vs VL=2.
//! Compare Sharpe and portfolio composition differences.
//!
//! Usage: cargo run --example turtle_vol_heldout_test --profile sweep

use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const SYMBOLS: [&str; 5] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

const EP: usize = 21;
const CHAND_PERIOD: usize = 20;
const CHAND_MULT: f64 = 2.15;
const HOLD_MAX: usize = 45;
const POSITION_CAP: usize = 3;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const TAKER_FEE: f64 = 0.001;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== VOL_LOOKBACK Held-Out Audit: VL=55 vs VL=2 on W04/W05 ===\n");
    let loader = DataLoader::new(None, None);

    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    for sym in SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", 2000).await {
            Ok(df) => {
                let close = vec_from_series_f64(df.column("close")?);
                let high  = vec_from_series_f64(df.column("high")?);
                let low   = vec_from_series_f64(df.column("low")?);
                let vol   = vec_from_series_f64(df.column("volume")?);
                sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
            }
            Err(e) => eprintln!("WARNING: {} load failed: {}", sym, e)
        }
    }

    let n = sym_data.values().next().map(|sd| sd.close.len()).unwrap_or(0);
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    println!("Loaded {} bars, {} total windows\n", n, total_windows);

    println!("{:<6} {:>7} {:>10} {:>10} {:>8}", "", "Window", "VL=2 Ret%", "VL=55 Ret%", "Delta");
    println!("{}", "-".repeat(45));

    for wi in [3, 4] {
        if wi >= total_windows { continue; }
        let label = if wi == 3 { "W04" } else { "W05" };

        let test_start = TRAIN_BARS + wi * TEST_BARS;
        let test_end = (test_start + TEST_BARS).min(n);

        let ret_vl2  = run_window(&sym_data, test_start, test_end, 2);
        let ret_vl55 = run_window(&sym_data, test_start, test_end, 55);

        println!("{:<6} {:>7} {:>+10.1}% {:>+10.1}% {:>+8.1}%", label, wi, ret_vl2 * 100.0, ret_vl55 * 100.0, (ret_vl55 - ret_vl2) * 100.0);
    }

    // Aggregate across W04+W05
    let w04_start = TRAIN_BARS + 3 * TEST_BARS;
    let w04_end = (w04_start + TEST_BARS).min(n);
    let w05_start = TRAIN_BARS + 4 * TEST_BARS;
    let w05_end = (w05_start + TEST_BARS).min(n);

    let ret_vl2_04  = run_window(&sym_data, w04_start, w04_end, 2);
    let ret_vl55_04 = run_window(&sym_data, w04_start, w04_end, 55);
    let ret_vl2_05  = run_window(&sym_data, w05_start, w05_end.min(n), 2);
    let ret_vl55_05 = run_window(&sym_data, w05_start, w05_end.min(n), 55);

    let agg_vl2  = (ret_vl2_04 + ret_vl2_05)  / 2.0;
    let agg_vl55 = (ret_vl55_04 + ret_vl55_05) / 2.0;

    println!("\n{:<6} {:>7} {:>+10.1}% {:>+10.1}% {:>+8.1}%", "AVG", "W04+W05", agg_vl2 * 100.0, agg_vl55 * 100.0, (agg_vl55 - agg_vl2) * 100.0);

    if agg_vl55 > agg_vl2 {
        println!("\n✅ VL=55 holds on held-out (+{:.1}% avg return)", (agg_vl55 - agg_vl2) * 100.0);
        println!("   Overfitting concern: REDUCED but cannot eliminate");
    } else {
        let delta = (agg_vl2 - agg_vl55) * 100.0;
        println!("\n⚠️ VL=55 underperforms VL=2 on held-out ({:.1}% avg return gap)", delta);
        println!("   OVERFITTING SIGNAL: Revert to VL=2 from coarse sweep.");
    }

    Ok(())
}

fn run_window(sym_data: &HashMap<String, SymData>, test_start: usize, test_end: usize, vol_lookback: usize) -> f64 {
    let mut equity = 1.0;
    let mut bar = test_start;

    while bar + 2 < test_end {
        // Rank symbols by dollar volume (using vol_lookback)
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for (sym, sd) in sym_data.iter() {
            if bar >= sd.close.len() { continue; }
            let rol_vol = rolling_avg(&sd.vol, vol_lookback, bar);
            let price = *sd.close.get(bar).unwrap_or(&0.0);
            let dv = rol_vol * price;
            scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<&str> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect();

        if top_syms.is_empty() {
            bar += 1;
            continue;
        }

        // Turtle breakout entry
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(*sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, EP, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Dual exit: Chandelier OR Turtle ATR
                        let mut highest_chand = sd.high[entry_bar_next];
                        let mut highest_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            // Chandelier(20, 2.15)
                            highest_chand = highest_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_chand - CHAND_MULT * atr_chand;
                            // Turtle ATR(24, 2.0)
                            highest_turtle = highest_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_turtle - TURTLE_ATR_MULT * atr_turtle;

                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if exit_bar < n && exit_bar >= entry_bar_next {
                            let exit_px = sd.close[exit_bar];
                            let ret = (exit_px / entry_px - 1.0) - TAKER_FEE * 2.0;
                            equity *= 1.0 + ret;
                            entered = true;
                        }
                    }
                }
            }
        }

        bar += 1;
    }

    equity - 1.0  // return fraction
}

fn turtle_signal(close: &[f64], high: &[f64], entry_period: usize, bar: usize) -> bool {
    if bar < entry_period + 1 || bar >= close.len() { return false; }
    let entry = *close[bar+1-entry_period..=bar].iter().max_by(|a,b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0);
    close[bar] >= entry
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0;
    for i in (idx+1-period..=idx).rev() {
        let tr = high[i].max(close[i]) - low[i].min(close[i]);
        sum += tr;
    }
    sum / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if window == 0 { return vals.get(idx).copied().unwrap_or(0.0); }
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn vec_from_series_f64(s: &Series) -> Vec<f64> {
    s.f64().unwrap().into_iter().map(|x| x.unwrap_or(0.0)).collect()
}

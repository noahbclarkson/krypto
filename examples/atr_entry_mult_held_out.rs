//! ATR_ENTRY_MULT Held-Out Validation: EM=0.00 vs EM=0.85 vs EM=0.90
//! 
//! Tests on pre-2021 held-out data (never used in any sweep).
//! Win condition: winner must beat EM=0.00 by >3 windows on held-out phases.
//! 
//! Run: cargo run --example atr_entry_mult_held_out --profile sweep

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

const CANDLES: u32 = 4000;
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
const TURTLE_ATR_MULT: f64 = 2.0;
const VOL_LOOKBACK: usize = 1;

const HELD_OUT_PAIRS: &[(&str, usize, usize)] = &[
    // (symbol, train_end_bar, test_end_bar)
    // Pre-2021 phases from 2019-2020
    ("BTCUSDT",  700, 900),   // P1 2019
    ("ETHUSDT",  700, 900),   // P1 2019
    ("XRPUSDT",  700, 900),   // P1 2019
    ("BTCUSDT",  900, 1100),  // P2 2019
    ("ETHUSDT",  900, 1100),  // P2 2019
    ("XRPUSDT",  900, 1100),  // P2 2019
    ("BTCUSDT", 1100, 1260),  // P3 2019
    ("ETHUSDT", 1100, 1260),  // P3 2019
    ("XRPUSDT", 1100, 1260),  // P3 2019
    ("BTCUSDT", 1260, 1460),  // P1 2020
    ("ETHUSDT", 1260, 1460),  // P1 2020
    ("XRPUSDT", 1260, 1460),  // P1 2020
    ("BTCUSDT", 1460, 1660),  // P2 2020
    ("ETHUSDT", 1460, 1660),  // P2 2020
    ("XRPUSDT", 1460, 1660),  // P2 2020
    ("BTCUSDT", 1660, 1850),  // P3 2020
    ("ETHUSDT", 1660, 1850),  // P3 2020
    ("XRPUSDT", 1660, 1850),  // P3 2020
];

#[derive(Clone)]
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

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64],
    entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
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

fn run_sim(sd: &SymData, test_start: usize, test_end: usize, atr_entry_mult: f64) 
    -> (bool, f64, f64, usize) {
    let mut equity = 1.0_f64;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut bar = test_start;

    while bar + 2 < test_end && bar < sd.close.len().saturating_sub(1) {
        if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
            if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY,
                TURTLE_ATR_PERIOD, atr_entry_mult, bar) {
                let entry_px = sd.close[bar];
                let entry = entry_px * (1.0 - TAKER_FEE);
                let entry_bar_next = bar + 1;
                let n = sd.close.len();

                let mut highest_high_chand = sd.high[entry_bar_next.min(n-1)];
                let mut lowest_low_turtle = sd.low[entry_bar_next.min(n-1)];
                let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                let mut exit_bar = max_bar;

                for b in entry_bar_next..=max_bar {
                    if b >= n { break; }
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
                    let exit = exit_px * (1.0 - TAKER_FEE);
                    let gross_ret = exit / entry - 1.0;
                    wins += if gross_ret > 0.0 { 1 } else { 0 };
                    total_trades += 1;
                    equity *= 1.0 + gross_ret;
                    bar = exit_bar + 1;
                    continue;
                }
            }
        }
        bar += 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    (pass, ret, win_rate, total_trades)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("==== ATR_ENTRY_MULT Held-Out Validation: EM=0.00 vs EM=0.85 vs EM=0.90 ====");
    println!("Pre-2021 phases (never used in sweeps): P1-P3 2019, P1-P3 2020");
    println!("Params: EP=21, CP=7, CM=2.30, HM=12, CAP=3, ATR=24, TAM=2.0\n");

    let loader = DataLoader::new(None, None);
    let syms: Vec<String> = vec!["BTCUSDT", "ETHUSDT", "XRPUSDT"]
        .iter().map(|s| s.to_string()).collect();

    let mut all_data: HashMap<String, SymData> = HashMap::new();
    for sym in &syms {
        let df = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let close: Vec<f64> = df.column("close")?.f64()?.into_iter().filter_map(|x| x).collect();
        let high: Vec<f64> = df.column("high")?.f64()?.into_iter().filter_map(|x| x).collect();
        let low: Vec<f64>  = df.column("low")?.f64()?.into_iter().filter_map(|x| x).collect();
        let vol: Vec<f64>  = df.column("volume")?.f64()?.into_iter().filter_map(|x| x).collect();
        all_data.insert(sym.clone(), SymData { close, high, low, vol });
        println!("  Loaded {}: {} bars", sym, all_data.get(sym).unwrap().close.len());
    }
    println!();

    let configs: Vec<(f64, &str)> = vec![
        (0.00, "EM=0.00 (baseline)"),
        (0.85, "EM=0.85 (winner)"),
        (0.90, "EM=0.90 (runner-up)"),
    ];

    let mut results: HashMap<String, Vec<(bool, f64, f64, usize)>> = HashMap::new();
    for (_, label) in &configs {
        results.insert(label.to_string(), Vec::new());
    }

    for (sym, train_end, test_end) in HELD_OUT_PAIRS {
        if let Some(sd) = all_data.get(*sym) {
            println!("Testing {} (bars {}-{})", sym, train_end, test_end);
            for &(em, label) in &configs {
                let (pass, ret, wr, trades) = run_sim(sd, *train_end, *test_end, em);
                println!("  {}: pass={}, ret={:.1}%, wr={:.0}%, trades={}", 
                    label, pass, ret, wr, trades);
                results.get_mut(label).unwrap().push((pass, ret, wr, trades));
            }
            println!();
        }
    }

    // Aggregate
    println!("=== HELD-OUT SUMMARY ===");
    for &(em, label) in &configs {
        let runs = results.get(label).unwrap();
        let pass_count = runs.iter().filter(|&&(p, _, _, _)| p).count();
        let total = runs.len();
        let avg_ret: f64 = runs.iter().map(|&(_, r, _, _)| r).sum::<f64>() / total as f64;
        let avg_wr: f64 = runs.iter().map(|&(_, _, wr, _)| wr).sum::<f64>() / total as f64;
        let total_trades: usize = runs.iter().map(|&(_, _, _, t)| t).sum();
        let pass_rate = pass_count as f64 / total as f64 * 100.0;
        println!("{}: {}/{} pass ({:.0}%), avg_ret={:.1}%, avg_wr={:.0}%, total_trades={}",
            label, pass_count, total, pass_rate, avg_ret, avg_wr, total_trades);
    }

    println!("\nRuntime: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

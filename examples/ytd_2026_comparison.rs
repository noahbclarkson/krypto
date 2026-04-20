//! =========================================================
//! 2026 YTD Parameter Comparison
//! =========================================================
//!
//! Compares three Chandelier parameter sets on 2026 YTD data:
//!   - P=5/M=3.00 (tightest stop)
//!   - P=11/M=2.25 (current production)
//!   - P=15/M=1.50 (prior production)
//!
//! Single run: 2026-01-01 to 2026-04-20
//! Measures: total return, Sharpe, trade count, avg hold bars, per-symbol contribution

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];
const EP: usize = 21;
const ATR_P: usize = 24;
const ATR_M: f64 = 2.0;
const HOLD_MAX: usize = 45;
const CAP: usize = 3;
const TAKER_FEE: f64 = 0.001;

const PARAMS: &[(&str, usize, f64)] = &[
    ("P=5/M=3.00",  5,  3.00),
    ("P=11/M=2.25", 11, 2.25),
    ("P=15/M=1.50", 15, 1.50),
];

fn main() -> Result<()> {
    let mut loader = DataLoader::new();
    let mut fe = FeatureEngine::new();

    // Load data for all symbols
    let mut all_data: HashMap<String, DataFrame> = HashMap::new();
    let mut max_bars = 0;
    for sym in SYMBOLS {
        let df = loader.load_symbol(sym, 4000)?;
        all_data.insert(sym.to_string(), df.clone());
        let n = df.height();
        if n > max_bars { max_bars = n; }
    }

    // Find 2026-01-01 index in each symbol
    let start_idx = all_data["BTCUSDT"]
        .column("time")?
        .datetime()
        .ok()
        .map(|s| {
            s.to_index_iter()
                .position(|(_, t)| {
                    let dt = chrono.DateTime::from_timestamp_millis(t).unwrap();
                    dt.year() == 2026 && dt.month() == 1 && dt.day() == 1
                })
                .unwrap_or(0)
        })
        .unwrap_or(0);

    println!("2026 starts at bar {} of {} (BTC)", start_idx, max_bars);
    println!("Trading period: 2026-01-01 to 2026-04-20");
    println!("---");

    // Run each param set
    for (label, cp, cm) in PARAMS {
        let mut equity = 1.0;
        let mut daily_returns: Vec<f64> = Vec::new();
        let mut total_trades = 0;
        let mut total_hold_bars = 0.0;
        let mut symbol_stats: HashMap<String, (f64, i32, f64)> = HashMap::new();

        for sym in SYMBOLS {
            let df = all_data.get(sym).unwrap();
            let n = df.height();

            let warmup = EP.max(cp).max(ATR_P) + ATR_P;
            // slice from warmup to end of 2026 data
            let trade_start = warmup.max(start_idx);
            if trade_start >= n { continue; }

            let close = df.column("close")?.f64()?.to_vec();
            let high  = df.column("high")?.f64()?.to_vec();
            let low   = df.column("low")?.f64()?.to_vec();
            let vol   = df.column("volume")?.f64()?.to_vec();

            // Daily returns for this symbol
            let mut sym_equity = 1.0f64;
            let mut in_pos = false;
            let mut entry_bar = 0;
            let mut entry_price = 0.0;
            let mut turtle_high = 0.0;
            let mut chand_trail = 0.0f64;
            let mut bars_held = 0;
            let mut sym_trades = 0;
            let mut sym_holds = 0.0;

            // Track position per bar across all symbols
            for bar in trade_start..n {
                let c = close[bar];
                let h = high[bar];
                let l = low[bar];
                let v = vol[bar];

                // ATR (using all bars up to bar for true no-look-ahead)
                let atr_end = bar.min(ATR_P + ATR_P - 1);
                let atr_slice = &close[(bar.saturating_sub(ATR_P)..=bar)];
                let h_slice = &high[(bar.saturating_sub(ATR_P)..=bar)];
                let l_slice = &low[(bar.saturating_sub(ATR_P)..=bar)];
                let atr = h_slice.iter()
                    .zip(l_slice.iter())
                    .map(|(h, l)| h - l)
                    .fold(0.0f64, |m, x| m.max(x));
                let atr = if atr <= 0.0 { 1.0 } else { atr };

                // Turtle breakout signal
                let ep_start = bar.saturating_sub(EP);
                let max_close_prior = close[ep_start..bar].iter().fold(0.0f64, |m, &x| m.max(x));

                // Chandelier trailing stop
                let hh_c = high[bar.saturating_sub(*cp)..=bar].iter().fold(0.0f64, |m, &x| m.max(x));
                let atr_c = atr;
                let trail_c = hh_c - cm * atr_c;

                // Turtle ATR exit
                let atr_t_start = bar.saturating_sub(ATR_P);
                let max_low_atr = low[atr_t_start..bar].iter().fold(f64::INFINITY, |m, &x| m.min(x));
                let trail_t = max_low_atr - ATR_M * atr;

                if !in_pos {
                    // Entry signal at bar close
                    if c > max_close_prior {
                        in_pos = true;
                        entry_bar = bar;
                        entry_price = c;
                        turtle_high = h;
                    }
                } else {
                    // In position: check exits
                    let exit_by_chand = c < chand_trail;
                    let exit_by_turtle = l < trail_t;
                    let exit_by_time = bars_held >= HOLD_MAX;

                    let exit_price = if exit_by_chand {
                        chand_trail.max(l) // stop below trail
                    } else if exit_by_turtle {
                        trail_t.max(l)
                    } else if exit_by_time {
                        c
                    } else {
                        0.0
                    };

                    if exit_price > 0.0 {
                        let ret = (exit_price / entry_price - 1.0) - TAKER_FEE;
                        sym_equity *= 1.0 + ret;
                        sym_trades += 1;
                        sym_holds += bars_held as f64;
                        daily_returns.push(ret);
                        in_pos = false;
                    } else {
                        // Update trailing stop (no exit)
                        chand_trail = trail_c;
                        turtle_high = turtle_high.max(h);
                    }
                    bars_held += 1;
                }
            }

            let sym_final = sym_equity - 1.0;
            symbol_stats.insert(sym.to_string(), (sym_final, sym_trades, sym_holds / sym_trades.max(1) as f64));
            equity *= sym_equity;
            total_trades += sym_trades;
        }

        // Compute Sharpe from daily returns (approximate)
        let sharpe = if daily_returns.len() > 5 {
            let mean = daily_returns.iter().sum::<f64>() / daily_returns.len() as f64;
            let std  = (daily_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily_returns.len() as f64).sqrt();
            if std > 0.0 { mean / std * (252.0_f64.sqrt()) } else { 0.0 }
        } else { 0.0 };

        let avg_hold = if total_trades > 0 { total_hold_bars / total_trades as f64 } else { 0.0 };

        println!("{}", label);
        println!("  Equity:      {:.1%}", equity - 1.0);
        println!("  Sharpe:      {:.2}", sharpe);
        println!("  Trades:      {}", total_trades);
        println!("  Per-symbol:");
        for sym in SYMBOLS {
            if let Some((ret, trades, avg_h)) = symbol_stats.get(sym) {
                println!("    {}: {:+.1%} ({} trades, avg hold {:.1f} bars)", sym, ret, trades, avg_h);
            }
        }
        println!();
    }

    Ok(())
}

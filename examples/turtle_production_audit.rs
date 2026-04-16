//! Quick audit: does Base5 (BTC/ETH/SOL/XRP/DOGE/ADA) pass W04 and W05?
//! Specifically: does excluding LTC/EOS/BCH fix the LowVolume5 failures?
//! Run: cargo run --example turtle_production_audit --profile sweep
use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

// Production universe: no LTC, EOS, BCH
const PRODUCTION_UNIVERSE: &[&str] = &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];

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
    matches!(close.get(idx), Some(&c) if c > max_close)
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn run_sim(sym_data: &HashMap<String, SymData>, symbols: &[String], test_start: usize, test_end: usize) -> (f64, f64, f64, usize) {
    let mut equity = 1.0_f64;
    let mut daily_rets = Vec::new();
    let mut wins = 0usize;
    let mut total_trades = 0usize;

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0) * sd.close.get(bar).copied().unwrap_or(0.0);
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
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
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
    (ret, sharpe, 0.0, total_trades)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n==== Turtle+Chandelier Production Universe Audit ====");
    println!("Universe: {:?}", PRODUCTION_UNIVERSE);
    println!("Focus: W04 (2022 bear) + W05 (2022 crash) vs LowVolume5\n");

    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for &sym in PRODUCTION_UNIVERSE {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw_cache.insert(sym.to_string(), df); }
            Err(e) => eprintln!("  WARNING: {} load failed: {}", sym, e),
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in PRODUCTION_UNIVERSE {
        if let Some(df) = raw_cache.get::<str>(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.to_string(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }
    println!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let symbols: Vec<String> = PRODUCTION_UNIVERSE.iter().map(|s| s.to_string()).collect();
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    println!("Total windows: {}\n", total_windows);

    // Focus on W03 (mega-bull), W04 (bear), W05 (crash) — the critical windows
    let focus_windows = if total_windows >= 6 { vec![3, 4, 5] } else { vec![] };
    let all_windows = (0..total_windows).collect::<Vec<_>>();

    println!("{:>6} | {:>8} | {:>7} | {:>6} | {:>7}", "Window", "Return", "Sharpe", "Trades", "Result");
    println!("{}", "-".repeat(50));

    let mut pass_count = 0usize;
    let mut total_count = 0usize;
    let mut focus_pass = 0usize;
    let mut focus_total = 0usize;

    for wi in 0..total_windows {
        let test_start = TRAIN_BARS + wi * TEST_BARS;
        let test_end = (test_start + TEST_BARS).min(n);
        if test_end.saturating_sub(test_start) < 5 { continue; }

        let (ret, sharpe, _, trades) = run_sim(&sym_data_map, &symbols, test_start, test_end);
        let passed = trades >= MIN_TRADES && ret > 0.0;
        let mark = if passed { "PASS" } else { "FAIL" };
        let star = if focus_windows.contains(&wi) { " ***" } else { "" };

        println!("W{:02}    | {:>+8.1}% | {:>7.2} | {:>6} | {}{}", wi, ret, sharpe, trades, mark, star);

        total_count += 1;
        if passed { pass_count += 1; }
        if focus_windows.contains(&wi) {
            focus_total += 1;
            if passed { focus_pass += 1; }
        }
    }

    println!("\n==== SUMMARY ====");
    println!("All windows:   {}/{} pass ({:.0}%)", pass_count, total_count, 100.0*pass_count as f64/total_count as f64);
    println!("W03/W04/W05:  {}/{} pass ({:.0}%)", focus_pass, focus_total, 100.0*focus_pass as f64/focus_total as f64);
    println!("\nNote: W03 = 2021 mega-bull, W04 = 2022 bear chop, W05 = 2022 FTX crash");

    Ok(())
}

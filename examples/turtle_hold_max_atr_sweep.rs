//! Turtle HM×ATR Period Sensitivity: Does ATR Period matter at different HOLD_MAX?
//!
//! Finding so far: ATR_P 5-100 produces IDENTICAL results with HM=12.
//! The Turtle ATR stop never fires (HOLD_MAX always wins first).
//!
//! Test: sweep HOLD_MAX ∈ {12, 20, 30, 45, 60, 90} × ATR_P ∈ {5, 20, 24, 40, 60}
//! to find the HM threshold where ATR period starts to matter.

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
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;

const HOLD_VALUES: &[usize] = &[12, 20, 30, 45, 60, 90];
const ATR_VALUES: &[usize] = &[5, 20, 24, 40, 60];

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

struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64> }

struct UniverseSetup {
    name: String,
    symbols: Vec<String>,
    sym_data: HashMap<String, SymData>,
    windows: Vec<(usize, usize)>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high[i]; let l = low[i]; let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { vals[idx] } else { vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64 }
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx { if let Some(&c) = close.get(i) { max_close = max_close.max(c); } }
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

fn run_sim(sym_data: &HashMap<String, SymData>, symbols: &[String], test_start: usize, test_end: usize, atr_period: usize, hold_max: usize) -> (f64, f64, usize, bool) {
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
                let price = sd.close[bar];
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
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, atr_period, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + hold_max).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar {
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, atr_period, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_turtle { exit_bar = b; break; }
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
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    (ret, sharpe, total_trades, pass)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== HOLD_MAX × ATR_PERIOD Sensitivity Sweep ====\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES { for &s in *syms { all_syms.insert(s.to_string()); } }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }
    let n = min_len.min(2800);
    eprintln!("Loaded {} symbols, {} bars\n", raw_cache.len(), n);

    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"), high: col_vec!("high"),
                low: col_vec!("low"), vol: col_vec!("volume"),
            });
        }
    }

    let mut uni_setups: Vec<UniverseSetup> = Vec::new();
    for &(uname, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }
        let sym_data: HashMap<String, SymData> = symbols.iter()
            .filter_map(|s| sym_data_map.get(s).map(|sd| (s.to_string(), SymData {
                close: sd.close.clone(), high: sd.high.clone(),
                low: sd.low.clone(), vol: sd.vol.clone(),
            })))
            .collect();
        let windows: Vec<(usize, usize)> = (0..total_windows).filter_map(|wi| {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { None } else { Some((test_start, test_end)) }
        }).collect();
        uni_setups.push(UniverseSetup { name: uname.to_string(), symbols, sym_data, windows });
    }

    eprintln!("{:>8} | {:>6} | {:>5} | {:>9} | {:>8} | {:>7}", "HM", "ATR_P", "Pass", "AvgSharpe", "AvgRet%", "Trades");
    eprintln!("{}", "-".repeat(55));

    let mut csv_rows = vec!("hm,atr_period,pass_rate,total_pass,total_windows,avg_sharpe,avg_ret,total_trades".to_string());

    for &hm in HOLD_VALUES {
        for &ap in ATR_VALUES {
            let t1 = Instant::now();
            let mut total_pass = 0usize;
            let mut total_windows = 0usize;
            let mut sum_sharpe = 0.0_f64;
            let mut sum_ret = 0.0_f64;
            let mut sum_trades = 0usize;

            for us in &uni_setups {
                for &(ts, te) in &us.windows {
                    let (ret, sharpe, trades, pass) = run_sim(&us.sym_data, &us.symbols, ts, te, ap, hm);
                    total_pass += if pass { 1 } else { 0 };
                    total_windows += 1;
                    sum_sharpe += sharpe;
                    sum_ret += ret;
                    sum_trades += trades;
                }
            }

            let avg_sharpe = if total_windows > 0 { sum_sharpe / total_windows as f64 } else { 0.0 };
            let avg_ret = if total_windows > 0 { sum_ret / total_windows as f64 } else { 0.0 };
            let pass_rate = if total_windows > 0 { total_pass as f64 / total_windows as f64 * 100.0 } else { 0.0 };

            csv_rows.push(format!("{},{},{:.2},{:.0},{:.0},{:.4},{:.2},{}",
                hm, ap, pass_rate, total_pass, total_windows, avg_sharpe, avg_ret, sum_trades));

            eprintln!("HM={:>3}, ATR_P={:>3}: {:>5.0}% pass, Sharpe={:.3}, Ret={:+7.1}%, {} trades [{:.1}s]",
                hm, ap, pass_rate, avg_sharpe, avg_ret, sum_trades, t1.elapsed().as_secs_f64());
        }
    }

    let mut f = File::create("snapshots/turtle_hm_atr_sensitivity.csv")?;
    for row in &csv_rows { writeln!(f, "{}", row)?; }
    eprintln!("\nWrote: snapshots/turtle_hm_atr_sensitivity.csv");
    eprintln!("Total: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

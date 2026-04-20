//! CHAND_PERIOD Extended Sweep: Smart Grid (24 values)
//! Production params except CHAND_PERIOD: EP=21, ATR_P=24, ATR_M=2.0, CHAND_M=1.50, HM=45, CAP=3, VL=2
//! 9 universes x ~9 windows x 24 values = ~1900 window-runs
//!
//! Grid strategy:
//!   Coarse (13 values): 5,10,15,20,25,30,40,50,60,70,80,90,100  [full range]
//!   Fine (11 values): 12-22 step 1                                [tight around P=15]
//! Total: 24 values covering the full range + fine region around production default
//!
//! Exports: per-window results + equity curves for candidates

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
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const EP: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const CHAND_MULT: f64 = 1.50;
const VOL_LOOKBACK: usize = 2;

// Smart grid: coarse full-range + fine around production P=15
const CHAND_PERIODS: &[usize] = &[
    // Coarse: full logical range
     5, 10, 15, 20, 25, 30, 40, 50, 60, 70, 80, 90, 100,
    // Fine: around production P=15
    12, 13, 14, 15, 16, 17, 18, 19, 21, 22, 23,
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

const CSV_OUT: &str = "snapshots/chand_period_smart_wf.csv";
const EQUITY_OUT: &str = "snapshots/chand_period_smart_equity.csv";

struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64> }

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
    if idx + 1 < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).map(|&c| c > max_close).unwrap_or(false)
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct WfResult { ret: f64, sharpe: f64, max_dd: f64, trades: usize, win_rate: f64, pass: bool }

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    chand_p: usize,
) -> (WfResult, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
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

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, EP, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next.min(n.saturating_sub(1))];
                        let mut highest_high_turtle = sd.high[entry_bar_next.min(n.saturating_sub(1))];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar {
                            if b >= n { break; }
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, chand_p, b);
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
                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;

                            equity *= 1.0 + gross_ret;
                            daily_rets.push(gross_ret);

                            let bars_to_add = exit_bar.saturating_sub(entry_bar_next);
                            for _ in 0..bars_to_add {
                                equity_curve.push(equity);
                            }
                            equity_curve.push(equity);
                        }
                        entered = true;
                        break;
                    }
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
        }
        bar += 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES;

    (WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== CHAND_PERIOD Smart Sweep: 24 values (coarse + fine) ====");
    eprintln!("P=15/M=1.50 production, 9 universes x ~9 windows, dual exit\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
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
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let mut all_results: Vec<(String, usize, usize, f64, f64, f64, usize, f64, i32)> = Vec::new();
    let mut all_equity: Vec<(String, usize, usize, usize, f64)> = Vec::new();

    for &chand_p in CHAND_PERIODS {
        eprint!("CP={:3} ", chand_p);

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();

            let max_windows = {
                let lens: Vec<usize> = symbols.iter().filter_map(|s| sym_data_map.get(s).map(|sd| sd.close.len())).collect();
                let mn = *lens.iter().min().unwrap_or(&0);
                mn.saturating_sub(TRAIN_BARS + TEST_BARS)
            };

            for wi in 0..max_windows {
                let test_start = wi;
                let test_end = (wi + TRAIN_BARS + TEST_BARS).min(
                    symbols.iter().filter_map(|s| sym_data_map.get(s).map(|sd| sd.close.len())).min().unwrap_or(0)
                );

                if test_end <= test_start + 10 { break; }

                let (result, eq_curve) = run_sim(&sym_data_map, &symbols, test_start, test_end, chand_p);

                all_results.push((
                    label.to_string(),
                    chand_p,
                    wi,
                    result.ret,
                    result.sharpe,
                    result.max_dd,
                    result.trades,
                    result.win_rate,
                    if result.pass { 1 } else { 0 },
                ));

                // Sample equity curve
                let step = (eq_curve.len() as f64 / 252.0).ceil().max(1.0) as usize;
                for (di, &eq) in eq_curve.iter().enumerate() {
                    if di % step == 0 || di == eq_curve.len() - 1 {
                        all_equity.push((label.to_string(), chand_p, wi, di, eq));
                    }
                }
            }
        }
        eprintln!();
    }

    // Write results CSV
    {
        let mut f = File::create(CSV_OUT)?;
        writeln!(f, "universe,chand_p,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;
        for r in &all_results {
            writeln!(f, "{},{},{},{},{},{},{},{},{}",
                r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8)?;
        }
    }

    // Write equity CSV
    {
        let mut f = File::create(EQUITY_OUT)?;
        writeln!(f, "universe,chand_p,window,day,equity")?;
        for r in &all_equity {
            writeln!(f, "{},{},{},{},{}", r.0, r.1, r.2, r.3, r.4)?;
        }
    }

    eprintln!("\nRuntime: {:.1}s", t0.elapsed().as_secs_f64());
    eprintln!("Saved: {} + {}", CSV_OUT, EQUITY_OUT);

    // Summary
    eprintln!("\n=== Summary by CHAND_PERIOD ===");
    let mut summary: std::collections::HashMap<usize, (f64, usize, usize, usize)> = std::collections::HashMap::new();
    for r in &all_results {
        let entry = summary.entry(r.1).or_insert((0.0, 0, 0, 0));
        entry.0 += r.4; // sum sharpe
        entry.1 += r.6; // sum trades
        entry.2 += if r.8 == 1 { 1 } else { 0 }; // passes
        entry.3 += 1; // windows
    }
    let total_windows = UNIVERSES.len() * (n.saturating_sub(TRAIN_BARS + TEST_BARS)).min(9);

    let mut sorted: Vec<_> = summary.iter().collect();
    sorted.sort_by(|a, b| {
        let a_s = a.1 .0 / a.1 .1.max(1) as f64;
        let b_s = b.1 .0 / b.1 .1.max(1) as f64;
        b_s.partial_cmp(&a_s).unwrap()
    });

    for (&cp, &(sum_s, sum_trades, sum_pass, _cnt)) in sorted.iter() {
        let avg_s = sum_s / sum_trades.max(1) as f64;
        let pass_r = sum_pass as f64 / total_windows as f64 * 100.0;
        eprintln!("  CP={:3}: Sharpe={:.4}, Trades={}, Pass={:.0}%", cp, avg_s, sum_trades, pass_r);
    }

    Ok(())
}

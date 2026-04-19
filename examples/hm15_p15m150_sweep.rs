//! HOLD_MAX re-optimization with P=15/M=1.50 (new Chandelier params)
//!
//! Prior HOLD_MAX sweep (2026-04-11) was done with CHAND_PERIOD=28/MULT=2.0.
//! New Chandelier params (P=15/M=1.50) are significantly tighter.
//! This may change the optimal HOLD_MAX — tighter stops exit faster,
//! potentially allowing a LOOSER hold maximum without increasing drawdown.
//!
//! Sweep: HM ∈ {10,15,20,25,30,35,40,45,50,55,60,70,80,90,105,120,150,180}
//! Universe: Base5 (BTC,ETH,SOL,XRP,DOGE,ADA) — 6 windows

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

// Updated Chandelier params (2026-04-19)
const CHAND_PERIOD: usize = 15;
const CHAND_MULT: f64 = 1.50;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

const HOLD_MAX_VALUES: &[usize] = &[
    10, 15, 20, 25, 30, 35, 40, 45, 50, 55,
    60, 70, 80, 90, 105, 120, 150, 180,
];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5", &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
];

const CSV_OUT: &str = "snapshots/hm_sweep_p15m150.csv";

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
    mn * 365.0_f64.sqrt() / sd
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
    equity_final: f64,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hold_max: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let vol_lookback = 2usize;

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, vol_lookback, bar);
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
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + hold_max).min(n.saturating_sub(1));
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
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            if equity > peak { peak = equity; }
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    let equity_final = equity;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_final }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();
    eprintln!("==== HOLD_MAX Sweep with P=15/M=1.50 ====");
    eprintln!("Testing {} HOLD_MAX values on Base5 (6 windows)\n", HOLD_MAX_VALUES.len());

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
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

    // Run sweep
    let mut all_results: Vec<(usize, String, usize, WfResult)> = Vec::new();

    for &hm in HOLD_MAX_VALUES {
        eprintln!("  HM={}", hm);
        for (uname, symbols) in UNIVERSES {
            let n_windows = (n - TRAIN_BARS) / TEST_BARS;
            for w in 0..n_windows {
                let train_end = TRAIN_BARS + w * TEST_BARS;
                let test_start = train_end;
                let test_end = (train_end + TEST_BARS).min(n);

                if test_end - test_start < 100 { continue; }

                let result = run_sim(&sym_data_map, &symbols.iter().map(|s| s.to_string()).collect::<Vec<_>>(), test_start, test_end, hm);
                all_results.push((hm, uname.to_string(), w, result));
            }
        }
    }

    // Aggregate
    let mut csv = File::create(CSV_OUT)?;
    writeln!(csv, "hm,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass,equity_final")?;

    let mut agg: HashMap<usize, (f64, f64, f64, usize, usize, usize, usize)> = HashMap::new();

    for (hm, uname, win, r) in &all_results {
        writeln!(csv, "{},{},{},{:.2},{:.4},{:.2},{},{:.2},{},{:.4}",
            hm, uname, win, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass, r.equity_final)?;
        let e = agg.entry(*hm).or_insert((0.0, 0.0, 0.0, 0, 0, 0, 0));
        e.0 += r.ret;
        e.1 += r.sharpe;
        e.2 += r.max_dd;
        e.3 += r.trades;
        e.4 += if r.pass { 1 } else { 0 };
        e.5 += 1;
        e.6 += 1; // total windows
    }

    eprintln!("\n==== AGGREGATE RESULTS (Base5, 6 windows) ====");
    eprintln!("{:>5} | {:>7} | {:>7} | {:>7} | {:>5} | {:>4} | {:>5} | {:>8}", "HM", "AvgRet%", "AvgSh", "AvgDD%", "Trades", "Pass", "Pass%", "EquFinal");
    eprintln!("{}", "-".repeat(70));

    let mut results_sorted: Vec<(usize, f64, f64, f64, usize, usize, f64)> = Vec::new();
    for (&hm, &(sum_ret, sum_sh, sum_dd, trades, pass_cnt, total, _)) in &agg {
        let n_windows = total / UNIVERSES.len();
        let avg_ret = sum_ret / n_windows as f64;
        let avg_sh = sum_sh / n_windows as f64;
        let avg_dd = sum_dd / n_windows as f64;
        let pass_pct = pass_cnt as f64 / total as f64 * 100.0;
        // equity final: use first universe result
        let eq_final = all_results.iter()
            .find(|(h, u, _, _)| *h == hm && *u == "Base5")
            .map(|(_, _, _, r)| r.equity_final)
            .unwrap_or(1.0);
        results_sorted.push((hm, avg_ret, avg_sh, avg_dd, trades, pass_cnt, eq_final));
        eprintln!("{:>5} | {:>7.1} | {:>7.4} | {:>7.2} | {:>5} | {:>4} | {:>5.1}% | {:>8.4}",
            hm, avg_ret, avg_sh, avg_dd, trades / (total as f64 / n_windows as f64) as usize, pass_cnt, pass_pct, eq_final);
    }

    // Sort by Sharpe descending
    results_sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    eprintln!("\n==== WINNER (by Sharpe) ====");
    if let Some((hm, ret, sh, dd, trades, passes, eq)) = results_sorted.first() {
        eprintln!("HM={}: Sharpe={:.4}, AvgRet={:.1}%, AvgDD={:.2}%, Pass={}/6 ({:.0}%), Equity={:.4}",
            hm, sh, ret, dd, passes, (*passes as f64 / 6.0 * 100.0), eq);
    }

    // Also export equity curves for top 3 HM candidates
    eprintln!("\nExporting equity curves...");
    for (hm, _, _, _, _, _, _) in results_sorted.iter().take(3) {
        eprintln!("  HM={}", hm);
        // Just print per-window equity for the first 3 candidates
        for (h, u, w, r) in all_results.iter().filter(|(h, u, _, _)| *h == *hm && *u == "Base5") {
            eprintln!("    {} W{}: ret={:.1}%, sh={:.3}, eq={:.4}", u, w, r.ret, r.sharpe, r.equity_final);
        }
    }

    eprintln!("\nRuntime: {:.1}s", t0.elapsed().as_secs_f64());
    eprintln!("CSV: {}", CSV_OUT);

    Ok(())
}

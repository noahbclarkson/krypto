//! ATR EMA Smoothing Period Hyperopt
//!
//! Tests EMA smoothing of Chandelier ATR values. ATR naturally fluctuates daily;
//! smoothing it with EMA before the Chandelier stop reduces stop-hopping noise.
//!
//! ATR_EMA_PERIOD=1 means raw ATR (baseline).
//! ATR_EMA_PERIOD ∈ {1..30} tested across 9 universes × ~6 windows.
//!
//! cargo run --example turtle_atr_ema_sweep --profile sweep

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
const CHAND_PERIOD: usize = 20;
const CHAND_MULT: f64 = 2.15;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 55;

const ATR_EMA_MIN: usize = 1;
const ATR_EMA_MAX: usize = 30;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"]),
    ("NoDOGE", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("Legacy4", &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    ("Legacy5BNB", &["ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"]),
    ("OldGuardNoBNB", &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
    ("LowVolume5", &["BTCUSDT", "ETHUSDT", "ADAUSDT", "LTCUSDT", "EOSUSDT"]),
    ("LargeCaps5", &["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT"]),
    ("DOGE5", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"]),
    ("FTXSurvivors", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
];

const CSV_OUT: &str = "snapshots/turtle_atr_ema_sweep.csv";
const EQUITY_CSV: &str = "snapshots/turtle_atr_ema_equity.csv";

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

fn ema(vals: &[f64], period: usize) -> Vec<f64> {
    if period == 0 || vals.is_empty() { return vals.to_vec(); }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut out = vec![vals[0]];
    for i in 1..vals.len() {
        out.push(alpha * vals[i] + (1.0 - alpha) * out[i - 1]);
    }
    out
}

fn turtle_signal(close: &[f64], _high: &[f64], entry_period: usize, idx: usize) -> bool {
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
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_ema_period: usize,
) -> WfResult {
    let mut atr_series: HashMap<String, Vec<f64>> = HashMap::new();
    for sym in symbols {
        if let Some(sd) = sym_data.get(sym) {
            let n = sd.close.len();
            let mut raw_atr = vec![0.0; n];
            for i in 0..n {
                raw_atr[i] = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, i);
            }
            let smoothed = ema(&raw_atr, atr_ema_period);
            atr_series.insert(sym.clone(), smoothed);
        }
    }

    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
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
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        let atr_s = atr_series.get(sym).map(|v| v.as_slice()).unwrap_or(&[]);

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = *atr_s.get(b).unwrap_or(&0.0);
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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("==== Turtle ATR EMA Smoothing Hyperopt ====");
    println!("Sweep: ATR_EMA {} to {}", ATR_EMA_MIN, ATR_EMA_MAX);

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
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
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
    println!("Loaded {} symbols, {} bars", sym_data_map.len(), n);

    #[derive(Default)]
    struct Agg {
        sum_ret: f64,
        sum_sharpe: f64,
        pass: usize,
        total: usize,
        worst_dd: f64,
        trades: usize,
    }

    let mut agg_by_param: HashMap<usize, Agg> = HashMap::new();
    for ema_val in ATR_EMA_MIN..=ATR_EMA_MAX {
        agg_by_param.insert(ema_val, Agg::default());
    }

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { continue; }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            for atr_ema in ATR_EMA_MIN..=ATR_EMA_MAX {
                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, atr_ema);
                let a = agg_by_param.get_mut(&atr_ema).unwrap();
                a.sum_ret += r.ret;
                a.sum_sharpe += r.sharpe;
                if r.pass { a.pass += 1; }
                a.total += 1;
                a.worst_dd = a.worst_dd.max(r.max_dd);
                a.trades += r.trades;
            }
        }
        println!("Completed universe: {}", label);
    }

    // Compute averages and find winner
    let mut results: Vec<(usize, f64, f64, usize, usize, f64, usize)> = Vec::new();
    for (&ema, agg) in &agg_by_param {
        let agg_total = agg.total;
        let avg_ret = agg.sum_ret / agg_total.max(1) as f64;
        let avg_sh = agg.sum_sharpe / agg_total.max(1) as f64;
        results.push((ema, avg_ret, avg_sh, agg.pass, agg_total, agg.worst_dd, agg.trades));
    }
    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    let winner = results[0].0;
    let baseline = ATR_EMA_MIN;

    println!("\n==== RESULTS (sorted by Sharpe) ====");
    for &(ema, ret, sh, p, tot, dd, tr) in &results {
        let pp = p as f64 / tot.max(1) as f64 * 100.0;
        let marker = if ema == winner { " *WINNER*" } else if ema == baseline { " *BASE*" } else { "" };
        println!("EMA={} Ret={} Sharpe={} Pass={}/{}({}%) DD={} Trades={}{}",
            ema, fmt_float(ret), fmt_float(sh), p, tot, fmt_float(pp), fmt_float(dd), tr, marker);
    }

    // Write CSV
    let mut f = File::create(CSV_OUT)?;
    writeln!(f, "atr_ema,avg_ret_pct,avg_sharpe,pass_count,total_count,pass_pct,worst_dd_pct,total_trades")?;
    for &(ema, ret, sh, p, tot, dd, tr) in &results {
        let pp = p as f64 / tot.max(1) as f64 * 100.0;
        writeln!(f, "{},{},{},{},{},{},{},{}", ema, ret, sh, p, tot, pp, dd, tr)?;
    }
    drop(f);
    println!("\nCSV: {}", CSV_OUT);

    // Equity curves for chart: BTCUSDT Base5 W00
    println!("\nGenerating equity curves for chart...");
    let btc_syms = vec![String::from("BTCUSDT")];

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let train_end = TRAIN_BARS;
    let test_start = train_end;
    let test_end = (test_start + TEST_BARS).min(n);

    let chart_params: Vec<usize> = vec![baseline, winner,
        results[results.len()/3].0, results[results.len()*2/3].0];

    let mut eq_lines = vec!["bar,param,equity".to_string()];
    for &atr_ema in &chart_params {
        let r = run_sim(&sym_data_map, &btc_syms, test_start, test_end, atr_ema);
        let step = (r.equity_curve.len() / 400).max(1);
        for (i, &eq) in r.equity_curve.iter().enumerate() {
            if i % step == 0 || i == r.equity_curve.len() - 1 {
                eq_lines.push(format!("{},{},{}", i, atr_ema, eq));
            }
        }
        println!("  ATR_EMA={} final_equity={}", atr_ema, r.equity_curve.last().copied().unwrap_or(1.0));
    }

    let mut eqf = File::create(EQUITY_CSV)?;
    for line in &eq_lines { writeln!(eqf, "{}", line)?; }
    drop(eqf);
    println!("Equity CSV: {}", EQUITY_CSV);

    // Full walk-forward comparison
    println!("\nWalk-forward comparison (winner vs baseline):");
    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { continue; }
        let tw = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if tw == 0 { continue; }

        let mut w_pass = 0;
        let mut b_pass = 0;
        let mut w_sh_sum = 0.0_f64;
        let mut b_sh_sum = 0.0_f64;
        let mut w_tr = 0usize;
        let mut b_tr = 0usize;

        for wi in 0..tw {
            let te = TRAIN_BARS + wi * TEST_BARS;
            let ts = te;
            let tend = (ts + TEST_BARS).min(n);
            if tend.saturating_sub(ts) < 5 { continue; }

            let wr = run_sim(&sym_data_map, &symbols, ts, tend, winner);
            let br = run_sim(&sym_data_map, &symbols, ts, tend, baseline);
            if wr.pass { w_pass += 1; }
            if br.pass { b_pass += 1; }
            w_sh_sum += wr.sharpe;
            b_sh_sum += br.sharpe;
            w_tr += wr.trades;
            b_tr += br.trades;
        }

        let w_sh = w_sh_sum / tw as f64;
        let b_sh = b_sh_sum / tw as f64;
        let winner_mark = if w_sh > b_sh { ">" } else { "=" };
        println!("  {} W({})({})  B({})({})  {} win  trades {}/{}", label, winner, fmt_float(w_sh), baseline, fmt_float(b_sh), winner_mark, w_tr, b_tr);
    }

    println!("\n=== WINNER ===");
    if let Some(&(ema, ret, sh, p, tot, dd, tr)) = results.first() {
        let pp = p as f64 / tot.max(1) as f64 * 100.0;
        let baseline_sh = results.iter().find(|x| x.0 == baseline).map(|x| x.2).unwrap_or(0.0);
        println!("ATR_EMA_PERIOD = {} (baseline = {} raw ATR)", ema, baseline);
        println!("Avg Sharpe: {} (baseline={}, delta={})", fmt_float(sh), fmt_float(baseline_sh), fmt_float(sh - baseline_sh));
        println!("Pass rate: {}/{} ({})", p, tot, fmt_float(pp));
        println!("Worst DD: {}", fmt_float(dd));
    }
    println!("Runtime: {:?}", t0.elapsed());

    Ok(())
}

fn fmt_float(v: f64) -> String {
    format!("{:.4}", v)
}

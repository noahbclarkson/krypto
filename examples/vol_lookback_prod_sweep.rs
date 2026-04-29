//! VOL_LOOKBACK production-parameter sweep (1..=100 step 1)
//!
//! Hyperopt target: dollar-volume smoothing lookback used by the walk-forward
//! validation harness for top-N symbol ranking. This is harness-only (not live bot
//! execution), but it materially affects validated portfolio composition.
//!
//! Important: earlier dense sweep used stale CHAND(28,2.00)/HM=45. This file
//! uses current production validation params: EP=21, CHAND(7,2.30),
//! TurtleATR(24,2.00), HM=12, CAP=3.
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

// Current validated production / validation params.
const TURTLE_ENTRY: usize = 21;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const BASELINE_VOL_LOOKBACK: usize = 8;

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

#[derive(Clone)]
struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64> }

#[derive(Clone)]
struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

#[derive(Clone)]
struct AggStats {
    vol_lookback: usize,
    avg_sharpe: f64,
    avg_ret: f64,
    worst_dd: f64,
    pass_count: usize,
    total_count: usize,
    total_trades: usize,
    avg_win_rate: f64,
    positive_universes: usize,
}

fn vol_lookbacks() -> Vec<usize> { (1..=100).collect() }

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if vals.is_empty() { return 0.0; }
    if idx + 1 < window { return vals[idx.min(vals.len() - 1)]; }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0;
    let mut count = 0usize;
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        sum += (h - l).max((h - c0).abs()).max((l - c0).abs());
        count += 1;
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
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
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { 0.0 } else { mean * 365.0_f64.sqrt() / sd }
}

fn max_drawdown_pct(equity_curve: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    for &e in equity_curve {
        if e.is_finite() && e > peak { peak = e; }
        if peak > 0.0 && e.is_finite() {
            let dd = (peak - e) / peak;
            if dd > max_dd { max_dd = dd; }
        }
    }
    max_dd * 100.0
}

fn trade_exit(sd: &SymData, entry_bar_next: usize) -> usize {
    let n = sd.close.len();
    let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
    let mut exit_bar = max_bar;
    let mut highest_high_chand = sd.high.get(entry_bar_next).copied().unwrap_or(0.0);
    let mut lowest_low_turtle = sd.low.get(entry_bar_next).copied().unwrap_or(0.0);
    for b in entry_bar_next..=max_bar {
        highest_high_chand = highest_high_chand.max(sd.high.get(b).copied().unwrap_or(0.0));
        let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
        let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

        lowest_low_turtle = lowest_low_turtle.min(sd.low.get(b).copied().unwrap_or(0.0));
        let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
        let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;

        let close = sd.close.get(b).copied().unwrap_or(0.0);
        if close < trail_chand || close < trail_turtle { exit_bar = b; break; }
    }
    exit_bar
}

fn ranked_symbols<'a>(sym_data: &'a HashMap<String, SymData>, symbols: &'a [String], bar: usize, vol_lookback: usize) -> Vec<&'a str> {
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
    scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect()
}

fn run_sim(sym_data: &HashMap<String, SymData>, symbols: &[String], test_start: usize, test_end: usize, vol_lookback: usize) -> WfResult {
    let mut equity = 1.0_f64;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut eq_marks = vec![1.0_f64];
    let mut bar = test_start;

    while bar + 2 < test_end {
        let top_syms = ranked_symbols(sym_data, symbols, bar, vol_lookback);
        let mut entered = false;
        for sym in top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() && turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                    let entry_px = sd.close[bar];
                    if entry_px <= 0.0 { continue; }
                    let entry = entry_px * (1.0 - TAKER_FEE);
                    let entry_bar_next = bar + 1;
                    if entry_bar_next >= sd.close.len() { continue; }
                    let exit_bar = trade_exit(sd, entry_bar_next);
                    if let Some(&exit_px) = sd.close.get(exit_bar) {
                        let exit = exit_px * (1.0 - TAKER_FEE);
                        let gross_ret = exit / entry - 1.0;
                        let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                        wins += usize::from(gross_ret > 0.0);
                        total_trades += 1;
                        equity *= 1.0 + gross_ret;
                        let avg_daily = gross_ret / bars_held as f64;
                        for _ in 0..bars_held { daily_rets.push(avg_daily); }
                        eq_marks.push(equity);
                        bar = exit_bar + 1;
                        entered = true;
                        break;
                    }
                }
            }
        }
        if !entered { bar += 1; }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_drawdown_pct(&eq_marks);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

fn run_full_equity_curve(sym_data: &HashMap<String, SymData>, symbols: &[String], n: usize, vol_lookback: usize) -> Vec<f64> {
    let warmup = TURTLE_ENTRY.max(CHAND_PERIOD).max(TURTLE_ATR_PERIOD) + 1;
    let mut equity = 1.0_f64;
    let mut curve = vec![1.0_f64; n];
    let mut bar = warmup;

    while bar + 2 < n {
        curve[bar] = equity;
        let top_syms = ranked_symbols(sym_data, symbols, bar, vol_lookback);
        let mut entered = false;
        for sym in top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() && turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                    let entry_px = sd.close[bar];
                    if entry_px <= 0.0 { continue; }
                    let entry = entry_px * (1.0 - TAKER_FEE);
                    let entry_bar_next = bar + 1;
                    if entry_bar_next >= sd.close.len() { continue; }
                    let exit_bar = trade_exit(sd, entry_bar_next);
                    for b in entry_bar_next..exit_bar.min(n) { curve[b] = equity; }
                    if let Some(&exit_px) = sd.close.get(exit_bar) {
                        let exit = exit_px * (1.0 - TAKER_FEE);
                        let gross_ret = exit / entry - 1.0;
                        equity *= 1.0 + gross_ret;
                        if exit_bar < n { curve[exit_bar] = equity; }
                        bar = exit_bar + 1;
                        entered = true;
                        break;
                    }
                }
            }
        }
        if !entered { bar += 1; }
    }
    for b in bar.min(n)..n { curve[b] = equity; }
    curve
}

fn aggregate_stats(vb: usize, rows: &[(String, WfResult)]) -> AggStats {
    let total_count = rows.len();
    let pass_count = rows.iter().filter(|(_, r)| r.pass).count();
    let avg_sharpe = if total_count == 0 { 0.0 } else { rows.iter().map(|(_, r)| r.sharpe).sum::<f64>() / total_count as f64 };
    let avg_ret = if total_count == 0 { 0.0 } else { rows.iter().map(|(_, r)| r.ret).sum::<f64>() / total_count as f64 };
    let worst_dd = rows.iter().map(|(_, r)| r.max_dd).fold(0.0_f64, f64::max);
    let total_trades = rows.iter().map(|(_, r)| r.trades).sum();
    let avg_win_rate = if total_count == 0 { 0.0 } else { rows.iter().map(|(_, r)| r.win_rate).sum::<f64>() / total_count as f64 };

    let mut uni: HashMap<&str, (f64, usize)> = HashMap::new();
    for (label, r) in rows {
        let ent = uni.entry(label.as_str()).or_insert((0.0, 0));
        ent.0 += r.ret;
        ent.1 += 1;
    }
    let positive_universes = uni.values().filter(|(sum, n)| *n > 0 && *sum / *n as f64 > 0.0).count();

    AggStats { vol_lookback: vb, avg_sharpe, avg_ret, worst_dd, pass_count, total_count, total_trades, avg_win_rate, positive_universes }
}

fn rank_key(s: &AggStats) -> (usize, usize, i64, i64) {
    (
        s.pass_count,
        s.positive_universes,
        (s.avg_sharpe * 1_000_000.0).round() as i64,
        (s.avg_ret * 1_000.0).round() as i64,
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let vals = vol_lookbacks();
    eprintln!("VOL_LOOKBACK prod sweep: {} values (1..=100 step 1)", vals.len());
    eprintln!("Params: EP={} CHAND({},{}) TurtleATR({},{}) HM={} CAP={} baseline VL={}",
        TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP, BASELINE_VOL_LOOKBACK);
    eprintln!("9 universes × walk-forward 252/252 windows × 100 values\n");

    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    let loader = DataLoader::new(None, None);
    let mut all_syms = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES { for &s in *syms { all_syms.insert(s.to_string()); } }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
        }
    }
    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = n.min(df.height());
            let close_col = df.column("close")?.f64()?;
            let high_col = df.column("high")?.f64()?;
            let low_col = df.column("low")?.f64()?;
            let vol_col = df.column("volume")?.f64()?;
            sym_data_map.insert(sym.clone(), SymData {
                close: close_col.into_iter().filter_map(|x| x).take(n_min).collect(),
                high: high_col.into_iter().filter_map(|x| x).take(n_min).collect(),
                low: low_col.into_iter().filter_map(|x| x).take(n_min).collect(),
                vol: vol_col.into_iter().filter_map(|x| x).take(n_min).collect(),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} aligned bars\n", sym_data_map.len(), n);

    let metrics_path = "snapshots/vol_lookback_prod_sweep.csv";
    let mut metrics = vec!["vol_lookback,universe,window,ret_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    let mut per_vb: HashMap<usize, Vec<(String, WfResult)>> = HashMap::new();
    for &vb in &vals { per_vb.insert(vb, Vec::new()); }

    for &(label, symbols_raw) in UNIVERSES {
        let symbols: Vec<String> = symbols_raw.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { eprintln!("SKIP {} missing symbols", label); continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        for wi in 0..total_windows {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }
            for &vb in &vals {
                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, vb);
                metrics.push(format!("{},{},{},{:.4},{:.6},{:.4},{},{:.4},{}",
                    vb, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass));
                per_vb.get_mut(&vb).unwrap().push((label.to_string(), r));
            }
        }
    }
    let mut f = File::create(metrics_path)?;
    for line in &metrics { writeln!(f, "{}", line)?; }

    let mut aggs: Vec<AggStats> = vals.iter().map(|&vb| aggregate_stats(vb, per_vb.get(&vb).unwrap())).collect();
    aggs.sort_by_key(rank_key);
    aggs.reverse();
    let winner = aggs[0].vol_lookback;
    let mut selected = vec![BASELINE_VOL_LOOKBACK];
    if !selected.contains(&winner) { selected.push(winner); }
    for s in aggs.iter().take(6) {
        if !selected.contains(&s.vol_lookback) { selected.push(s.vol_lookback); }
        if selected.len() >= 5 { break; }
    }

    let summary_path = "snapshots/vol_lookback_prod_summary.csv";
    let mut sf = File::create(summary_path)?;
    writeln!(sf, "vol_lookback,pass_count,total_count,pass_rate_pct,positive_universes,avg_sharpe,avg_ret_pct,worst_dd_pct,total_trades,avg_win_rate_pct")?;
    let mut sorted_by_vb = aggs.clone();
    sorted_by_vb.sort_by_key(|s| s.vol_lookback);
    for s in sorted_by_vb {
        let pr = if s.total_count == 0 { 0.0 } else { s.pass_count as f64 / s.total_count as f64 * 100.0 };
        writeln!(sf, "{},{},{},{:.4},{},{:.6},{:.4},{:.4},{},{:.4}",
            s.vol_lookback, s.pass_count, s.total_count, pr, s.positive_universes, s.avg_sharpe, s.avg_ret, s.worst_dd, s.total_trades, s.avg_win_rate)?;
    }

    let base5_symbols: Vec<String> = UNIVERSES[0].1.iter().map(|s| s.to_string()).collect();
    let equity_path = "snapshots/vol_lookback_prod_equity.csv";
    let mut ef = File::create(equity_path)?;
    write!(ef, "bar")?;
    for vb in &selected { write!(ef, ",vl_{}", vb)?; }
    writeln!(ef)?;
    let curves: Vec<(usize, Vec<f64>)> = selected.iter().map(|&vb| (vb, run_full_equity_curve(&sym_data_map, &base5_symbols, n, vb))).collect();
    for i in 0..n {
        write!(ef, "{}", i)?;
        for (_, curve) in &curves { write!(ef, ",{:.10}", curve[i])?; }
        writeln!(ef)?;
    }

    eprintln!("Metrics: {}", metrics_path);
    eprintln!("Summary: {}", summary_path);
    eprintln!("Equity curves: {}", equity_path);
    eprintln!("\nTop 10 by robustness (pass rate > universes > Sharpe > return):");
    eprintln!("{:>4} | {:>7} | {:>4} | {:>9} | {:>9} | {:>9} | {:>7}", "VL", "Pass", "+Uni", "AvgSharpe", "AvgRet%", "WorstDD%", "Trades");
    for s in aggs.iter().take(10) {
        let pr = if s.total_count == 0 { 0.0 } else { s.pass_count as f64 / s.total_count as f64 * 100.0 };
        eprintln!("{:>4} | {:>6.1}% | {:>4} | {:>9.4} | {:>+8.1}% | {:>8.1}% | {:>7}",
            s.vol_lookback, pr, s.positive_universes, s.avg_sharpe, s.avg_ret, s.worst_dd, s.total_trades);
    }
    let baseline = aggs.iter().find(|s| s.vol_lookback == BASELINE_VOL_LOOKBACK).unwrap();
    eprintln!("\nWINNER: VL={} | BASELINE VL={} rank={} | selected curves {:?}",
        winner,
        BASELINE_VOL_LOOKBACK,
        aggs.iter().position(|s| s.vol_lookback == BASELINE_VOL_LOOKBACK).unwrap() + 1,
        selected);
    eprintln!("Baseline avg Sharpe {:.4}, avg ret {:.1}%, pass {}/{}; Winner avg Sharpe {:.4}, avg ret {:.1}%, pass {}/{}",
        baseline.avg_sharpe, baseline.avg_ret, baseline.pass_count, baseline.total_count,
        aggs[0].avg_sharpe, aggs[0].avg_ret, aggs[0].pass_count, aggs[0].total_count);
    eprintln!("Runtime: {:?}", t0.elapsed());
    Ok(())
}

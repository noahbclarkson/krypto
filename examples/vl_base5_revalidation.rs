//! T33: VOL_LOOKBACK 90 vs 8 — Base5 Production Re-Validation
//!
//! Compare VL=90 vs VL=8 on Base5 × 6 windows.
//! Decision: If VL=90 wins Base5, update default. If VL=90 loses, revert to VL=8.
//! Production params: EP=21, CHAND_P=7, CHAND_M=2.30, ATR_P=24, ATR_M=2.0, HM=12, CAP=3, ATR_ENTRY_MULT=0.00

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

const EP: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const ATR_ENTRY_MULT: f64 = 0.00;
const MIN_TRADES: usize = 3;

const BASE5: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];
const VL_VALUES: [usize; 2] = [8, 90];
const CSV_OUT: &str = "snapshots/vl_base5_revalidation.csv";

struct SymData {
    close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64>,
}

fn tr(high: f64, low: f64, prev_close: f64) -> f64 {
    (high - low).max((high - prev_close).abs()).max((low - prev_close).abs())
}

fn compute_atr(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0_f64;
    for i in (idx + 1 - period)..=idx {
        let h = *high.get(i).unwrap_or(&0.0);
        let l = *low.get(i).unwrap_or(&0.0);
        let pc = *close.get(i.saturating_sub(1)).unwrap_or(&h);
        sum += tr(h, l, pc);
    }
    sum / period as f64
}

fn vol_rank(sd: &SymData, bar: usize, lookback: usize) -> f64 {
    if bar < lookback { return 1.0; }
    let cur = *sd.vol.get(bar.min(sd.vol.len() - 1)).unwrap_or(&0.0);
    let start = bar.saturating_sub(lookback);
    let len = bar - start;
    if len == 0 { return 1.0; }
    let sum: f64 = sd.vol[start..bar].iter().sum();
    let avg = sum / len as f64;
    if avg <= 0.0 { return 1.0; }
    (cur / avg).min(3.0)
}

struct WfResult {
    return_pct: f64, sharpe: f64, max_dd_pct: f64,
    trades: usize, win_rate_pct: f64, passed: bool,
}

#[derive(Clone)]
struct Pos { sym: String, entry: f64, hi: f64, bar: usize }

fn run_sim(sym_data: &HashMap<String, SymData>, symbols: &[String],
           test_start: usize, test_end: usize, vol_lookback: usize) -> WfResult {
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut total_trades = 0usize;
    let mut wins = 0usize;
    let mut daily_rets = Vec::with_capacity(test_end - test_start);
    let mut positions: Vec<Pos> = Vec::new();

    for bar in test_start..test_end {
        let prev_equity = equity;

        // Entry
        for sym in symbols {
            if positions.iter().filter(|p| &p.sym == sym).count() >= 1 { continue; }
            if positions.len() >= POSITION_CAP { continue; }
            let sd = sym_data.get(sym).unwrap();
            if bar < EP { continue; }
            let close = *sd.close.get(bar.min(sd.close.len() - 1)).unwrap_or(&0.0);
            if close <= 0.0 { continue; }

            let max_close = sd.close[(bar.saturating_sub(EP))..bar].iter().fold(0.0_f64, |m, &v| m.max(v));
            let vr = vol_rank(sd, bar, vol_lookback);

            if close > max_close && vr > 0.5 {
                if ATR_ENTRY_MULT > 0.0 {
                    let a = compute_atr(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar.saturating_sub(1));
                    if a <= 0.0 || close < max_close + a * ATR_ENTRY_MULT { continue; }
                }
                positions.push(Pos { sym: sym.clone(), entry: close, hi: close, bar });
                total_trades += 1;
            }
        }

        // Exit — Turtle ATR trailing stop
        let mut to_remove = Vec::new();
        for (i, pos) in positions.iter_mut().enumerate() {
            let sd = sym_data.get(&pos.sym).unwrap();
            let cur_close = *sd.close.get(bar.min(sd.close.len() - 1)).unwrap_or(&pos.entry);
            let atr_val = compute_atr(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar.saturating_sub(1));
            let hi_end = sd.high[pos.bar..=bar.min(sd.high.len() - 1)].iter().fold(pos.entry, |m, &v| m.max(v));
            pos.hi = hi_end;
            let stop = if atr_val > 0.0 { pos.hi - TURTLE_ATR_MULT * atr_val } else { 0.0 };
            let exceeded = bar - pos.bar >= HOLD_MAX;
            let stopped = atr_val > 0.0 && *sd.low.get(bar.min(sd.low.len() - 1)).unwrap_or(&0.0) <= stop;
            if exceeded || stopped {
                let exit_price = if stopped {
                    (pos.hi - TURTLE_ATR_MULT * atr_val).max(cur_close * 0.99)
                } else { cur_close };
                let pnl = (exit_price / pos.entry - 1.0) - TAKER_FEE * 2.0;
                let pos_size = 1.0 / POSITION_CAP as f64;
                equity *= 1.0 + pnl * pos_size;
                if pnl > 0.0 { wins += 1; }
                to_remove.push(i);
            }
        }
        for i in to_remove.into_iter().rev() { positions.remove(i); }

        // Record daily return
        let ret = (equity - prev_equity) / prev_equity;
        daily_rets.push(ret);

        let dd = (equity - peak) / peak;
        if dd < max_dd { max_dd = dd; }
        peak = equity.max(peak);
    }

    let ret_pct = (equity - 1.0) * 100.0;
    let sharpe = if daily_rets.len() < 10 { 0.0 } else {
        let m = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
        let s = (daily_rets.iter().map(|&r| (r - m).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
        if s == 0.0 { 0.0 } else { m / s * (252.0_f64.sqrt()) }
    };
    let wr = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    WfResult { return_pct: ret_pct, sharpe, max_dd_pct: max_dd * 100.0, trades: total_trades, win_rate_pct: wr, passed: total_trades >= MIN_TRADES && sharpe > 0.0 }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let mut csv = vec!["vl,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    let mut results: HashMap<usize, Vec<WfResult>> = HashMap::new();
    for &v in &VL_VALUES { results.insert(v, Vec::new()); }

    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;

    for &sym in BASE5 {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.to_string(), df);
            }
            Err(e) => eprintln!("Failed {}: {}", sym, e),
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();

    for &sym in BASE5 {
        if let Some(df) = raw_cache.get(sym) {
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

    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);
    let symbols: Vec<String> = BASE5.iter().map(|s| s.to_string()).collect();
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

    for &vl in &VL_VALUES {
        eprintln!("\n==== VL={} ====", vl);
        let mut agg = 0.0_f64; let mut passed = 0usize;
        for wi in 0..total_windows {
            let ts = TRAIN_BARS + wi * TEST_BARS;
            let te = (ts + TEST_BARS).min(n);
            if te - ts < 50 { continue; }
            let r = run_sim(&sym_data_map, &symbols, ts, te, vl);
            let ps = if r.passed { "PASS" } else { "FAIL" };
            eprintln!("  W{:02}: {:+8.1}%  sh={:6.3}  dd={:6.1}%  tr={:3}  {}",
                wi, r.return_pct, r.sharpe, r.max_dd_pct.abs(), r.trades, ps);
            csv.push(format!("{},Base5,{},{:.2},{:.4},{:.2},{},{:.1},{}",
                vl, wi, r.return_pct, r.sharpe, r.max_dd_pct.abs(), r.trades, r.win_rate_pct, if r.passed { 1 } else { 0 }));
            agg += r.return_pct; if r.passed { passed += 1; }
            results.get_mut(&vl).unwrap().push(r);
        }
        let avg_ret = agg / total_windows as f64;
        let pass_pct = passed as f64 / total_windows as f64 * 100.0;
        eprintln!("  AGG: avg {:+8.1}%  {}/{} pass ({:.0}%)\n", avg_ret, passed, total_windows, pass_pct);
    }

    eprintln!("\n==== SUMMARY: VL=90 vs VL=8 on Base5 ====\n");
    for &vl in &VL_VALUES {
        let rs = results.get(&vl).unwrap();
        let pass = rs.iter().filter(|r| r.passed).count(); let tot = rs.len();
        let sh = rs.iter().map(|r| r.sharpe).sum::<f64>() / tot as f64;
        let rt = rs.iter().map(|r| r.return_pct).sum::<f64>() / tot as f64;
        let dd = rs.iter().map(|r| r.max_dd_pct).sum::<f64>() / tot as f64;
        let wr = rs.iter().map(|r| r.win_rate_pct).sum::<f64>() / tot as f64;
        let tr: usize = rs.iter().map(|r| r.trades).sum();
        eprintln!("  VL={}: {}/{} ({:.0}%)  sh={:.3}  ret={:+.1}%  dd={:.1}%  wr={:.1}%  tr={}", vl, pass, tot, pass as f64/tot as f64*100.0, sh, rt, dd.abs(), wr, tr);
    }

    let mut f = File::create(CSV_OUT)?;
    for l in &csv { writeln!(f, "{}", l)?; }
    eprintln!("\nSaved: {}", CSV_OUT);
    eprintln!("Time: {:.1}s\n", t0.elapsed().as_secs_f64());
    Ok(())
}
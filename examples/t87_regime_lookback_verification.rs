//! T87: REGIME_LOOKBACK Head-to-Head Exact-Live Verification
//!
//! Purpose: directly compare LB=8 (T86 winner) vs LB=41 (production default)
//! using IDENTICAL exact-live event-driven logic. This is the definitive test
//! before any production parameter update.
//!
//! T86 sweep found:
//!   LB=8  → equity 3.36x (winner)
//!   LB=40 → equity 2.93x (nearest to production LB=41)
//!   LB=41 → NOT directly tested in sweep
//!
//! live_bot_exact_equity.rs (LB=41, production) → equity 2.76x
//!
//! This harness runs LB=8 and LB=41 in IDENTICAL path and compares.

use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;
const BASE_SYMBOLS: [&str; 6] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

use krypto::live::config::{
    ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_ATR_PERIOD, HEDGE_LOOKBACK,
    HEDGE_SIZE_MULT, HOLD_MAX, POSITION_CAP, REGIME_ATR_PERIOD, TURTLE_ATR_MULT,
    TURTLE_ATR_PERIOD, TURTLE_EP,
};

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    dates: Vec<String>,
}

#[derive(Clone)]
struct PositionState {
    entry_bar: usize,
    entry_price: f64,
    entry_exec: f64,
    size: f64,
    highest_high: f64,
    lowest_low: f64,
    bars_held: usize,
    atr_buf: VecDeque<f64>,
}

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 { sd.close[idx] } else { sd.close[idx - 1] };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(sd: &SymData, period: usize, idx: usize) -> f64 {
    if period == 0 || idx < period { return 0.0; }
    let start = idx + 1 - period;
    let mut sum = 0.0;
    for i in start..=idx { sum += tr_at(sd, i); }
    sum / period as f64
}

fn btc_atr_percentile(btc: &SymData, idx: usize, lb: usize) -> f64 {
    let len = idx + 1;
    if len <= REGIME_ATR_PERIOD.max(lb) + 1 { return 50.0; }
    let curr_atr = atr_at(btc, REGIME_ATR_PERIOD, idx);
    let curr_close = btc.close[idx];
    if curr_atr <= 0.0 || curr_close <= 0.0 { return 50.0; }
    let curr_pct = curr_atr / curr_close;
    let start = idx.saturating_sub(lb);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let close = btc.close[i];
        if close <= 0.0 { continue; }
        let hist_atr = atr_at(btc, REGIME_ATR_PERIOD, i);
        if hist_atr <= 0.0 { continue; }
        if hist_atr / close < curr_pct { below += 1; }
        total += 1;
    }
    if total == 0 { 50.0 } else { (below as f64 / total as f64) * 100.0 }
}

fn hedge_active(btc: &SymData, idx: usize) -> bool {
    let n = idx + 1;
    if n < HEDGE_LOOKBACK + HEDGE_ATR_PERIOD { return false; }
    let mut trs = Vec::with_capacity(HEDGE_ATR_PERIOD);
    for i in (n - HEDGE_ATR_PERIOD)..n { trs.push(tr_at(btc, i)); }
    let hedge_atr = trs.iter().sum::<f64>() / HEDGE_ATR_PERIOD as f64;
    let mut hist = Vec::with_capacity(HEDGE_LOOKBACK);
    for j in 1..=HEDGE_LOOKBACK {
        let hist_idx = n.saturating_sub(j);
        if hist_idx == 0 { break; }
        hist.push(tr_at(btc, hist_idx));
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    hist.get(pct_idx).is_some_and(|&threshold| hedge_atr > threshold)
}

fn live_bot_entry_signal(sd: &SymData, idx: usize) -> bool {
    let len = idx + 1;
    if len < TURTLE_EP + 1 { return false; }
    let ws = len - TURTLE_EP;
    let max_close = sd.close[ws..=idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if sd.close[idx] < max_close { return false; }
    if ATR_ENTRY_MULT > 0.0 && len >= TURTLE_ATR_PERIOD + 1 {
        let atr = atr_at(sd, TURTLE_ATR_PERIOD, idx);
        if sd.close[idx] < max_close + atr * ATR_ENTRY_MULT { return false; }
    }
    true
}

fn seed_atr_buf(sd: &SymData, idx: usize) -> VecDeque<f64> {
    let len = idx + 1;
    let avail = len.min(TURTLE_ATR_PERIOD);
    let start = len.saturating_sub(avail);
    let mut atr_buf = VecDeque::with_capacity(TURTLE_ATR_PERIOD);
    for offset in 0..avail {
        let b_idx = start + offset;
        if b_idx >= len { break; }
        let pc = if offset == 0 { sd.close[b_idx] } else { sd.close[start + offset - 1] };
        let tr = (sd.high[b_idx] - sd.low[b_idx])
            .max((sd.high[b_idx] - pc).abs())
            .max((sd.low[b_idx] - pc).abs());
        atr_buf.push_back(tr);
    }
    atr_buf
}

fn mtm_equity(realized: f64, positions: &HashMap<String, PositionState>, data: &HashMap<String, SymData>, idx: usize, fee: f64) -> f64 {
    let mut open_ret = 0.0;
    for (sym, pos) in positions {
        if let Some(sd) = data.get(sym) {
            if idx < sd.close.len() {
                let liq_exec = sd.close[idx] * (1.0 - fee);
                open_ret += pos.size * (liq_exec / pos.entry_exec - 1.0);
            }
        }
    }
    realized * (1.0 + open_ret)
}

fn annualised_sharpe(equity: &[f64]) -> f64 {
    if equity.len() < 2 { return 0.0; }
    let mut rets = Vec::with_capacity(equity.len() - 1);
    for w in equity.windows(2) { if w[0] > 0.0 { rets.push(w[1] / w[0] - 1.0); } }
    if rets.is_empty() { return 0.0; }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    let sd = var.sqrt();
    if sd == 0.0 { return 0.0; }
    mean * 365.0_f64.sqrt() / sd
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

struct SimResult {
    equity: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    curve: Vec<f64>,
}

fn run_exact_live(
    data: &HashMap<String, SymData>,
    btc: &SymData,
    symbols: &[String],
    lb: usize,
    test_start: usize,
    test_end: usize,
    fee: f64,
) -> SimResult {
    let mut realized_equity = 1.0_f64;
    let mut equity_curve = Vec::with_capacity(test_end - test_start);
    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut trades_count = 0usize;

    for idx in test_start..test_end {
        equity_curve.push(mtm_equity(realized_equity, &positions, data, idx, fee));

        let to_close: Vec<String> = positions.keys().cloned().collect();
        for sym in to_close {
            if let Some(sd) = data.get(&sym) {
                let pos = positions.get_mut(&sym).unwrap();
                let tr = tr_at(sd, idx);
                if sd.high[idx] > pos.highest_high { pos.highest_high = sd.high[idx]; }
                if sd.low[idx] < pos.lowest_low { pos.lowest_low = sd.low[idx]; }
                pos.bars_held += 1;
                pos.atr_buf.push_back(tr);
                if pos.atr_buf.len() > TURTLE_ATR_PERIOD { pos.atr_buf.pop_front(); }

                let mut exit_reason: Option<String> = None;
                if pos.bars_held >= HOLD_MAX {
                    exit_reason = Some("HOLD_MAX".to_string());
                } else if pos.atr_buf.len() >= TURTLE_ATR_PERIOD {
                    let atr = pos.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                    let turtle_stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                    if atr > 0.0 && sd.low[idx] <= turtle_stop {
                        exit_reason = Some("ATR".to_string());
                    }
                }

                if exit_reason.is_some() {
                    let exit_exec = sd.close[idx] * (1.0 - fee);
                    let pct_ret = exit_exec / pos.entry_exec - 1.0;
                    realized_equity *= 1.0 + pos.size * pct_ret;
                    trades_count += 1;
                    positions.remove(&sym);
                }
            }
        }

        if positions.len() >= POSITION_CAP { continue; }
        if !live_bot_entry_signal(&data.get("BTCUSDT").cloned().unwrap_or(SymData { close: vec![], high: vec![], low: vec![], dates: vec![] }), idx) { continue; }

        for sym in symbols {
            if positions.len() >= POSITION_CAP { break; }
            if positions.contains_key(sym) { continue; }
            if let Some(sd) = data.get(sym) {
                if !live_bot_entry_signal(sd, idx) { continue; }
                let btc_pct = btc_atr_percentile(btc, idx, lb);
                if btc_pct < ATR_RANK_THRESHOLD { continue; }

                let hedge = hedge_active(btc, idx);
                let mut size = 1.0 / POSITION_CAP as f64;
                if hedge { size *= HEDGE_SIZE_MULT; }

                positions.insert(sym.clone(), PositionState {
                    entry_bar: idx,
                    entry_price: sd.close[idx],
                    entry_exec: sd.close[idx] * (1.0 + fee),
                    size,
                    highest_high: sd.high[idx],
                    lowest_low: sd.low[idx],
                    bars_held: 0,
                    atr_buf: seed_atr_buf(sd, idx),
                });
            }
        }
    }

    let last_idx = test_end.saturating_sub(1);
    for (sym, pos) in positions.drain() {
        if let Some(sd) = data.get(&sym) {
            let exit_exec = sd.close[last_idx] * (1.0 - fee);
            let pct_ret = exit_exec / pos.entry_exec - 1.0;
            realized_equity *= 1.0 + pos.size * pct_ret;
            trades_count += 1;
        }
    }
    if let Some(last) = equity_curve.last_mut() { *last = realized_equity; }

    SimResult { equity: realized_equity, sharpe: annualised_sharpe(&equity_curve), max_dd: max_dd(&equity_curve), trades: trades_count, curve: equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T87: REGIME_LOOKBACK Head-to-Head Verification ===");
    println!("Comparing LB=8 (T86 winner) vs LB=41 (production default)");
    println!();

    let fee = 0.0004;
    let loader = krypto::data::loader::DataLoader::new(None, None);
    let mut raw_data: HashMap<String, SymData> = HashMap::new();

    for sym in BASE_SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let time_col = df.column("time").ok();
        let dates: Vec<String> = (0..close.len())
            .map(|i| time_col.and_then(|s| s.get(i).ok()).map(|v| v.to_string()).unwrap_or_default())
            .collect();
        raw_data.insert(sym.to_string(), SymData { close, high, low, dates });
    }

    let mut common_dates = raw_data.get("BTCUSDT").expect("BTCUSDT loaded").dates.clone();
    common_dates.retain(|d| raw_data.values().all(|sd| sd.dates.iter().any(|x| x == d)));
    common_dates.sort();
    common_dates.dedup();

    let mut data: HashMap<String, SymData> = HashMap::new();
    for (sym, sd) in &raw_data {
        let index_by_date: HashMap<String, usize> = sd.dates.iter().enumerate().map(|(i, d)| (d.clone(), i)).collect();
        let mut close = Vec::with_capacity(common_dates.len());
        let mut high = Vec::with_capacity(common_dates.len());
        let mut low = Vec::with_capacity(common_dates.len());
        let mut dates = Vec::with_capacity(common_dates.len());
        for d in &common_dates {
            if let Some(&i) = index_by_date.get(d) {
                close.push(sd.close[i]);
                high.push(sd.high[i]);
                low.push(sd.low[i]);
                dates.push(d.clone());
            }
        }
        data.insert(sym.clone(), SymData { close, high, low, dates });
    }

    let symbols: Vec<String> = BASE_SYMBOLS.iter().map(|s| s.to_string()).collect();
    let btc = data.get("BTCUSDT").expect("BTCUSDT loaded").clone();
    let test_start = WARMUP_BARS;
    let test_end = common_dates.len();

    let lb_values = [8, 41];
    let mut results: HashMap<usize, SimResult> = HashMap::new();

    for &lb in &lb_values {
        print!("Running LB={}...", lb);
        let r = run_exact_live(&data, &btc, &symbols, lb, test_start, test_end, fee);
        println!(" equity={:.4f}x sharpe={:.3f} dd={:.2f}% trades={}", r.equity, r.sharpe, r.max_dd, r.trades);
        results.insert(lb, r);
    }

    let r8 = results.get(&8).expect("LB=8 result");
    let r41 = results.get(&41).expect("LB=41 result");
    let delta_eq = (r8.equity - r41.equity) / r41.equity * 100.0;
    let delta_sh = r8.sharpe - r41.sharpe;

    println!();
    println!("=== T87 HEAD-TO-HEAD RESULTS ===");
    println!("{:<6} {:>10} {:>8} {:>8} {:>8}", "LB", "Equity", "Sharpe", "MaxDD%", "Trades");
    println!("{:<6} {:>10.4f} {:>8.4f} {:>8.2f} {:>8}", 8, r8.equity, r8.sharpe, r8.max_dd, r8.trades);
    println!("{:<6} {:>10.4f} {:>8.4f} {:>8.2f} {:>8}", 41, r41.equity, r41.sharpe, r41.max_dd, r41.trades);
    println!();
    println!("Delta: LB=8 vs LB=41:");
    println!("  Equity: {:.4f}x vs {:.4f}x ({:+.1f}%)", r8.equity, r41.equity, delta_eq);
    println!("  Sharpe: {:.4f} vs {:.4f} ({:+.4f})", r8.sharpe, r41.sharpe, delta_sh);
    println!("  MaxDD:  {:.2f}% vs {:.2f}% ({:+.2f}pp)", r8.max_dd, r41.max_dd, r8.max_dd - r41.max_dd);
    println!("  Trades: {} vs {}", r8.trades, r41.trades);

    // Export equity curves
    let max_len = r8.curve.len().max(r41.curve.len());
    let mut eq_csv_lines = vec!["bar,date,lb_8,lb_41".to_string()];
    for offset in 0..max_len {
        let idx = test_start + offset;
        let date = btc.dates.get(idx).cloned().unwrap_or_default();
        let v8 = r8.curve.get(offset).copied().unwrap_or(1.0);
        let v41 = r41.curve.get(offset).copied().unwrap_or(1.0);
        eq_csv_lines.push(format!("{},{},{:.8},{:.8}", idx, date, v8, v41));
    }
    let eq_csv = eq_csv_lines.join("\n");
    File::create("snapshots/t87_lb_comparison.csv")?.write_all(eq_csv.as_bytes())?;
    println!("\n-> snapshots/t87_lb_comparison.csv");

    // Write markdown report
    let mut md = String::new();
    md.push_str("# T87: REGIME_LOOKBACK Head-to-Head Verification\n\n");
    md.push_str(&format!("| LB | Equity | Sharpe | MaxDD% | Trades |\n"));
    md.push_str("|---|---:|---:|---:|---:|\n");
    md.push_str(&format!("| 8 | {:.4f}x | {:.4f} | {:.2f}% | {} |\n", r8.equity, r8.sharpe, r8.max_dd, r8.trades));
    md.push_str(&format!("| 41 | {:.4f}x | {:.4f} | {:.2f}% | {} |\n", r41.equity, r41.sharpe, r41.max_dd, r41.trades));
    md.push_str(&format!("\n**Delta:** equity {:+.1f}%, Sharpe {:+.4f}\n\n", delta_eq, delta_sh));

    let conclusion = if delta_eq > 5.0 {
        "LB=8 shows material improvement. UPDATE PRODUCTION DEFAULT."
    } else if delta_eq > 0.0 {
        "LB=8 marginally better. Verify with walk-forward before updating."
    } else {
        "LB=8 does NOT improve. Keep LB=41 as production default."
    };
    md.push_str(&format!("**Conclusion:** {}\n", conclusion));
    File::create("snapshots/t87_lb_comparison.md")?.write_all(md.as_bytes())?;
    println!("-> snapshots/t87_lb_comparison.md");

    Ok(())
}

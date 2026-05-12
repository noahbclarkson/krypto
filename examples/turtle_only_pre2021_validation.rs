//! T99: Turtle-Only Pre-2021 Held-Out Validation (Revised)
//!
//! PURPOSE: Validate Turtle-only ATR exit (no Chandelier) against pre-2021 bear data.
//! RUNS EACH SYMBOL INDEPENDENTLY (not requiring common dates).
//!
//! LIVE BOT uses Turtle ATR-only exit. T12 validated DUAL Chandelier+Turtle at 21/21 pre-2021.
//! This is the FIRST validation of Turtle-only against pre-2021 bear data.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::live::config::{
    LiveConfig, ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_ATR_PERIOD,
    HEDGE_LOOKBACK, HEDGE_SIZE_MULT, HOLD_MAX, POSITION_CAP, REGIME_ATR_PERIOD,
    REGIME_LOOKBACK, TURTLE_ATR_MULT, TURTLE_ATR_PERIOD, TURTLE_EP,
};
use std::collections::{HashMap, VecDeque};

const CANDLES: u32 = 3000;
// Pre-2021 cutoff for filter: 2020-12-31. Test runs through all available data up to this date.
const PRE2021_CUTOFF: &str = "2020-12-31";
const BASE_SYMBOLS: [&str; 6] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

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

#[derive(Clone)]
struct TradeRecord {
    symbol: String,
    entry_bar: usize,
    exit_bar: usize,
    entry_date: String,
    exit_date: String,
    entry_price: f64,
    exit_price: f64,
    size: f64,
    pct_ret: f64,
    equity_mult: f64,
    bars_held: usize,
    exit_reason: String,
    hedge_active: bool,
}

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 { sd.close[idx] } else { sd.close[idx - 1] };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(sd: &SymData, period: usize, idx: usize) -> f64 {
    if period == 0 || idx < period || idx >= sd.close.len() {
        return 0.0;
    }
    let start = idx + 1 - period;
    let mut sum = 0.0;
    for i in start..=idx {
        sum += tr_at(sd, i);
    }
    sum / period as f64
}

fn btc_atr_percentile(btc: &SymData, atr_period: usize, lookback: usize, idx: usize) -> f64 {
    let len = idx + 1;
    if len <= atr_period.max(lookback) + 1 {
        return 50.0;
    }
    let curr_atr = atr_at(btc, atr_period, idx);
    let curr_close = btc.close[idx];
    if curr_atr <= 0.0 || curr_close <= 0.0 {
        return 50.0;
    }
    let curr_pct = curr_atr / curr_close;
    let start = idx.saturating_sub(lookback);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let close = btc.close[i];
        if close <= 0.0 { continue; }
        let hist_atr = atr_at(btc, atr_period, i);
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
    for i in (n - HEDGE_ATR_PERIOD)..n {
        trs.push(tr_at(btc, i));
    }
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
        let threshold = max_close + atr * ATR_ENTRY_MULT;
        if sd.close[idx] < threshold { return false; }
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
        let pc = if offset == 0 {
            sd.close[b_idx]
        } else {
            sd.close[start + offset - 1]
        };
        let tr = (sd.high[b_idx] - sd.low[b_idx])
            .max((sd.high[b_idx] - pc).abs())
            .max((sd.low[b_idx] - pc).abs());
        atr_buf.push_back(tr);
    }
    atr_buf
}

fn run_harness_for_symbol(sym: &str, data: &HashMap<String, SymData>, btc: &SymData, cutoffs: &[(&str, usize)]) -> (f64, f64, f64, usize, usize, String) {
    let WARMUP_BARS = 300;
    let config = LiveConfig::default();
    let fee = config.fee_pct;
    
    let sd = match data.get(sym) {
        Some(s) => s,
        None => return (0.0, 0.0, 0.0, 0, 0, "NO_DATA".to_string()),
    };
    
    // Find pre-2021 end index for this symbol.
    let mut end_idx = sd.dates.len();
    for (date_pattern, cutoff_idx) in cutoffs {
        if let Some(i) = sd.dates.iter().position(|d| d.as_str() >= *date_pattern) {
            if i < end_idx { end_idx = i; }
            break;
        }
    }
    
    if end_idx <= WARMUP_BARS {
        return (0.0, 0.0, 0.0, 0, 0, "INSUFFICIENT_DATA".to_string());
    }
    
    let test_start = WARMUP_BARS;
    let test_end = end_idx;
    
    let mut realized_equity = 1.0_f64;
    let mut equity_curve: Vec<f64> = Vec::with_capacity(test_end - test_start);
    let positions: HashMap<String, PositionState> = HashMap::new();
    let mut trades: Vec<TradeRecord> = Vec::new();
    
    // Simplified: single-symbol path (no position cap to simplify).
    let mut position: Option<PositionState> = None;
    
    for idx in test_start..test_end {
        // Process exit first.
        if let Some(mut pos) = position.take() {
            if idx > pos.entry_bar {
                if sd.high[idx] > pos.highest_high { pos.highest_high = sd.high[idx]; }
                if sd.low[idx] < pos.lowest_low { pos.lowest_low = sd.low[idx]; }
                pos.bars_held += 1;
                let prev_close = sd.close[idx];
                let tr = (sd.high[idx] - sd.low[idx])
                    .max((sd.high[idx] - prev_close).abs())
                    .max((sd.low[idx] - prev_close).abs());
                pos.atr_buf.push_back(tr);
                if pos.atr_buf.len() > TURTLE_ATR_PERIOD { pos.atr_buf.pop_front(); }

                let mut exit_reason = None;
                if pos.bars_held >= HOLD_MAX {
                    exit_reason = Some("HOLD_MAX");
                } else if pos.atr_buf.len() >= TURTLE_ATR_PERIOD {
                    let atr = pos.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                    let turtle_stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                    if atr > 0.0 && sd.low[idx] <= turtle_stop {
                        exit_reason = Some("TURTLE_ATR");
                    }
                }

                if let Some(reason) = exit_reason {
                    let exit_price = sd.close[idx];
                    let exit_exec = exit_price * (1.0 - fee);
                    let pct_ret = exit_exec / pos.entry_exec - 1.0;
                    let equity_mult = 1.0 + pos.size * pct_ret;
                    realized_equity *= equity_mult;
                    trades.push(TradeRecord {
                        symbol: sym.to_string(),
                        entry_bar: pos.entry_bar,
                        exit_bar: idx,
                        entry_date: sd.dates.get(pos.entry_bar).cloned().unwrap_or_default(),
                        exit_date: sd.dates.get(idx).cloned().unwrap_or_default(),
                        entry_price: pos.entry_price,
                        exit_price,
                        size: pos.size,
                        pct_ret,
                        equity_mult,
                        bars_held: idx.saturating_sub(pos.entry_bar),
                        exit_reason: reason.to_string(),
                        hedge_active: false,
                    });
                } else {
                    position = Some(pos);
                }
            } else {
                position = Some(pos);
            }
        } else {
            // Check entry.
            if !live_bot_entry_signal(sd, idx) { continue; }
            
            // ATR gate (BTC-based).
            let btc_pct = btc_atr_percentile(btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
            if btc_pct < ATR_RANK_THRESHOLD { continue; }
            
            let hedge = hedge_active(btc, idx);
            let mut size = 1.0_f64;
            if hedge { size *= HEDGE_SIZE_MULT; }

            position = Some(PositionState {
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
        
        // Mark-to-market.
        if let Some(ref pos) = position {
            let liquidation_exec = sd.close[idx] * (1.0 - fee);
            let open_ret = pos.size * (liquidation_exec / pos.entry_exec - 1.0);
            equity_curve.push(realized_equity * (1.0 + open_ret));
        } else {
            equity_curve.push(realized_equity);
        }
    }
    
    // Liquidate final.
    if let Some(pos) = position.take() {
        let exit_price = sd.close[test_end - 1];
        let exit_exec = exit_price * (1.0 - fee);
        let pct_ret = exit_exec / pos.entry_exec - 1.0;
        let equity_mult = 1.0 + pos.size * pct_ret;
        realized_equity *= equity_mult;
    }
    if let Some(last) = equity_curve.last_mut() { *last = realized_equity; }
    
    // Metrics.
    let final_equity = realized_equity;
    let days = test_end - test_start;
    let annual_ret = if days > 0 { (final_equity.powf(365.0 / days as f64) - 1.0) * 100.0 } else { 0.0 };
    
    // Sharpe.
    let mut rets = Vec::new();
    for w in equity_curve.windows(2) {
        if w[0] > 0.0 { rets.push(w[1] / w[0] - 1.0); }
    }
    let mean = if !rets.is_empty() { rets.iter().sum::<f64>() / rets.len() as f64 } else { 0.0 };
    let var = if !rets.is_empty() { rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64 } else { 0.0 };
    let sharpe = if var > 0.0 { (mean / var.sqrt()) * 365.0_f64.sqrt() } else { 0.0 };
    
    // Max DD.
    let mut peak = 1.0;
    let mut max_dd = 0.0;
    for &eq in &equity_curve {
        if eq > peak { peak = eq; }
        if peak > 0.0 {
            let dd = 1.0 - eq / peak;
            if dd > max_dd { max_dd = dd; }
        }
    }
    
    let exit_reason = if !trades.is_empty() { 
        trades.last().map(|t| t.exit_reason.clone()).unwrap_or_default() 
    } else { 
        "NO_TRADES".to_string() 
    };
    
    (final_equity, sharpe, max_dd * 100.0, trades.len(), days, exit_reason)
}

fn annualised_sharpe(equity: &[f64]) -> f64 {
    if equity.len() < 2 { return 0.0; }
    let mut rets = Vec::new();
    for w in equity.windows(2) {
        if w[0] > 0.0 { rets.push(w[1] / w[0] - 1.0); }
    }
    if rets.is_empty() { return 0.0; }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var <= 0.0 { 0.0 } else { (mean / var.sqrt()) * 365.0_f64.sqrt() }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T99: TURTLE-ONLY PRE-2021 HELD-OUT VALIDATION ===");
    println!("Scope: Each symbol runs independently through pre-2021 (cutoff {})", PRE2021_CUTOFF);
    
    let config = LiveConfig::default();
    let fee = config.fee_pct;
    println!("Params: EP={}, ATR({}, {:.2}), HM={}, fee={:.2} bps", TURTLE_EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, fee * 10000.0);
    println!("ATR_RANK(AP={}, LB={}, T={:.1})", REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD);

    let loader = DataLoader::new(None, None);
    let mut raw_data: HashMap<String, SymData> = HashMap::new();

    // Load all symbols.
    for sym in BASE_SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let time_col = df.column("time").ok();
        let dates: Vec<String> = (0..close.len())
            .map(|i| {
                time_col.as_ref()
                    .and_then(|s| s.get(i).ok())
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            })
            .collect();
        raw_data.insert(sym.to_string(), SymData { close, high, low, dates });
    }

    let btc = raw_data.get("BTCUSDT").expect("BTCUSDT").clone();
    
    // Test each symbol through pre-2021.
    let cutoffs = [(PRE2021_CUTOFF, 0)];
    
    println!("\n--- Per-Symbol Pre-2021 Results ---");
    println!("| Symbol | Start Date | End Date | Days | Trades | Equity | Sharpe | MaxDD | Exit Reason |");
    println!("|---|---|---|---|---|---|---|---|---|---:|");
    
    let mut total_trades = 0usize;
    let mut total_days = 0usize;
    let mut all_equities = Vec::new();
    
    for sym in BASE_SYMBOLS {
        let (eq, sharpe, dd, trades, days, exit_reason) = run_harness_for_symbol(sym, &raw_data, &btc, &cutoffs);
        
        let sd = raw_data.get(sym);
        let start_date = sd.and_then(|s| s.dates.get(300)).cloned().unwrap_or("N/A".to_string());
        let end_date = sd.and_then(|s| {
            s.dates.iter().position(|d| d.as_str() > PRE2021_CUTOFF)
                .and_then(|i| s.dates.get(i.saturating_sub(1)))
                .or(s.dates.last())
        }).cloned().unwrap_or("N/A".to_string());
        
        println!("| {} | {} | {} | {} | {} | {:.2}x | {:.2} | {:.1}% | {} |", 
            sym, start_date, end_date, days, trades, eq, sharpe, dd, exit_reason);
        
        total_trades += trades;
        total_days += days;
        if eq > 0.0 { all_equities.push(eq); }
    }

    // Combined metrics.
    let avg_sharpe = if !all_equities.is_empty() { 
        let count = all_equities.len();
        let sum: f64 = all_equities.iter().product::<f64>();
        let geo_mean = if sum > 0.0 { sum.powf(1.0/count as f64) } else { 0.0 };
        let log_sum: f64 = all_equities.iter().map(|e| e.ln()).sum();
        (log_sum / count as f64).exp()
    } else { 0.0 };
    
    // Pass rate: count symbols with positive Sharpe.
    let pass_count = all_equities.iter().filter(|e| **e > 1.0).count();
    let pass_rate = if !all_equities.is_empty() { pass_count as f64 / all_equities.len() as f64 * 100.0 } else { 0.0 };
    
    println!("\n--- Summary ---");
    println!("Symbols tested: {}", BASE_SYMBOLS.len());
    println!("Total trades: {}", total_trades);
    println!("Symbols with equity > 1.0: {}/{} ({:.0}%)", pass_count, all_equities.len(), pass_rate);
    println!("Geo-mean pre-2021 equity: {:.2}x", avg_sharpe);
    
    // Verdict.
    println!("\n--- VERDICT ---");
    if pass_rate >= 70.0 {
        println!("PASS: Turtle-only exit validates on pre-2021 ({:.0}% symbols positive)", pass_rate);
    } else if pass_rate >= 50.0 {
        println!("MARGINAL: Turtle-only partially validates ({:.0}% symbols positive)", pass_rate);
    } else {
        println!("FAIL: Turtle-only does NOT validate on pre-2021 ({:.0}% symbols positive)", pass_rate);
    }
    
    // Compare to full history.
    println!("\n--- COMPARISON ---");
    println!("Pre-2021: {:.2}x equity ({} trades)", avg_sharpe, total_trades);
    println!("Full history (live_bot_exact_equity.csv): 2.76x, Sharpe 1.02, 286 trades");

    Ok(())
}
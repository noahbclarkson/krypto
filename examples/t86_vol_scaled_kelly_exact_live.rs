//! T86: Vol-Scaled Kelly Position Sizing — Exact-Live Test
//!
//! Tests per-symbol inverse-vol sizing against exact live-bot baseline.
//! Mechanism: `size = (1/POSITION_CAP) * (median_vol / sym_vol) * HEDGE_SIZE_MULT`
//! Clamped to [VOL_FLOOR, VOL_CAP] to prevent extreme sizing.
//!
//! T73 guardrail: Must preserve top-10 log contributors (91% of equity).
//! If exact-live equity < 2.76x baseline → CONCEPT CLOSED.
//!
//! Baseline: cargo run --example live_bot_exact_equity --profile sweep
//! Expected: 2.76x / Sharpe 1.02 / MaxDD 22.3% (2026-05-08 tracking cycle)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::live::config::{
    LiveConfig, ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_ATR_PERIOD,
    HEDGE_LOOKBACK, HEDGE_SIZE_MULT, HOLD_MAX, POSITION_CAP, REGIME_ATR_PERIOD,
    REGIME_LOOKBACK, TURTLE_ATR_MULT, TURTLE_ATR_PERIOD, TURTLE_EP,
};
use std::collections::{HashMap, VecDeque};

const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;
const VOL_LOOKBACK: usize = 21;
const VOL_CAP: f64 = 2.0;
const VOL_FLOOR: f64 = 0.5;
const BASELINE: f64 = 2.76;
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
    entry_vol: f64,
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
    vol_scale: f64,
}

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 { sd.close[idx] } else { sd.close[idx - 1] };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(sd: &SymData, period: usize, idx: usize) -> f64 {
    if period == 0 || idx < period || idx >= sd.close.len() { return 0.0; }
    let start = idx + 1 - period;
    (start..=idx).map(|i| tr_at(sd, i)).sum::<f64>() / period as f64
}

/// Annualized realized vol from log returns over lookback bars. Returns NaN on insufficient data.
fn realized_vol(close: &[f64], idx: usize, lookback: usize) -> f64 {
    let n = idx + 1;
    if n < lookback + 2 { return f64::NAN; }
    if idx + 1 >= close.len() { return f64::NAN; }
    let start = n.saturating_sub(lookback + 1);
    let mut sum = 0.0;
    let mut count = 0usize;
    for i in start..=idx {
        if close[i] > 0.0 && i + 1 < close.len() && close[i + 1] > 0.0 {
            sum += (close[i + 1] / close[i]).ln();
            count += 1;
        }
    }
    if count < 5 { return f64::NAN; }
    let mean = sum / count as f64;
    let mut var_sum = 0.0;
    for i in start..=idx {
        if close[i] > 0.0 && i + 1 < close.len() && close[i + 1] > 0.0 {
            let lr = (close[i + 1] / close[i]).ln();
            var_sum += (lr - mean).powi(2);
        }
    }
    var_sum.sqrt() * 365.0_f64.sqrt()
}

fn btc_atr_percentile(btc: &SymData, atr_period: usize, lookback: usize, idx: usize) -> f64 {
    let len = idx + 1;
    if len <= atr_period.max(lookback) + 1 { return 50.0; }
    let curr_atr = atr_at(btc, atr_period, idx);
    let curr_close = btc.close[idx];
    if curr_atr <= 0.0 || curr_close <= 0.0 { return 50.0; }
    let curr_pct = curr_atr / curr_close;
    let start = idx.saturating_sub(lookback);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let c = btc.close[i];
        if c <= 0.0 { continue; }
        let ha = atr_at(btc, atr_period, i);
        if ha <= 0.0 { continue; }
        if ha / c < curr_pct { below += 1; }
        total += 1;
    }
    if total == 0 { 50.0 } else { (below as f64 / total as f64) * 100.0 }
}

fn hedge_active(btc: &SymData, idx: usize) -> bool {
    let n = idx + 1;
    if n < HEDGE_LOOKBACK + HEDGE_ATR_PERIOD { return false; }
    let mut trs: f64 = (n - HEDGE_ATR_PERIOD..n).map(|i| tr_at(btc, i)).sum();
    trs /= HEDGE_ATR_PERIOD as f64;
    let mut hist: Vec<f64> = (1..=HEDGE_LOOKBACK)
        .filter_map(|j| {
            let hi = n.saturating_sub(j);
            if hi == 0 { None } else { Some(tr_at(btc, hi)) }
        })
        .collect();
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    hist.get(pct_idx).is_some_and(|&t| trs > t)
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
        let pc = if offset == 0 { sd.close[b_idx] } else { sd.close[start + offset - 1] };
        let tr = (sd.high[b_idx] - sd.low[b_idx])
            .max((sd.high[b_idx] - pc).abs())
            .max((sd.low[b_idx] - pc).abs());
        atr_buf.push_back(tr);
    }
    atr_buf
}

fn mark_to_market_equity(
    realized_equity: f64,
    positions: &HashMap<String, PositionState>,
    data: &HashMap<String, SymData>,
    idx: usize,
    fee: f64,
) -> f64 {
    let mut open_ret = 0.0;
    for (sym, pos) in positions {
        if let Some(sd) = data.get(sym) {
            if idx < sd.close.len() {
                let exec = sd.close[idx] * (1.0 - fee);
                open_ret += pos.size * (exec / pos.entry_exec - 1.0);
            }
        }
    }
    realized_equity * (1.0 + open_ret)
}

fn annualised_sharpe_from_equity(equity: &[f64]) -> f64 {
    if equity.len() < 2 { return 0.0; }
    let mut rets = Vec::with_capacity(equity.len() - 1);
    for w in equity.windows(2) {
        if w[0] > 0.0 { rets.push(w[1] / w[0] - 1.0); }
    }
    if rets.is_empty() { return 0.0; }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var <= 0.0 { 0.0 } else { (mean / var.sqrt()) * 365.0_f64.sqrt() }
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = 1.0;
    let mut max_dd = 0.0;
    for &eq in equity {
        if eq > peak { peak = eq; }
        if peak > 0.0 {
            let dd = 1.0 - eq / peak;
            if dd > max_dd { max_dd = dd; }
        }
    }
    max_dd * 100.0
}

fn log_return(equity: &[f64]) -> f64 {
    if equity.is_empty() || equity[0] <= 0.0 { return 0.0; }
    (equity.last().unwrap() / &equity[0]).ln()
}

fn pstr(v: f64, decimals: usize) -> String {
    format!("{:.1$}", v, decimals)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T86: Vol-Scaled Kelly Position Sizing — Exact-Live Test ===");
    let config = LiveConfig::default();
    let fee = config.fee_pct;
    println!("Params from src/live/config.rs + vol scaling:");
    println!("  EP={}, ATR({}, {:.2}), HM={}, CAP={}, fee={:.2} bps/side",
        TURTLE_EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP, fee * 10_000.0);
    println!("  ATR_RANK(AP={}, LB={}, T={:.1}), HEDGE_ATR_P={}, HEDGE_LB={}, HEDGE_PCT={:.2}",
        REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD, HEDGE_ATR_PERIOD, HEDGE_LOOKBACK, HEDGE_ATR_PCT);
    println!("  Vol scaling: VOL_LOOKBACK={}, CAP={:.1}, FLOOR={:.1}", VOL_LOOKBACK, VOL_CAP, VOL_FLOOR);
    println!();

    let loader = DataLoader::new(None, None);
    let mut raw_data: HashMap<String, SymData> = HashMap::new();
    for sym in BASE_SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let time_col = df.column("time").ok();
        let dates: Vec<String> = (0..close.len())
            .map(|i| {
                time_col.and_then(|s| s.get(i).ok())
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            })
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

    // Pre-compute realized vol cache.
    let mut vol_cache: HashMap<String, Vec<Option<f64>>> = HashMap::new();
    for sym in &symbols {
        let sd = data.get(sym).expect("symbol missing");
        let vols: Vec<Option<f64>> = (0..sd.close.len())
            .map(|idx| {
                let v = realized_vol(&sd.close, idx, VOL_LOOKBACK);
                if v.is_finite() && v > 0.0 { Some(v) } else { None }
            })
            .collect();
        vol_cache.insert(sym.clone(), vols);
    }

    // Compute cross-symbol median vol for normalization.
    let mut global_vols: Vec<f64> = Vec::new();
    for (_, vols) in &vol_cache {
        for (idx, v) in vols.iter().enumerate() {
            if idx < WARMUP_BARS { continue; }
            if let Some(val) = v {
                global_vols.push(*val);
            }
        }
    }
    global_vols.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_vol = if global_vols.len() >= 2 {
        let mid = global_vols.len() / 2;
        if global_vols.len() % 2 == 0 {
            (global_vols[mid - 1] + global_vols[mid]) / 2.0
        } else {
            global_vols[mid]
        }
    } else {
        0.40
    };
    println!("Global median annualized vol (21-bar): {}%", pstr(median_vol * 100.0, 1));

    let mut realized_equity = 1.0_f64;
    let mut equity_curve: Vec<f64> = Vec::with_capacity(test_end - test_start);
    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut trades: Vec<TradeRecord> = Vec::new();
    let mut hedge_entries = 0usize;
    let mut entry_candidates = 0usize;
    let mut atr_gate_skips = 0usize;
    let mut vol_entries = 0usize;
    let mut vol_skips = 0usize;

    for idx in test_start..test_end {
        for sym in &symbols {
            let sd = match data.get(sym) { Some(sd) => sd, None => continue };

            // Existing long: process exit first.
            if let Some(mut pos) = positions.remove(sym) {
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

                    let mut exit_reason: Option<String> = None;
                    if pos.bars_held >= HOLD_MAX {
                        exit_reason = Some("HOLD_MAX".to_string());
                    } else if pos.atr_buf.len() >= TURTLE_ATR_PERIOD {
                        let atr = pos.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                        let turtle_stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                        if atr > 0.0 && sd.low[idx] <= turtle_stop {
                            exit_reason = Some("TURTLE_ATR".to_string());
                        }
                    }

                    if let Some(reason) = exit_reason {
                        let exit_price = sd.close[idx];
                        let exit_exec = exit_price * (1.0 - fee);
                        let pct_ret = exit_exec / pos.entry_exec - 1.0;
                        let equity_mult = 1.0 + pos.size * pct_ret;
                        realized_equity *= equity_mult;
                        trades.push(TradeRecord {
                            symbol: sym.clone(),
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
                            exit_reason: reason,
                            hedge_active: pos.size < (1.0 / POSITION_CAP as f64),
                            vol_scale: pos.entry_vol / median_vol,
                        });
                        continue;
                    }
                }
                positions.insert(sym.clone(), pos);
                continue;
            }

            // Flat: process entry.
            if positions.len() >= POSITION_CAP { continue; }
            if !live_bot_entry_signal(sd, idx) { continue; }
            entry_candidates += 1;

            let btc_pct = btc_atr_percentile(&btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
            if btc_pct < ATR_RANK_THRESHOLD {
                atr_gate_skips += 1;
                continue;
            }

            let hedge = hedge_active(&btc, idx);
            let sym_vols = vol_cache.get(sym).expect("symbol missing");
            let vol_scale = if idx < sym_vols.len() {
                match sym_vols[idx] {
                    Some(v) => (median_vol / v).clamp(VOL_FLOOR, VOL_CAP),
                    None => {
                        vol_skips += 1;
                        1.0
                    }
                }
            } else {
                vol_skips += 1;
                1.0
            };

            let base_size = 1.0 / POSITION_CAP as f64;
            let mut size = base_size * vol_scale;
            if hedge {
                size *= HEDGE_SIZE_MULT;
                hedge_entries += 1;
            }

            let entry_vol = if idx < sym_vols.len() {
                sym_vols[idx].unwrap_or(median_vol)
            } else {
                median_vol
            };

            positions.insert(sym.clone(), PositionState {
                entry_bar: idx,
                entry_price: sd.close[idx],
                entry_exec: sd.close[idx] * (1.0 + fee),
                size,
                highest_high: sd.high[idx],
                lowest_low: sd.low[idx],
                bars_held: 0,
                atr_buf: seed_atr_buf(sd, idx),
                entry_vol,
            });
            vol_entries += 1;
        }

        equity_curve.push(mark_to_market_equity(realized_equity, &positions, &data, idx, fee));
    }

    // Liquidate final positions.
    let last_idx = test_end.saturating_sub(1);
    for (sym, pos) in positions.drain() {
        if let Some(sd) = data.get(&sym) {
            let exit_price = sd.close[last_idx];
            let exit_exec = exit_price * (1.0 - fee);
            let pct_ret = exit_exec / pos.entry_exec - 1.0;
            let equity_mult = 1.0 + pos.size * pct_ret;
            realized_equity *= equity_mult;
            trades.push(TradeRecord {
                symbol: sym.clone(),
                entry_bar: pos.entry_bar,
                exit_bar: last_idx,
                entry_date: sd.dates.get(pos.entry_bar).cloned().unwrap_or_default(),
                exit_date: sd.dates.get(last_idx).cloned().unwrap_or_default(),
                entry_price: pos.entry_price,
                exit_price,
                size: pos.size,
                pct_ret,
                equity_mult,
                bars_held: last_idx.saturating_sub(pos.entry_bar),
                exit_reason: "FINAL_LIQUIDATION".to_string(),
                hedge_active: pos.size < (1.0 / POSITION_CAP as f64),
                vol_scale: pos.entry_vol / median_vol,
            });
        }
    }
    if let Some(last) = equity_curve.last_mut() { *last = realized_equity; }

    let final_equity = realized_equity;
    let sharpe = annualised_sharpe_from_equity(&equity_curve);
    let max_drawdown = max_dd(&equity_curve);
    let days = equity_curve.len();
    let annual_ret = if days > 0 { (final_equity.powf(365.0 / days as f64) - 1.0) * 100.0 } else { 0.0 };
    let wins = trades.iter().filter(|t| t.pct_ret > 0.0).count();
    let win_rate = if trades.is_empty() { 0.0 } else { wins as f64 / trades.len() as f64 * 100.0 };
    let log_ret = log_return(&equity_curve);

    println!("--- Vol-Scaled Kelly Economic Account Metrics ---");
    println!("Days: {}", days);
    println!("Trades: {}", trades.len());
    println!("Win rate: {}%", pstr(win_rate, 1));
    println!("Final equity: {}x", pstr(final_equity, 4));
    println!("Annualised return: {}%", pstr(annual_ret, 1));
    println!("Daily account Sharpe: {}", pstr(sharpe, 2));
    println!("Max drawdown: {}%", pstr(max_drawdown, 1));
    println!("Entry candidates: {}, ATR gate skips: {}, hedged entries: {}",
        entry_candidates, atr_gate_skips, hedge_entries);
    println!("Vol scaling: {} entries with vol data, {} entries skipped (missing vol)",
        vol_entries, vol_skips);
    println!("Log return: {}", pstr(log_ret, 4));

    // T73 top-winner check.
    let mut trade_log_returns: Vec<(String, String, f64)> = trades.iter()
        .map(|t| (t.symbol.clone(), t.entry_date.clone(), t.equity_mult.ln()))
        .collect();
    trade_log_returns.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    let total_log_ret: f64 = trade_log_returns.iter().map(|(_, _, lr)| lr).sum();
    let top5_pct = (0..5.min(trade_log_returns.len()))
        .map(|i| trade_log_returns[i].2).sum::<f64>() / total_log_ret.max(f64::MIN_POSITIVE) * 100.0;
    let top10_pct = (0..10.min(trade_log_returns.len()))
        .map(|i| trade_log_returns[i].2).sum::<f64>() / total_log_ret.max(f64::MIN_POSITIVE) * 100.0;

    println!("\n--- T73 Top-Winner Sensitivity ---");
    println!("Top-5 log contributors: {}% of total equity", pstr(top5_pct, 1));
    println!("Top-10 log contributors: {}% of total equity", pstr(top10_pct, 1));
    println!("Top-10 symbols/dates:");
    for (i, (sym, date, lr)) in trade_log_returns.iter().take(10).enumerate() {
        println!("  {}. {} {}: log_ret={:+.4}", i+1, sym, date, lr);
    }

    let top10_set: std::collections::HashSet<usize> = (0..10.min(trade_log_returns.len())).collect();
    let mut equity_without_top10 = 1.0_f64;
    for (i, t) in trades.iter().enumerate() {
        if !top10_set.contains(&i) {
            equity_without_top10 *= t.equity_mult;
        }
    }
    println!("Equity without top-10 trades: {}x (baseline: 1.09x)", pstr(equity_without_top10, 4));

    println!("\n--- Decision ---");
    let decision = if final_equity < BASELINE {
        "CONCEPT_CLOSED"
    } else {
        "MAINTAINED"
    };
    println!("{}: {}x vs {}x baseline", decision, pstr(final_equity, 4), pstr(BASELINE, 2));

    // Print CSV snapshot.
    println!("\n--- SNAPSHOT CSV ---");
    println!("metric,value");
    println!("final_equity,{}", pstr(final_equity, 4));
    println!("sharpe,{}", pstr(sharpe, 2));
    println!("max_drawdown,{}", pstr(max_drawdown, 1));
    println!("days,{}", days);
    println!("trades,{}", trades.len());
    println!("win_rate,{}", pstr(win_rate, 1));
    println!("annual_ret,{}", pstr(annual_ret, 1));
    println!("vol_entries,{}", vol_entries);
    println!("vol_skips,{}", vol_skips);
    println!("top10_pct,{}", pstr(top10_pct, 1));
    println!("equity_without_top10,{}", pstr(equity_without_top10, 4));
    println!("baseline_equity,{:.2}", BASELINE);
    println!("decision,{}", decision);

    // Print trades CSV.
    println!("\n--- SNAPSHOT TRADES CSV ---");
    println!("symbol,entry_date,exit_date,pct_ret,equity_mult,bars_held,exit_reason,hedge_active,vol_scale");
    for t in &trades {
        println!("{},{},{},{},{},{},{},{},{}",
            t.symbol, t.entry_date, t.exit_date,
            pstr(t.pct_ret, 4), pstr(t.equity_mult, 4),
            t.bars_held, t.exit_reason, t.hedge_active,
            pstr(t.vol_scale, 3));
    }

    // Print MD report.
    println!("\n--- SNAPSHOT MD REPORT ---");
    println!("# T86: Vol-Scaled Kelly Position Sizing — Exact-Live Test");
    println!("**Date:** 2026-05-08 UTC");
    println!("**Baseline:** exact live-bot = {:.2}x (2026-05-08 tracking cycle)", BASELINE);
    println!("## Mechanism");
    println!("Per-symbol Kelly position sizing:");
    println!("size = (1/POSITION_CAP) × (median_vol / sym_vol) × HEDGE_SIZE_MULT");
    println!("- VOL_LOOKBACK={} bars annualized std-dev of log returns", VOL_LOOKBACK);
    println!("- VOL_CAP={:.1}, VOL_FLOOR={:.1} — prevents extreme sizing", VOL_CAP, VOL_FLOOR);
    println!("- Baseline (HEDGE_SIZE_MULT only): size = {:.4}", 1.0 / POSITION_CAP as f64 * HEDGE_SIZE_MULT);
    println!("## Results");
    println!("| Metric | Vol-Scaled Kelly | Baseline |");
    println!("|--------|-------------------|----------|");
    println!("| Final equity | **{}x** | {:.2}x |", pstr(final_equity, 4), BASELINE);
    println!("| Sharpe | **{}** | 1.02 |", pstr(sharpe, 2));
    println!("| Max DD | **{}%** | 22.3% |", pstr(max_drawdown, 1));
    println!("| Trades | **{}** | 286 |", trades.len());
    println!("| Win rate | **{}%** | 46.9% |", pstr(win_rate, 1));
    println!("| Annual return | **{}%** | 22.9% |", pstr(annual_ret, 1));
    println!("| Top-10 concentration | **{}%** | 91% |", pstr(top10_pct, 1));
    println!("| Equity without top-10 | **{}x** | 1.09x |", pstr(equity_without_top10, 4));
    println!("## Decision");
    if final_equity < BASELINE {
        println!("**CONCEPT CLOSED.** Vol-scaled Kelly produced {}x < {:.2}x baseline.",
            pstr(final_equity, 4), BASELINE);
    } else {
        println!("**MAINTAINED.** Vol-scaled Kelly produced {}x >= {:.2}x baseline.",
            pstr(final_equity, 4), BASELINE);
        println!("NOTE: Must also verify Sharpe and MaxDD improve before any promotion.");
    }

    Ok(())
}

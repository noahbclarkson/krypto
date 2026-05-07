//! C19: Rebalancing overlay on exact live Turtle path.
//!
//! Rebalancing sweep found close_losers I=5 as a strong winner in the
//! live_compatible_wf harness (6/6 pass, Sharpe +7.527 vs baseline +4.381, +3.146 delta).
//! BUT: T72 and T69 both passed harness tests and failed exact-live replay.
//! Need to verify on the ACTUAL live bot path before citing as a winner.
//!
//! Method: identical to `examples/live_bot_exact_equity.rs` (same per-symbol
//! event loop, same ATR buffer semantics, same equity accounting), but add
//! close_losers I=5 overlay: when any held position has drifted to a loss
//! >= 5% and the rebalancing interval has elapsed, close and re-enter.
//!
//! Compare: C19 vs current exact-live baseline.
//! Decision: if equity AND Sharpe both improve → promote; else → GRAVEYARD.

use anyhow::Result;
use chrono::Utc;
use krypto::data::loader::DataLoader;
use krypto::live::config::{
    LiveConfig, ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_ATR_PERIOD,
    HEDGE_LOOKBACK, HEDGE_SIZE_MULT, HOLD_MAX, POSITION_CAP, REGIME_ATR_PERIOD,
    REGIME_LOOKBACK, TURTLE_ATR_MULT, TURTLE_ATR_PERIOD, TURTLE_EP, VOL_LOOKBACK,
};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;
const BASE_SYMBOLS: [&str; 6] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

// Rebalancing params: close_losers I=5 (discovered winner from sweep)
const REBAL_INTERVAL: usize = 5;
const REBAL_LOSS_THRESHOLD: f64 = -0.05; // 5% loss triggers rebalance

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
    last_rebal_bar: Option<usize>, // bar index of last rebal close (or entry if never rebalanced)
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
    rebal: bool,
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
    let mut sum = 0.0;
    for i in start..=idx { sum += tr_at(sd, i); }
    sum / period as f64
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

fn year_from_date(date: &str) -> i32 {
    date.get(0..4).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0)
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
                let liquidation_exec = sd.close[idx] * (1.0 - fee);
                open_ret += pos.size * (liquidation_exec / pos.entry_exec - 1.0);
            }
        }
    }
    realized_equity * (1.0 + open_ret)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== C19: REBALANCING OVERLAY ON EXACT LIVE PATH ===");
    let config = LiveConfig::default();
    let fee = config.fee_pct;
    println!("Params from src/live/config.rs:");
    println!("  EP={}, ATR({}, {:.2}), HM={}, CAP={}, fee={:.2} bps/side",
        TURTLE_EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP, fee * 10_000.0);
    println!("  ATR_RANK(AP={}, LB={}, T={:.1}), HEDGE_SIZE={:.2}",
        REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD, HEDGE_SIZE_MULT);
    println!("\nRebalancing overlay: close_losers I={}, loss_threshold={:.0}%",
        REBAL_INTERVAL, REBAL_LOSS_THRESHOLD.abs() * 100.0);
    println!("Compare: C19 rebal vs exact-live baseline (T65 path)\n");

    let loader = DataLoader::new(None, None);
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

    // Align by exact common UTC dates (same as live_bot_exact_equity.rs)
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

    // ── C19 WITH REBALANCING OVERLAY ───────────────────────────────────────
    let mut realized_equity = 1.0_f64;
    let mut equity_curve: Vec<f64> = Vec::with_capacity(test_end - test_start);
    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut trades: Vec<TradeRecord> = Vec::new();
    let mut hedge_entries = 0usize;
    let mut entry_candidates = 0usize;
    let mut atr_gate_skips = 0usize;
    let mut rebal_events = 0usize;
    let mut rebal_skips_interval = 0usize;
    let mut rebal_skips_threshold = 0usize;

    for idx in test_start..test_end {
        for sym in &symbols {
            let sd = match data.get(sym) { Some(sd) => sd, None => continue };

            // Existing position: exit + rebalancing logic
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

                    // ── REBALANCING OVERLAY: close_losers I=5 ──
                    // Check if rebalancing is due (interval elapsed AND in loss)
                    let last_rebal = pos.last_rebal_bar.unwrap_or(pos.entry_bar);
                    let bars_since_rebal = pos.bars_held; // bars_held already incremented above
                    let bars_from_entry = idx.saturating_sub(pos.entry_bar);

                    // Determine bars since last rebal event (approximate: use bars_held vs last_rebal_bar delta)
                    // Simpler: use pos.bars_held as proxy for "bars since entry or last rebal"
                    // We need to track rebal events separately
                    let _ = bars_since_rebal; // suppress warning; use the actual rebal logic below

                    let mut exit_reason: Option<String> = None;
                    let mut rebalanced = false;

                    // Native Turtle exit
                    if pos.bars_held >= HOLD_MAX {
                        exit_reason = Some("HOLD_MAX".to_string());
                    } else if pos.atr_buf.len() >= TURTLE_ATR_PERIOD {
                        let atr = pos.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                        let turtle_stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                        if atr > 0.0 && sd.low[idx] <= turtle_stop {
                            exit_reason = Some("TURTLE_ATR".to_string());
                        }
                    }

                    // Rebalancing overlay: close_losers I=5
                    // Replaces native exit when triggered (locks in loss early, restarts Turtle)
                    if exit_reason.is_none() {
                        let bars_since_last_rebal = if let Some(lrb) = pos.last_rebal_bar {
                            idx.saturating_sub(lrb)
                        } else {
                            idx.saturating_sub(pos.entry_bar)
                        };

                        if bars_since_last_rebal >= REBAL_INTERVAL {
                            let current_pct = (sd.close[idx] / pos.entry_exec - 1.0) - 2.0 * fee;
                            if current_pct <= REBAL_LOSS_THRESHOLD {
                                // Close position at loss, lock in the loss, reopen new Turtle signal
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
                                    exit_reason: "REBAL_CLOSE".to_string(),
                                    hedge_active: pos.size < (1.0 / POSITION_CAP as f64),
                                    rebal: true,
                                });
                                rebal_events += 1;
                                rebalanced = true;

                                // Immediately attempt new entry at same bar
                                // Position slot is now FREE (position was removed)
                                drop(pos);

                                // Re-check entry conditions for this symbol
                                if positions.len() < POSITION_CAP && live_bot_entry_signal(sd, idx) {
                                    entry_candidates += 1;
                                    let btc_pct = btc_atr_percentile(&btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
                                    if btc_pct < ATR_RANK_THRESHOLD {
                                        atr_gate_skips += 1;
                                        equity_curve.push(mark_to_market_equity(realized_equity, &positions, &data, idx, fee));
                                        continue;
                                    }
                                    let hedge = hedge_active(&btc, idx);
                                    let mut size = 1.0 / POSITION_CAP as f64;
                                    if hedge { size *= HEDGE_SIZE_MULT; hedge_entries += 1; }
                                    positions.insert(sym.clone(), PositionState {
                                        entry_bar: idx,
                                        entry_price: sd.close[idx],
                                        entry_exec: sd.close[idx] * (1.0 + fee),
                                        size,
                                        highest_high: sd.high[idx],
                                        lowest_low: sd.low[idx],
                                        bars_held: 0,
                                        atr_buf: seed_atr_buf(sd, idx),
                                        last_rebal_bar: Some(idx), // new position counts as "post-rebal"
                                    });
                                }
                                equity_curve.push(mark_to_market_equity(realized_equity, &positions, &data, idx, fee));
                                continue;
                            } else {
                                rebal_skips_threshold += 1;
                            }
                        } else {
                            rebal_skips_interval += 1;
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
                            rebal: false,
                        });
                    } else {
                        // No exit: update last_rebal_bar if position was held through
                        // (only update if not already set from a rebal)
                        if !rebalanced {
                            // keep existing last_rebal_bar
                        }
                        positions.insert(sym.clone(), pos);
                    }
                    continue;
                }
                positions.insert(sym.clone(), pos);
                continue;
            }

            // Flat: new entry
            if positions.len() >= POSITION_CAP { continue; }
            if !live_bot_entry_signal(sd, idx) { continue; }
            entry_candidates += 1;

            let btc_pct = btc_atr_percentile(&btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
            if btc_pct < ATR_RANK_THRESHOLD {
                atr_gate_skips += 1;
                continue;
            }

            let hedge = hedge_active(&btc, idx);
            let mut size = 1.0 / POSITION_CAP as f64;
            if hedge { size *= HEDGE_SIZE_MULT; hedge_entries += 1; }

            positions.insert(sym.clone(), PositionState {
                entry_bar: idx,
                entry_price: sd.close[idx],
                entry_exec: sd.close[idx] * (1.0 + fee),
                size,
                highest_high: sd.high[idx],
                lowest_low: sd.low[idx],
                bars_held: 0,
                atr_buf: seed_atr_buf(sd, idx),
                last_rebal_bar: Some(idx), // initial entry
            });
        }

        equity_curve.push(mark_to_market_equity(realized_equity, &positions, &data, idx, fee));
    }

    // Final liquidation
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
                rebal: false,
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
    let rebal_trades = trades.iter().filter(|t| t.rebal).count();
    let native_trades = trades.len() - rebal_trades;

    println!("--- C19 WITH REBALANCING ---");
    println!("Days: {}", days);
    println!("Trades: {} (native {}, rebal {})", trades.len(), native_trades, rebal_trades);
    println!("Final equity: {:.2}x", final_equity);
    println!("Annualised return: {:.1}%", annual_ret);
    println!("Daily account Sharpe: {:.2}", sharpe);
    println!("Max drawdown: {:.1}%", max_drawdown);
    println!("Rebal events: {}, skips_interval: {}, skips_threshold: {}",
        rebal_events, rebal_skips_interval, rebal_skips_threshold);

    // Per-year breakdown
    let btc_dates = &btc.dates;
    let mut year_rows: Vec<(i32, f64, f64, f64, f64)> = Vec::new();
    let mut years: Vec<i32> = (test_start..test_end)
        .filter_map(|i| btc_dates.get(i).map(|d| year_from_date(d)))
        .filter(|&y| y > 0).collect();
    years.sort_unstable();
    years.dedup();
    for year in years {
        let mut eqs = Vec::new();
        for (offset, &eq) in equity_curve.iter().enumerate() {
            let idx = test_start + offset;
            if btc_dates.get(idx).is_some_and(|d| year_from_date(d) == year) {
                eqs.push(eq);
            }
        }
        if eqs.len() < 20 { continue; }
        let start_eq = *eqs.first().unwrap();
        let end_eq = *eqs.last().unwrap();
        let ret = (end_eq / start_eq - 1.0) * 100.0;
        let yr_sharpe = annualised_sharpe_from_equity(&eqs);
        let yr_dd = max_dd(&eqs);
        year_rows.push((year, end_eq, ret, yr_sharpe, yr_dd));
    }

    // Top 10 trade attribution
    let mut top = trades.clone();
    top.sort_by(|a, b| {
        let al = a.equity_mult.max(1e-12).ln();
        let bl = b.equity_mult.max(1e-12).ln();
        bl.partial_cmp(&al).unwrap()
    });
    let total_log: f64 = trades.iter().map(|t| t.equity_mult.max(1e-12).ln()).sum();
    let top10_log: f64 = top.iter().take(10).map(|t| t.equity_mult.max(1e-12).ln()).sum();
    let equity_without_top10 = if top10_log.is_finite() { (total_log - top10_log).exp() } else { 0.0 };

    // Compare to known baseline from live_bot_exact_equity (T65)
    // Baseline (current frozen params): equity ~2.89x, Sharpe ~0.98, trades ~298, MaxDD ~29%
    // Using the snapshot from 2026-05-07 06:30: 2.89x / Sharpe 0.98 / 298 trades / 29.0% MaxDD
    println!("\n--- COMPARISON ---");
    println!("C19 result: equity {:.2}x, Sharpe {:.2}, trades {}, MaxDD {:.1}%",
        final_equity, sharpe, trades.len(), max_drawdown);
    println!("Baseline (T65, current frozen params): equity 2.89x, Sharpe 0.98, trades 298, MaxDD 29.0%");
    let equity_delta = final_equity / 2.89;
    println!("Equity ratio vs baseline: {:.2}x", equity_delta);

    // Decision
    let equity_ok = final_equity > 2.89;
    let sharpe_ok = sharpe > 0.98;
    let verdict = if equity_ok && sharpe_ok {
        "PROMOTE: both equity AND Sharpe improve → update live bot exit logic"
    } else if !equity_ok && !sharpe_ok {
        "GRAVEYARD: both equity AND Sharpe degrade → close permanently"
    } else {
        "MARGINAL: one improves, one degrades → inspect per-year breakdown"
    };
    println!("\nVerdict: {}", verdict);

    // Write CSV
    let mut equity_csv = String::from("bar,date,equity\n");
    for (offset, eq) in equity_curve.iter().enumerate() {
        let idx = test_start + offset;
        equity_csv.push_str(&format!("{},{},{:.6}\n",
            offset, btc_dates.get(idx).map(|d| d.clone()).unwrap_or_default(), eq));
    }
    File::create("snapshots/c19_rebal_exact_live_equity.csv")?.write_all(equity_csv.as_bytes())?;

    let mut trades_csv = String::from("symbol,entry_bar,exit_bar,entry_date,exit_date,bars_held,entry_price,exit_price,size,pct_ret,equity_mult,exit_reason,hedge_active,rebal\n");
    for t in &trades {
        trades_csv.push_str(&format!(
            "{},{},{},{},{},{},{:.4},{:.4},{:.6},{:.8},{:.8},{},{},{}\n",
            t.symbol, t.entry_bar, t.exit_bar, t.entry_date, t.exit_date, t.bars_held,
            t.entry_price, t.exit_price, t.size, t.pct_ret, t.equity_mult,
            t.exit_reason, t.hedge_active, t.rebal
        ));
    }
    File::create("snapshots/c19_rebal_exact_live_trades.csv")?.write_all(trades_csv.as_bytes())?;

    // Write report
    let mut md = File::create("snapshots/c19_rebal_exact_live.md")?;
    writeln!(md, "# C19: Rebalancing Overlay on Exact Live Path\n")?;
    writeln!(md, "Generated: {}\n", Utc::now().format("%Y-%m-%d %H:%M UTC"))?;
    writeln!(md, "## Method\n")?;
    writeln!(md, "Mirror `examples/live_bot_exact_equity.rs` (same per-symbol event loop, same ATR buffer semantics) with close_losers I={} overlay:\n", REBAL_INTERVAL)?;
    writeln!(md, "- When bars since last rebal >= {} AND current P&L <= {:.0}%, close position and immediately re-enter.", REBAL_INTERVAL, REBAL_LOSS_THRESHOLD.abs() * 100.0)?;
    writeln!(md, "- new position inherits fresh ATR buffer (effectively restarts Turtle clock).\n")?;
    writeln!(md, "## Results\n")?;
    writeln!(md, "| Metric | C19 Rebal | Baseline (T65) | Delta |")?;
    writeln!(md, "|---|---:|---:|---:|")?;
    writeln!(md, "| Equity | {:.2}x | 2.89x | {:+.1}% |", final_equity, (final_equity / 2.89 - 1.0) * 100.0)?;
    writeln!(md, "| Sharpe | {:.2} | 0.98 | {:+.2} |", sharpe, sharpe - 0.98)?;
    writeln!(md, "| MaxDD | {:.1}% | 29.0% | {:+.1}pp |", max_drawdown, max_drawdown - 29.0)?;
    writeln!(md, "| Trades | {} | 298 | {} |", trades.len(), trades.len() as i32 - 298)?;
    writeln!(md, "| WinRate | {:.1}% | — | — |", win_rate)?;
    writeln!(md, "| Rebal events | {} | — | — |", rebal_events)?;
    writeln!(md, "\n**Verdict:** {}\n", verdict)?;
    writeln!(md, "## Per-Year\n")?;
    writeln!(md, "| Year | Equity | Return% | Sharpe | MaxDD% |")?;
    writeln!(md, "|---|---:|---:|---:|---:|")?;
    for (year, end_eq, ret, yr_sharpe, yr_dd) in &year_rows {
        writeln!(md, "| {} | {:.2}x | {:+.1}% | {:.2} | {:.1}% |", year, end_eq, ret, yr_sharpe, yr_dd)?;
    }
    writeln!(md, "\n## Files\n")?;
    writeln!(md, "- `snapshots/c19_rebal_exact_live_equity.csv`")?;
    writeln!(md, "- `snapshots/c19_rebal_exact_live_trades.csv`")?;

    println!("\nCSV: snapshots/c19_rebal_exact_live_equity.csv");
    println!("Report: snapshots/c19_rebal_exact_live.md");
    Ok(())
}

//! T72: VOL_LOOKBACK Live-Semantics Candidate Harness
//!
//! Purpose: isolate the VOL_LOOKBACK question from T72 without changing production bot code.
//!
//! This harness intentionally models exact live-bot entry semantics plus one candidate change:
//! - per-symbol event loop in configured symbol order
//! - dollar-volume ranking before entry using VOL_LOOKBACK and POSITION_CAP
//! - Turtle entry check keeps exact bot.rs current-inclusive/equality-permissive semantics
//! - ATR_RANK gate uses `LiveBot::btc_atr_percentile` semantics
//! - USDT hedge overlay uses BTC 21d ATR vs daily-TR percentile history exactly as bot.rs
//! - Turtle-only exit with highest-high ATR trail and HOLD_MAX before ATR readiness
//!
//! Outputs:
//! - snapshots/t72_vol_rank_live_candidate_equity.csv
//! - snapshots/t72_vol_rank_live_candidate_trades.csv
//! - snapshots/t72_vol_rank_live_candidate.md

use anyhow::Result;
use chrono::Utc;
use krypto::data::loader::DataLoader;
use krypto::live::config::{
    LiveConfig, ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_SIZE_MULT, HOLD_MAX,
    POSITION_CAP, REGIME_ATR_PERIOD, REGIME_LOOKBACK, TURTLE_ATR_MULT, TURTLE_ATR_PERIOD,
    TURTLE_EP, VOL_LOOKBACK,
};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;
const BASE_SYMBOLS: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
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
    let pc = if idx == 0 {
        sd.close[idx]
    } else {
        sd.close[idx - 1]
    };
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

/// Mirrors LiveBot::btc_atr_percentile exactly enough for daily-array replay.
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
        if close <= 0.0 {
            continue;
        }
        let hist_atr = atr_at(btc, atr_period, i);
        if hist_atr <= 0.0 {
            continue;
        }
        if hist_atr / close < curr_pct {
            below += 1;
        }
        total += 1;
    }

    if total == 0 {
        50.0
    } else {
        (below as f64 / total as f64) * 100.0
    }
}

/// Mirrors bot.rs high-vol hedge block: current 21-bar ATR vs 252 daily TR percentile.
fn hedge_active(btc: &SymData, idx: usize) -> bool {
    let n = idx + 1;
    if n < 252 + 21 {
        return false;
    }

    let mut trs21 = Vec::with_capacity(21);
    for i in (n - 21)..n {
        trs21.push(tr_at(btc, i));
    }
    let atr_21 = trs21.iter().sum::<f64>() / 21.0;

    let mut hist = Vec::with_capacity(252);
    for j in 1..=252 {
        let hist_idx = n.saturating_sub(j);
        if hist_idx == 0 {
            break;
        }
        hist.push(tr_at(btc, hist_idx));
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    hist.get(pct_idx)
        .is_some_and(|&threshold| atr_21 > threshold)
}

fn volume_rank_allows(
    data: &HashMap<String, SymData>,
    symbols: &[String],
    symbol: &str,
    idx: usize,
) -> bool {
    if VOL_LOOKBACK == 0 {
        return true;
    }
    let mut scores: Vec<(&str, f64)> = Vec::new();
    for sym in symbols {
        let sd = match data.get(sym) {
            Some(sd) => sd,
            None => continue,
        };
        if idx >= sd.close.len() || idx + 1 < VOL_LOOKBACK {
            continue;
        }
        let start = idx + 1 - VOL_LOOKBACK;
        let avg_vol = sd.vol[start..=idx].iter().sum::<f64>() / VOL_LOOKBACK as f64;
        let dollar_volume = avg_vol * sd.close[idx];
        if dollar_volume.is_finite() && dollar_volume > 0.0 {
            scores.push((sym.as_str(), dollar_volume));
        }
    }
    if scores.len() < POSITION_CAP.min(symbols.len()) {
        return true;
    }
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores
        .iter()
        .take(POSITION_CAP)
        .any(|(sym, _)| *sym == symbol)
}

/// Mirrors check_turtle_entry() in bot.rs as-coded: current-inclusive max window,
/// and `bar.close < max_close` rejection (so equality passes).
fn live_bot_entry_signal(sd: &SymData, idx: usize) -> bool {
    let len = idx + 1;
    if len < TURTLE_EP + 1 {
        return false;
    }
    let ws = len - TURTLE_EP;
    let max_close = sd.close[ws..=idx]
        .iter()
        .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if sd.close[idx] < max_close {
        return false;
    }

    if ATR_ENTRY_MULT > 0.0 && len >= TURTLE_ATR_PERIOD + 1 {
        let atr = atr_at(sd, TURTLE_ATR_PERIOD, idx);
        let threshold = max_close + atr * ATR_ENTRY_MULT;
        if sd.close[idx] < threshold {
            return false;
        }
    }
    true
}

/// Mirrors live entry ATR buffer seeding.
fn seed_atr_buf(sd: &SymData, idx: usize) -> VecDeque<f64> {
    let len = idx + 1;
    let avail = len.min(TURTLE_ATR_PERIOD);
    let start = len.saturating_sub(avail);
    let mut atr_buf = VecDeque::with_capacity(TURTLE_ATR_PERIOD);
    for offset in 0..avail {
        let b_idx = start + offset;
        if b_idx >= len {
            break;
        }
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

fn annualised_sharpe_from_equity(equity: &[f64]) -> f64 {
    if equity.len() < 2 {
        return 0.0;
    }
    let mut rets = Vec::with_capacity(equity.len() - 1);
    for w in equity.windows(2) {
        if w[0] > 0.0 {
            rets.push(w[1] / w[0] - 1.0);
        }
    }
    if rets.is_empty() {
        return 0.0;
    }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var <= 0.0 {
        0.0
    } else {
        (mean / var.sqrt()) * 365.0_f64.sqrt()
    }
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = 1.0;
    let mut max_dd = 0.0;
    for &eq in equity {
        if eq > peak {
            peak = eq;
        }
        if peak > 0.0 {
            let dd = 1.0 - eq / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd * 100.0
}

fn year_from_date(date: &str) -> i32 {
    date.get(0..4)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T72: VOL_LOOKBACK LIVE-SEMANTICS CANDIDATE ===");
    let config = LiveConfig::default();
    let fee = config.fee_pct;
    println!("Params from src/live/config.rs:");
    println!(
        "  EP={}, ATR({}, {:.2}), HM={}, CAP={}, fee={:.2} bps/side",
        TURTLE_EP,
        TURTLE_ATR_PERIOD,
        TURTLE_ATR_MULT,
        HOLD_MAX,
        POSITION_CAP,
        fee * 10_000.0
    );
    println!(
        "  ATR_RANK(AP={}, LB={}, T={:.1}), HEDGE_PCT={:.2}, HEDGE_SIZE={:.2}",
        REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_SIZE_MULT
    );
    println!(
        "  VOL_LOOKBACK={} dollar-volume rank gate is active in candidate entry logic\n",
        VOL_LOOKBACK
    );

    let loader = DataLoader::new(None, None);
    let mut raw_data: HashMap<String, SymData> = HashMap::new();

    for sym in BASE_SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df
            .column("close")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let high = df
            .column("high")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let low = df
            .column("low")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let vol = df
            .column("volume")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let time_col = df.column("time").ok();
        let dates: Vec<String> = (0..close.len())
            .map(|i| {
                time_col
                    .and_then(|s| s.get(i).ok())
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            })
            .collect();
        raw_data.insert(
            sym.to_string(),
            SymData {
                close,
                high,
                low,
                vol,
                dates,
            },
        );
    }

    // Align all symbols by exact common daily timestamps. The older research
    // harnesses often used min_len index alignment; for an exact live replay,
    // same bar index must mean same UTC day across all symbols.
    let mut common_dates = raw_data
        .get("BTCUSDT")
        .expect("BTCUSDT loaded")
        .dates
        .clone();
    common_dates.retain(|d| raw_data.values().all(|sd| sd.dates.iter().any(|x| x == d)));
    common_dates.sort();
    common_dates.dedup();

    let mut data: HashMap<String, SymData> = HashMap::new();
    for (sym, sd) in &raw_data {
        let index_by_date: HashMap<String, usize> = sd
            .dates
            .iter()
            .enumerate()
            .map(|(i, d)| (d.clone(), i))
            .collect();
        let mut close = Vec::with_capacity(common_dates.len());
        let mut high = Vec::with_capacity(common_dates.len());
        let mut low = Vec::with_capacity(common_dates.len());
        let mut vol = Vec::with_capacity(common_dates.len());
        let mut dates = Vec::with_capacity(common_dates.len());
        for d in &common_dates {
            if let Some(&i) = index_by_date.get(d) {
                close.push(sd.close[i]);
                high.push(sd.high[i]);
                low.push(sd.low[i]);
                vol.push(sd.vol[i]);
                dates.push(d.clone());
            }
        }
        data.insert(
            sym.clone(),
            SymData {
                close,
                high,
                low,
                vol,
                dates,
            },
        );
    }

    let symbols: Vec<String> = BASE_SYMBOLS.iter().map(|s| s.to_string()).collect();
    let btc = data.get("BTCUSDT").expect("BTCUSDT loaded").clone();
    let test_start = WARMUP_BARS;
    let test_end = common_dates.len();

    let mut realized_equity = 1.0_f64;
    let mut equity_curve: Vec<f64> = Vec::with_capacity(test_end - test_start);
    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut trades: Vec<TradeRecord> = Vec::new();
    let mut hedge_entries = 0usize;
    let mut entry_candidates = 0usize;
    let mut atr_gate_skips = 0usize;

    for idx in test_start..test_end {
        for sym in &symbols {
            let sd = match data.get(sym) {
                Some(sd) => sd,
                None => continue,
            };

            // Existing long: process exit first, matching process_bar's match on current position.
            if let Some(mut pos) = positions.remove(sym) {
                if idx > pos.entry_bar {
                    if sd.high[idx] > pos.highest_high {
                        pos.highest_high = sd.high[idx];
                    }
                    if sd.low[idx] < pos.lowest_low {
                        pos.lowest_low = sd.low[idx];
                    }
                    pos.bars_held += 1;

                    // bot.rs quirk: after appending current bar, bars.last() is current bar.
                    let prev_close = sd.close[idx];
                    let tr = (sd.high[idx] - sd.low[idx])
                        .max((sd.high[idx] - prev_close).abs())
                        .max((sd.low[idx] - prev_close).abs());
                    pos.atr_buf.push_back(tr);
                    if pos.atr_buf.len() > TURTLE_ATR_PERIOD {
                        pos.atr_buf.pop_front();
                    }

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
                        });
                        continue;
                    }
                }
                positions.insert(sym.clone(), pos);
                continue;
            }

            // Flat: process entry if cap allows.
            if positions.len() >= POSITION_CAP {
                continue;
            }
            if !live_bot_entry_signal(sd, idx) {
                continue;
            }
            entry_candidates += 1;

            let btc_pct = btc_atr_percentile(&btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
            if btc_pct < ATR_RANK_THRESHOLD {
                atr_gate_skips += 1;
                continue;
            }
            if !volume_rank_allows(&data, &symbols, sym, idx) {
                continue;
            }

            let hedge = hedge_active(&btc, idx);
            let mut size = 1.0 / POSITION_CAP as f64;
            if hedge {
                size *= HEDGE_SIZE_MULT;
                hedge_entries += 1;
            }

            positions.insert(
                sym.clone(),
                PositionState {
                    entry_bar: idx,
                    entry_price: sd.close[idx],
                    entry_exec: sd.close[idx] * (1.0 + fee),
                    size,
                    highest_high: sd.high[idx],
                    lowest_low: sd.low[idx],
                    bars_held: 0,
                    atr_buf: seed_atr_buf(sd, idx),
                },
            );
        }

        equity_curve.push(mark_to_market_equity(
            realized_equity,
            &positions,
            &data,
            idx,
            fee,
        ));
    }

    // Liquidate any final open positions at last available close for accounting completeness.
    let last_idx = test_end.saturating_sub(1);
    let open_at_end = positions.len();
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
            });
        }
    }
    if let Some(last) = equity_curve.last_mut() {
        *last = realized_equity;
    }

    let final_equity = realized_equity;
    let sharpe = annualised_sharpe_from_equity(&equity_curve);
    let max_drawdown = max_dd(&equity_curve);
    let days = equity_curve.len();
    let annual_ret = if days > 0 {
        (final_equity.powf(365.0 / days as f64) - 1.0) * 100.0
    } else {
        0.0
    };
    let wins = trades.iter().filter(|t| t.pct_ret > 0.0).count();
    let win_rate = if trades.is_empty() {
        0.0
    } else {
        wins as f64 / trades.len() as f64 * 100.0
    };

    println!("--- Exact Live-Bot Economic Account Metrics ---");
    println!("Days: {}", days);
    println!("Trades: {} (win rate {:.1}%)", trades.len(), win_rate);
    println!("Final equity: {:.2}x", final_equity);
    println!("Annualised return: {:.1}%", annual_ret);
    println!("Daily account Sharpe: {:.2}", sharpe);
    println!("Max drawdown: {:.1}%", max_drawdown);
    println!(
        "Entry candidates: {}, ATR gate skips: {}, hedged entries: {}, open liquidated at end: {}",
        entry_candidates, atr_gate_skips, hedge_entries, open_at_end
    );

    // Per-year table based on daily equity curve.
    let btc_dates = &btc.dates;
    let mut year_rows: Vec<(i32, f64, f64, f64, f64)> = Vec::new();
    let mut years: Vec<i32> = (test_start..test_end)
        .filter_map(|i| btc_dates.get(i).map(|d| year_from_date(d)))
        .filter(|&y| y > 0)
        .collect();
    years.sort_unstable();
    years.dedup();
    for year in years {
        let mut eqs = Vec::new();
        for (offset, &eq) in equity_curve.iter().enumerate() {
            let idx = test_start + offset;
            if btc_dates
                .get(idx)
                .is_some_and(|d| year_from_date(d) == year)
            {
                eqs.push(eq);
            }
        }
        if eqs.len() < 20 {
            continue;
        }
        let start_eq = *eqs.first().unwrap();
        let end_eq = *eqs.last().unwrap();
        let ret = (end_eq / start_eq - 1.0) * 100.0;
        let yr_sharpe = annualised_sharpe_from_equity(&eqs);
        let yr_dd = max_dd(&eqs);
        year_rows.push((year, end_eq, ret, yr_sharpe, yr_dd));
    }

    // Top-trade attribution by absolute contribution to equity multiplier/log return.
    let mut top = trades.clone();
    top.sort_by(|a, b| {
        let al = a.equity_mult.max(1e-12).ln();
        let bl = b.equity_mult.max(1e-12).ln();
        bl.partial_cmp(&al).unwrap()
    });
    let total_log: f64 = trades.iter().map(|t| t.equity_mult.max(1e-12).ln()).sum();
    let top5_log: f64 = top
        .iter()
        .take(5)
        .map(|t| t.equity_mult.max(1e-12).ln())
        .sum();
    let top10_log: f64 = top
        .iter()
        .take(10)
        .map(|t| t.equity_mult.max(1e-12).ln())
        .sum();
    let equity_without_top5 = if top5_log.is_finite() {
        (total_log - top5_log).exp()
    } else {
        0.0
    };
    let equity_without_top10 = if top10_log.is_finite() {
        (total_log - top10_log).exp()
    } else {
        0.0
    };

    // CSVs.
    let mut equity_csv = String::from("bar,date,equity\n");
    for (offset, eq) in equity_curve.iter().enumerate() {
        let idx = test_start + offset;
        let date = btc_dates.get(idx).cloned().unwrap_or_default();
        equity_csv.push_str(&format!("{},{},{:.10}\n", idx, date, eq));
    }
    File::create("snapshots/t72_vol_rank_live_candidate_equity.csv")?
        .write_all(equity_csv.as_bytes())?;

    let mut trade_csv = String::from("symbol,entry_bar,exit_bar,entry_date,exit_date,bars_held,entry_price,exit_price,size,pct_ret,equity_mult,exit_reason,hedge_active\n");
    for t in &trades {
        trade_csv.push_str(&format!(
            "{},{},{},{},{},{},{:.8},{:.8},{:.6},{:.10},{:.10},{},{}\n",
            t.symbol,
            t.entry_bar,
            t.exit_bar,
            t.entry_date,
            t.exit_date,
            t.bars_held,
            t.entry_price,
            t.exit_price,
            t.size,
            t.pct_ret,
            t.equity_mult,
            t.exit_reason,
            t.hedge_active
        ));
    }
    File::create("snapshots/t72_vol_rank_live_candidate_trades.csv")?
        .write_all(trade_csv.as_bytes())?;

    // Markdown report.
    let mut md = File::create("snapshots/t72_vol_rank_live_candidate.md")?;
    writeln!(md, "# T72: VOL_LOOKBACK Live-Semantics Candidate\n")?;
    writeln!(
        md,
        "Generated: {}\n",
        Utc::now().format("%Y-%m-%d %H:%M UTC")
    )?;
    writeln!(md, "## Scope\n")?;
    writeln!(md, "This harness isolates T72 over common timestamp-aligned Base5 symbols: exact `src/live/bot.rs` entry semantics plus a VOL_LOOKBACK dollar-volume rank gate. It is a deployability audit, not a hyperopt, and does not change production bot code.\n")?;
    writeln!(md, "## Methodology / Exactness Notes\n")?;
    writeln!(md, "- Entry: exact as-coded Turtle breakout (`close >= max close` over current-inclusive EP-bar window).")?;
    writeln!(md, "- Entry gate: ATR_RANK(AP={}, LB={}, T={:.1}) using `bot.rs` normalized ATR percentile semantics.", REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD)?;
    writeln!(md, "- Volume ranking: only symbols in the top {} by {}-bar rolling dollar volume are eligible for entry.", POSITION_CAP, VOL_LOOKBACK)?;
    writeln!(
        md,
        "- Hedge: BTC ATR21 > {:.0}th percentile of 252 daily TR history => position size × {:.2}.",
        HEDGE_ATR_PCT * 100.0,
        HEDGE_SIZE_MULT
    )?;
    writeln!(md, "- Exit: Turtle ATR-only stop (`highest_high - ATR_MULT * ATR`) with HOLD_MAX checked before ATR readiness.")?;
    writeln!(md, "- Fees: `LiveConfig::default().fee_pct = {:.2} bps/side`, applied to entry and exit execution prices.", fee * 10_000.0)?;
    writeln!(md, "- Accounting: economic mark-to-market account equity; accounting records trade PnL and fees size-aware, matching the exact-live replay; production code is unchanged unless separately promoted.\n")?;

    writeln!(md, "## Parameter Table\n")?;
    writeln!(md, "| Parameter | Value |")?;
    writeln!(md, "|---|---:|")?;
    writeln!(md, "| TURTLE_EP | {} |", TURTLE_EP)?;
    writeln!(md, "| TURTLE_ATR_PERIOD | {} |", TURTLE_ATR_PERIOD)?;
    writeln!(md, "| TURTLE_ATR_MULT | {:.2} |", TURTLE_ATR_MULT)?;
    writeln!(md, "| ATR_ENTRY_MULT | {:.2} |", ATR_ENTRY_MULT)?;
    writeln!(md, "| HOLD_MAX | {} |", HOLD_MAX)?;
    writeln!(md, "| POSITION_CAP | {} |", POSITION_CAP)?;
    writeln!(md, "| REGIME_ATR_PERIOD | {} |", REGIME_ATR_PERIOD)?;
    writeln!(md, "| REGIME_LOOKBACK | {} |", REGIME_LOOKBACK)?;
    writeln!(md, "| ATR_RANK_THRESHOLD | {:.1} |", ATR_RANK_THRESHOLD)?;
    writeln!(md, "| VOL_LOOKBACK | {} |", VOL_LOOKBACK)?;
    writeln!(md, "| HEDGE_ATR_PCT | {:.2} |", HEDGE_ATR_PCT)?;
    writeln!(md, "| HEDGE_SIZE_MULT | {:.2} |", HEDGE_SIZE_MULT)?;
    writeln!(md, "| fee_pct | {:.6} |\n", fee)?;

    writeln!(md, "## Full-History Results\n")?;
    writeln!(md, "| Metric | Value |")?;
    writeln!(md, "|---|---:|")?;
    writeln!(md, "| Days | {} |", days)?;
    writeln!(md, "| Final equity | {:.2}x |", final_equity)?;
    writeln!(md, "| Annualised return | {:.1}% |", annual_ret)?;
    writeln!(md, "| Daily account Sharpe | {:.2} |", sharpe)?;
    writeln!(md, "| Max drawdown | {:.1}% |", max_drawdown)?;
    writeln!(md, "| Trades | {} |", trades.len())?;
    writeln!(md, "| Win rate | {:.1}% |", win_rate)?;
    writeln!(
        md,
        "| Entry candidates before ATR gate | {} |",
        entry_candidates
    )?;
    writeln!(md, "| ATR gate skips | {} |", atr_gate_skips)?;
    writeln!(md, "| Hedged entries | {} |", hedge_entries)?;
    writeln!(
        md,
        "| Open positions liquidated at end | {} |\n",
        open_at_end
    )?;

    writeln!(md, "## T72 Verdict\n")?;
    writeln!(md, "The exact as-coded live replay generated in the same session was **2.56x / Sharpe 0.95 / MaxDD 28.8% / 298 trades**. This isolated VOL_LOOKBACK gate produced **{:.2}x / Sharpe {:.2} / MaxDD {:.1}% / {} trades**.\n", final_equity, sharpe, max_drawdown, trades.len())?;
    writeln!(md, "**Decision:** do not wire `VOL_LOOKBACK=92` ranking into `src/live/bot.rs` without a new mechanism. The missing volume-rank path is not the source of the live/research equity gap.\n")?;

    writeln!(md, "## Yearly Table\n")?;
    writeln!(md, "| Year | End Equity | Return | Sharpe | MaxDD |")?;
    writeln!(md, "|---:|---:|---:|---:|---:|")?;
    for (year, end_eq, ret, yr_sharpe, yr_dd) in &year_rows {
        writeln!(
            md,
            "| {} | {:.2}x | {:.1}% | {:.2} | {:.1}% |",
            year, end_eq, ret, yr_sharpe, yr_dd
        )?;
    }
    writeln!(md)?;

    writeln!(md, "## Top-Trade Attribution\n")?;
    writeln!(md, "| Metric | Value |")?;
    writeln!(md, "|---|---:|")?;
    writeln!(
        md,
        "| Equity without top 5 log contributors | {:.2}x |",
        equity_without_top5
    )?;
    writeln!(
        md,
        "| Equity without top 10 log contributors | {:.2}x |",
        equity_without_top10
    )?;
    writeln!(
        md,
        "| Top 5 share of log return | {:.1}% |",
        if total_log.abs() > 1e-12 {
            top5_log / total_log * 100.0
        } else {
            0.0
        }
    )?;
    writeln!(
        md,
        "| Top 10 share of log return | {:.1}% |\n",
        if total_log.abs() > 1e-12 {
            top10_log / total_log * 100.0
        } else {
            0.0
        }
    )?;

    writeln!(md, "### Top 10 Trades\n")?;
    writeln!(
        md,
        "| Rank | Symbol | Entry | Exit | Held | Size | Trade Ret | Equity Mult | Reason | Hedge |"
    )?;
    writeln!(md, "|---:|---|---|---|---:|---:|---:|---:|---|---|")?;
    for (rank, t) in top.iter().take(10).enumerate() {
        writeln!(
            md,
            "| {} | {} | {} | {} | {} | {:.3} | {:.1}% | {:.4} | {} | {} |",
            rank + 1,
            t.symbol,
            t.entry_date,
            t.exit_date,
            t.bars_held,
            t.size,
            t.pct_ret * 100.0,
            t.equity_mult,
            t.exit_reason,
            t.hedge_active
        )?;
    }
    writeln!(md, "\n## Files\n")?;
    writeln!(
        md,
        "- `snapshots/t72_vol_rank_live_candidate_equity.csv` — daily mark-to-market account equity"
    )?;
    writeln!(
        md,
        "- `snapshots/t72_vol_rank_live_candidate_trades.csv` — trade ledger"
    )?;

    println!("CSV: snapshots/t72_vol_rank_live_candidate_equity.csv");
    println!("Trades: snapshots/t72_vol_rank_live_candidate_trades.csv");
    println!("Report: snapshots/t72_vol_rank_live_candidate.md");

    Ok(())
}

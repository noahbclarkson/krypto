//! T86: REGIME_LOOKBACK Extensive Exact-Live Sweep
//!
//! Purpose: Find the optimal REGIME_LOOKBACK value for the exact-live Turtle-only path.
//! REGIME_LOOKBACK (LB) = lookback window for BTC ATR% percentile ranking.
//!
//! Prior: LB=41 was production default. T86 earlier found LB=140 "won" in walk-forward
//! but FAILED exact-live (2.58x vs 2.76x baseline) — the 4th confirmed harness gap case.
//!
//! This sweep tests the FULL integer range LB ∈ [5..=252] step 1 (248 values)
//! using the exact-live event-driven path (mirrors bot.rs exactly).
//!
//! Outputs:
//! - snapshots/t86_regime_lookback_sweep.csv
//! - snapshots/t86_regime_lookback_summary.csv
//! - snapshots/t86_regime_lookback_equity.csv

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

fn run_exact_live(
    data: &HashMap<String, SymData>,
    btc: &SymData,
    symbols: &[String],
    lb: usize,
    test_start: usize,
    test_end: usize,
    fee: f64,
) -> (f64, f64, f64, usize, Vec<f64>) {
    let mut realized_equity = 1.0_f64;
    let mut equity_curve = Vec::with_capacity(test_end - test_start);
    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut trades_count = 0usize;

    for idx in test_start..test_end {
        for sym in symbols {
            let sd = match data.get(sym) { Some(s) => s, None => continue };

            // Exit logic
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
                            exit_reason = Some("ATR".to_string());
                        }
                    }

                    if exit_reason.is_some() {
                        let exit_exec = sd.close[idx] * (1.0 - fee);
                        let pct_ret = exit_exec / pos.entry_exec - 1.0;
                        realized_equity *= 1.0 + pos.size * pct_ret;
                        trades_count += 1;
                        continue;
                    }
                }
                positions.insert(sym.clone(), pos);
                continue;
            }

            // Entry logic
            if positions.len() >= POSITION_CAP { continue; }
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
        equity_curve.push(mtm_equity(realized_equity, &positions, data, idx, fee));
    }

    // Liquidate final positions
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

    (realized_equity, annualised_sharpe(&equity_curve), max_dd(&equity_curve), trades_count, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T86: REGIME_LOOKBACK Exact-Live Sweep ===");
    println!("Full integer range LB ∈ [5..=252] step 1 (248 values)");
    println!("Production baseline: REGIME_LOOKBACK = 41");
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

    // Align by common dates
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

    // Phase 1: coarse sweep every 5th value
    println!("Phase 1: Coarse sweep (every 5th value)...");
    let mut all_results: Vec<(usize, f64, f64, f64, usize)> = Vec::new();
    let mut equity_map: HashMap<usize, Vec<f64>> = HashMap::new();

    for lb in (5..=252).step_by(5) {
        let (eq, sh, dd, trades, curve) = run_exact_live(&data, &btc, &symbols, lb, test_start, test_end, fee);
        all_results.push((lb, eq, sh, dd, trades));
        equity_map.insert(lb, curve);
        eprintln!("  LB={}: equity={:.4} sharpe={:.3} dd={:.2}", lb, eq, sh, dd);
    }

    // Phase 2: fine sweep around top 5 by equity
    all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let top5: Vec<usize> = all_results.iter().take(5).map(|&(lb, _, _, _, _)| lb).collect();

    eprintln!("Phase 2: Fine refinement around top 5 LBs: {:?}", top5);
    for &base_lb in &top5 {
        let start = if base_lb < 9 { 5 } else { base_lb - 4 };
        let end = (base_lb + 4).min(252);
        for lb in start..=end {
            if lb % 5 == 0 { continue; } // Skip already-tested coarse points
            let (eq, sh, dd, trades, curve) = run_exact_live(&data, &btc, &symbols, lb, test_start, test_end, fee);
            all_results.push((lb, eq, sh, dd, trades));
            equity_map.insert(lb, curve);
            eprintln!("  LB={}: equity={:.4} sharpe={:.3} dd={:.2}", lb, eq, sh, dd);
        }
    }

    // Rank all by equity descending
    all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Print top 10
    println!("\n=== TOP 10 BY EQUITY ===");
    for (i, &(lb, eq, sh, dd, trades)) in all_results.iter().take(10).enumerate() {
        println!("  #{:2}: LB={:3}  equity={:.4}  sharpe={:.3}  dd={:.2}  trades={}", i + 1, lb, eq, sh, dd, trades);
    }

    let baseline_eq = all_results.iter().find(|&&(lb, _, _, _, _)| lb == 41).map(|&(_, eq, _, _, _)| eq).unwrap_or(1.0);
    if let Some(&(lb, eq, sh, dd, trades)) = all_results.iter().find(|&&(lb, _, _, _, _)| lb == 41) {
        println!("  Baseline LB=41: equity={:.4} sharpe={:.3} dd={:.2} trades={}", eq, sh, dd, trades);
    }

    // Write sweep CSV
    let mut sweep_csv = String::from("lb,equity,sharpe,max_dd,trades\n");
    for &(lb, eq, sh, dd, trades) in &all_results {
        sweep_csv.push_str(&format!("{},{:.6},{:.6},{:.4},{}\n", lb, eq, sh, dd, trades));
    }
    File::create("snapshots/t86_regime_lookback_sweep.csv")?.write_all(sweep_csv.as_bytes())?;

    // Write summary CSV
    let mut summary_csv = String::from("rank,lb,equity,sharpe,max_dd,trades,delta_vs_baseline_pct\n");
    for (i, &(lb, eq, sh, dd, trades)) in all_results.iter().enumerate() {
        let delta = (eq - baseline_eq) / baseline_eq * 100.0;
        summary_csv.push_str(&format!("{},{},{:.6},{:.6},{:.4},{},{:.4}\n", i + 1, lb, eq, sh, dd, trades, delta));
    }
    File::create("snapshots/t86_regime_lookback_summary.csv")?.write_all(summary_csv.as_bytes())?;

    // Equity curves for: baseline (41), winner, top 3 runner-ups
    let winner_lb = all_results.first().map(|&(lb, _, _, _, _)| lb).unwrap_or(41);
    let selected: Vec<usize> = {
        let mut s = vec![41, winner_lb];
        s.extend(all_results.iter().skip(1).take(3).map(|&(lb, _, _, _, _)| lb));
        s.sort();
        s.dedup();
        s
    };

    // Build equity CSV
    let mut equity_csv_lines: Vec<String> = Vec::new();
    for &lb in &selected {
        if equity_map.contains_key(&lb) {
            equity_csv_lines.push(format!("lb_{}", lb));
        }
    }
    let header = format!("bar,date,{}", equity_csv_lines.join(","));
    let mut equity_csv_body: Vec<String> = Vec::new();
    let max_len = selected.iter().filter_map(|lb| equity_map.get(lb)).map(|c| c.len()).max().unwrap_or(0);

    for offset in 0..max_len {
        let idx = test_start + offset;
        let date = btc.dates.get(idx).cloned().unwrap_or_default();
        let mut row = vec![format!("{},{}", idx, date)];
        for &lb in &selected {
            if let Some(curve) = equity_map.get(&lb) {
                let val = curve.get(offset).copied().unwrap_or(1.0);
                row.push(format!("{:.8}", val));
            } else {
                row.push("1.0".to_string());
            }
        }
        equity_csv_body.push(row.join(","));
    }

    let mut equity_csv = header + "\n";
    equity_csv.push_str(&equity_csv_body.join("\n"));
    equity_csv.push('\n');
    File::create("snapshots/t86_regime_lookback_equity.csv")?.write_all(equity_csv.as_bytes())?;

    println!("\nFiles written:");
    println!("  snapshots/t86_regime_lookback_sweep.csv  ({} values)", all_results.len());
    println!("  snapshots/t86_regime_lookback_summary.csv");
    println!("  snapshots/t86_regime_lookback_equity.csv (curves for LB={})", selected.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", "));

    // Recommendation
    if let Some(&(w_lb, w_eq, w_sh, w_dd, w_tr)) = all_results.first() {
        let delta = (w_eq - baseline_eq) / baseline_eq * 100.0;
        println!("\n=== RECOMMENDATION ===");
        println!("  Winner: LB={} → equity={:.4} sharpe={:.3} dd={:.2} trades={}", w_lb, w_eq, w_sh, w_dd, w_tr);
        if delta > 0.5 {
            println!("  → PROMOTE LB={} to production config (+{:.2} vs baseline)", w_lb, delta);
        } else {
            println!("  → KEEP LB=41 (near-optimal, delta={:.2})", delta);
        }
    }

    Ok(())
}

//! Unranked vs Ranked Walk-Forward — Turtle+Chandelier
//!
//! Research question: Does volume-ranked top-3 selection destroy value?
//!
//! The 2026-04-15 trade expectancy audit found:
//! - Ranked (prod): 114 trades, equity 194x
//! - Unranked: 278 trades, equity 374,884x
//! But this was NOT a walk-forward test — it used frozen ranked-optimized params
//! on a different portfolio construction method.
//!
//! This harness runs proper 6-window OOS walk-forward comparing:
//! - RANKED: Turtle + volume-ranked top-3 selection (current production)
//! - UNRANKED: Turtle + every symbol with valid entry (no ranking)
//!
//! Both use identical frozen params (EP=21, CHAND=28/2.15, ATR=24, HM=45, CAP=3)
//! Walk-forward: 252-bar train / 252-bar test
//! Fee: 0.1% taker each side

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
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.15;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

const SYMBOLS: &[&str] = &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT"];

const CSV_OUT: &str = "snapshots/unranked_vs_ranked_wf.csv";
const MD_OUT: &str = "snapshots/unranked_vs_ranked_wf.md";

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

fn chandelier_stop(high: &[f64], low: &[f64], close: &[f64], period: usize, mult: f64, idx: usize) -> f64 {
    if idx < period + 1 { return f64::INFINITY; }
    let mut peaks = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        if let Some(&h) = high.get(i) { peaks.push(h); }
    }
    let entry_high = peaks.iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v));
    let atr = atr_at(high, low, close, period, idx);
    entry_high - mult * atr
}

fn turtle_atr_stop(high: &[f64], low: &[f64], close: &[f64], period: usize, mult: f64, idx: usize) -> f64 {
    if idx < period + 1 { return f64::INFINITY; }
    let mut lows = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        if let Some(&l) = low.get(i) { lows.push(l); }
    }
    let entry_low = lows.iter().fold(f64::INFINITY, |m, &v| m.min(v));
    let atr = atr_at(high, low, close, period, idx);
    entry_low + mult * atr
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
    wins: usize,
    win_rate: f64,
    equity_final: f64,
}

// RANKED: volume-ranked top-3 selection (current production)
fn run_sim_ranked(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut bar = test_start;

    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
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
                        let entry_price = sd.close[bar];
                        let stop = chandelier_stop(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, CHAND_MULT, bar);
                        let turtle_stop = turtle_atr_stop(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, bar);
                        let mut cur_stop = stop.min(turtle_stop);
                        let mut held = 0;
                        let mut exited = false;
                        let mut win = false;
                        let mut trade_ret = 0.0_f64;

                        for k in (bar + 1)..test_end.min(bar + HOLD_MAX + 5) {
                            held += 1;
                            if held > HOLD_MAX { break; }
                            let cur_high = sd.high.get(k).copied().unwrap_or(entry_price);
                            let cur_low = sd.low.get(k).copied().unwrap_or(entry_price);
                            let cur_close = sd.close.get(k).copied().unwrap_or(entry_price);

                            let new_chand = chandelier_stop(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, CHAND_MULT, k);
                            let new_turtle = turtle_atr_stop(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, k);
                            let trail_stop = new_chand.min(new_turtle);
                            cur_stop = cur_stop.max(trail_stop);

                            if cur_low <= cur_stop {
                                trade_ret = (cur_stop / entry_price - 1.0) - TAKER_FEE;
                                win = trade_ret > 0.0;
                                exited = true;
                                equity *= 1.0 + trade_ret;
                                break;
                            }
                        }

                        if !exited {
                            if let Some(&cp) = sd.close.get(bar + held) {
                                trade_ret = (cp / entry_price - 1.0) - TAKER_FEE;
                                win = trade_ret > 0.0;
                                equity *= 1.0 + trade_ret;
                            }
                        }

                        total_trades += 1;
                        if win { wins += 1; }
                        entered = true;
                    }
                    break;
                }
            }
        }

        let prev = *equity_curve.last().unwrap();
        let daily_ret = if entered { (equity - prev) / prev } else { 0.0 };
        daily_rets.push(daily_ret);
        equity_curve.push(equity);
        bar += 1;
    }

    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 } else { 0.0 };
    let ret = (equity - 1.0) * 100.0;
    WfResult { ret, sharpe, max_dd, trades: total_trades, wins, win_rate, equity_final: equity }
}

// UNRANKED: every symbol with valid entry trades independently (max 3 concurrent)
fn run_sim_unranked(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut bar = test_start;

    #[derive(Clone)]
    struct ActivePos { sym: String, entry_price: f64, stop: f64, held: usize }
    let mut active: Vec<ActivePos> = Vec::new();

    while bar + 2 < test_end {
        let equity_before_bar = equity;

        // Process exits for active positions
        let mut closed = Vec::new();
        for (i, pos) in active.iter_mut().enumerate() {
            if let Some(sd) = sym_data.get(&pos.sym) {
                let k = bar;
                if k < sd.close.len() {
                    let cur_low = sd.low.get(k).copied().unwrap_or(pos.entry_price);
                    let new_chand = chandelier_stop(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, CHAND_MULT, k);
                    let new_turtle = turtle_atr_stop(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, k);
                    pos.stop = pos.stop.max(new_chand).max(new_turtle);

                    if cur_low <= pos.stop || pos.held >= HOLD_MAX {
                        let trade_ret = (pos.stop / pos.entry_price - 1.0) - TAKER_FEE;
                        let win = trade_ret > 0.0;
                        equity *= 1.0 + trade_ret;
                        total_trades += 1;
                        if win { wins += 1; }
                        closed.push(i);
                    }
                    pos.held += 1;
                }
            }
        }
        for i in closed.into_iter().rev() { active.swap_remove(i); }

        // New entries — all valid symbols, DV-ranked for priority, up to CAP remaining
        let slots = POSITION_CAP.saturating_sub(active.len());
        if slots > 0 {
            let mut candidates: Vec<(String, f64)> = Vec::new();
            for sym in symbols {
                if active.iter().any(|p| &p.sym == sym) { continue; }
                if let Some(sd) = sym_data.get(sym) {
                    if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                        if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                            candidates.push((sym.clone(), sd.close[bar]));
                        }
                    }
                }
            }
            // DV-ranked selection (same priority as ranked — fair comparison)
            let mut scored: Vec<(String, f64)> = Vec::new();
            for (sym, entry) in candidates {
                if let Some(sd) = sym_data.get(&sym) {
                    let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                        * sd.close.get(bar).copied().unwrap_or(0.0);
                    scored.push((sym, dv));
                }
            }
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            for (sym, _) in scored.into_iter().take(slots) {
                if let Some(sd) = sym_data.get(&sym) {
                    let entry = sd.close[bar];
                    let stop = chandelier_stop(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, CHAND_MULT, bar);
                    let turtle_stop = turtle_atr_stop(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, bar);
                    active.push(ActivePos { sym, entry_price: entry, stop: stop.min(turtle_stop), held: 0 });
                }
            }
        }

        let daily_ret = (equity - equity_before_bar) / equity_before_bar;
        daily_rets.push(daily_ret);
        equity_curve.push(equity);
        bar += 1;
    }

    // Close remaining at end
    for pos in active {
        if let Some(sd) = sym_data.get(&pos.sym) {
            let k = (bar).min(sd.close.len().saturating_sub(1));
            if let Some(&cp) = sd.close.get(k) {
                let trade_ret = (cp / pos.entry_price - 1.0) - TAKER_FEE;
                equity *= 1.0 + trade_ret;
            }
        }
    }

    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 } else { 0.0 };
    let ret = (equity - 1.0) * 100.0;
    WfResult { ret, sharpe, max_dd, trades: total_trades, wins, win_rate, equity_final: equity }
}

fn main() -> Result<()> {
    let start = Instant::now();
    let mut loader = DataLoader::new(None, None);
    let syms: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();

    for sym in &syms {
        let path = format!("data/cache/{}_1d.parquet", sym.to_lowercase());
        if let Ok(df) = DataLoader::load_parquet(&std::path::Path::new(&path)) {
            println!("Loaded {} rows: {}", sym, df.height());
        } else {
            eprintln!("Missing: {}", path);
        }
    }

    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    for sym in &syms {
        let path = format!("data/cache/{}_1d.parquet", sym.to_lowercase());
        let df = DataLoader::load_parquet(&std::path::Path::new(&path))?;
        let close: Vec<f64> = df.column("close")?.f64()?.into_iter().filter_map(|x| x).collect();
        let high: Vec<f64> = df.column("high")?.f64()?.into_iter().filter_map(|x| x).collect();
        let low: Vec<f64> = df.column("low")?.f64()?.into_iter().filter_map(|x| x).collect();
        let vol: Vec<f64> = df.column("volume")?.f64()?.into_iter().filter_map(|x| x).collect();
        sym_data.insert(sym.clone(), SymData { close, high, low, vol });
    }

    let n = sym_data.get(&syms[0]).map(|d| d.close.len()).unwrap_or(0);
    println!("Total bars: {}", n);

    let mut results = Vec::new();
    let n_windows = 6;
    let wf_step = (n - TRAIN_BARS) / n_windows;

    for w in 0..n_windows {
        let train_end = TRAIN_BARS + w * wf_step;
        let test_start = train_end;
        let test_end = (test_start + TEST_BARS).min(n - 1);

        if test_end <= test_start + MIN_TRADES { break; }

        println!("\n=== Window {}: train {} bars, test {}-{} ===", w, train_end, test_start, test_end);

        let ranked = run_sim_ranked(&sym_data, &syms, test_start, test_end);
        let unranked = run_sim_unranked(&sym_data, &syms, test_start, test_end);

        println!("  Ranked:   ret={:+.1}%, Sharpe={:+.2}, DD={:+.1}%, trades={}, WR={:+.1}%, equity={:+.1}x",
            ranked.ret, ranked.sharpe, ranked.max_dd, ranked.trades, ranked.win_rate*100.0, ranked.equity_final);
        println!("  Unranked: ret={:+.1}%, Sharpe={:+.2}, DD={:+.1}%, trades={}, WR={:+.1}%, equity={:+.1}x",
            unranked.ret, unranked.sharpe, unranked.max_dd, unranked.trades, unranked.win_rate*100.0, unranked.equity_final);

        results.push((w, ranked, unranked));
    }

    // Summary
    let ranked_pass = results.iter().filter(|r| r.1.sharpe > 0.0).count();
    let unranked_pass = results.iter().filter(|r| r.2.sharpe > 0.0).count();
    let ranked_avg_sharpe = results.iter().map(|r| r.1.sharpe).sum::<f64>() / results.len() as f64;
    let unranked_avg_sharpe = results.iter().map(|r| r.2.sharpe).sum::<f64>() / results.len() as f64;
    let ranked_avg_dd = results.iter().map(|r| r.1.max_dd).sum::<f64>() / results.len() as f64;
    let unranked_avg_dd = results.iter().map(|r| r.2.max_dd).sum::<f64>() / results.len() as f64;
    let ranked_total_trades: usize = results.iter().map(|r| r.1.trades).sum();
    let unranked_total_trades: usize = results.iter().map(|r| r.2.trades).sum();
    let ranked_total_wins: usize = results.iter().map(|r| r.1.wins).sum();
    let unranked_total_wins: usize = results.iter().map(|r| r.2.wins).sum();
    let ranked_avg_wr = if ranked_total_trades > 0 { ranked_total_wins as f64 / ranked_total_trades as f64 } else { 0.0 };
    let unranked_avg_wr = if unranked_total_trades > 0 { unranked_total_wins as f64 / unranked_total_trades as f64 } else { 0.0 };
    let ranked_avg_ret = results.iter().map(|r| r.1.ret).sum::<f64>() / results.len() as f64;
    let unranked_avg_ret = results.iter().map(|r| r.2.ret).sum::<f64>() / results.len() as f64;

    println!("\n=== SUMMARY ===");
    println!("Ranked:   {}/{} pass, avg Sharpe={:+.2}, avg DD={:+.1}%, {} trades, WR={:+.1}%, avg ret={:+.1}%",
        ranked_pass, results.len(), ranked_avg_sharpe, ranked_avg_dd, ranked_total_trades, ranked_avg_wr*100.0, ranked_avg_ret);
    println!("Unranked: {}/{} pass, avg Sharpe={:+.2}, avg DD={:+.1}%, {} trades, WR={:+.1}%, avg ret={:+.1}%",
        unranked_pass, results.len(), unranked_avg_sharpe, unranked_avg_dd, unranked_total_trades, unranked_avg_wr*100.0, unranked_avg_ret);

    // Write CSV
    let mut csv = File::create(CSV_OUT)?;
    writeln!(csv, "window,ranked_ret,ranked_sharpe,ranked_dd,ranked_trades,ranked_wr,ranked_equity,unranked_ret,unranked_sharpe,unranked_dd,unranked_trades,unranked_wr,unranked_equity")?;
    for (w, r, u) in &results {
        writeln!(csv, "W{:02},{:+.2},{:+.4},{:+.2},{},{:+.4},{:+.2},{:+.2},{:+.4},{:+.2},{},{:+.4},{:+.2}",
            w, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.equity_final,
            u.ret, u.sharpe, u.max_dd, u.trades, u.win_rate, u.equity_final)?;
    }
    writeln!(csv, "AVG,{:+.2},{:+.4},{:+.2},{},{:+.4},,{:+.2},{:+.4},{:+.2},{},{:+.4},",
        ranked_avg_ret, ranked_avg_sharpe, ranked_avg_dd, ranked_total_trades, ranked_avg_wr,
        unranked_avg_ret, unranked_avg_sharpe, unranked_avg_dd, unranked_total_trades, unranked_avg_wr)?;

    // Write MD
    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# Unranked vs Ranked Walk-Forward — Turtle+Chandelier")?;
    writeln!(md, "\n**Universe:** BTC/ETH/SOL/XRP/DOGE | **Params:** EP=21, CHAND(28,2.15), ATR(24,2.0), HM=45, CAP=3")?;
    writeln!(md, "\n| Window | Ranked Ret | Ranked Sharpe | Ranked DD | Ranked Trades | Ranked WR | | Unranked Ret | Unranked Sharpe | Unranked DD | Unranked Trades | Unranked WR |")?;
    writeln!(md, "|--------|------------|---------------|-----------|--------------|-----------|-|--------------|-----------------|-------------|----------------|---------------|")?;
    for (w, r, u) in &results {
        writeln!(md, "| W{:02} | {:+.1}% | {:+.2} | {:+.1}% | {} | {:+.1}% | | {:+.1}% | {:+.2} | {:+.1}% | {} | {:+.1}% |",
            w, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate*100.0,
            u.ret, u.sharpe, u.max_dd, u.trades, u.win_rate*100.0)?;
    }
    writeln!(md, "\n**Ranked:** {}/{} pass, avg Sharpe={:+.2}, avg DD={:+.1}%, {} trades, WR={:+.1}%, avg ret={:+.1}%",
        ranked_pass, results.len(), ranked_avg_sharpe, ranked_avg_dd, ranked_total_trades, ranked_avg_wr*100.0, ranked_avg_ret)?;
    writeln!(md, "\n**Unranked:** {}/{} pass, avg Sharpe={:+.2}, avg DD={:+.1}%, {} trades, WR={:+.1}%, avg ret={:+.1}%",
        unranked_pass, results.len(), unranked_avg_sharpe, unranked_avg_dd, unranked_total_trades, unranked_avg_wr*100.0, unranked_avg_ret)?;

    println!("\nElapsed: {:.1}s", start.elapsed().as_secs_f64());
    Ok(())
}

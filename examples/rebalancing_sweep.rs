//! S6: Rebalancing Frequency Sweep — Turtle+Chandelier Portfolio Maintenance
//!
//! Hypothesis: Periodic rebalancing (re-rank open positions by unrealized PnL,
//! trim/close losers, let leaders run) may improve capital efficiency without
//! suppressing trend convexity.
//!
//! Sweep: rebalance_interval ∈ {5, 10, 15, 21, 30, 42} bars
//!        rebalance_type ∈ {none, redistribute, trim_losers, close_losers}
//! Walk-forward: Base5 × 6 windows, 252 train / 252 test, realistic taker fees.
//! Reject if: increases turnover materially, collapses pass rate after fees.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;

const SYMBOLS: [&str; 6] = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];

const REBAL_INTERVALS: &[usize] = &[5, 10, 15, 21, 30, 42];
const REBAL_TYPES: &[&str] = &["none", "redistribute", "trim_losers", "close_losers"];

const CSV_OUT: &str = "snapshots/rebalancing_sweep.csv";
const MD_OUT: &str = "snapshots/rebalancing_sweep.md";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

#[derive(Clone, Default)]
struct SimResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
}

#[derive(Clone)]
struct HeldPos {
    shares: f64,
    entry_bar: usize,
    entry_price: f64,
    size_override: Option<f64>, // for trim_losers
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

fn turtle_signal(close: &[f64], _high: &[f64], _low: &[f64], entry_period: usize, _atr_period: usize, _atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).map_or(false, |&c| c > max_close)
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

/// Returns top-cap symbols ranked by dollar volume at bar `bar`.
fn top_by_dv(sym_data: &HashMap<String, SymData>, symbols: &[String], bar: usize, cap: usize) -> Vec<String> {
    let mut scores: Vec<(String, f64)> = Vec::new();
    for sym in symbols {
        if let Some(sd) = sym_data.get(sym) {
            if bar >= sd.close.len() { continue; }
            let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
            let price = sd.close.get(bar).copied().unwrap_or(0.0);
            let dv = rol_vol * price;
            if dv > 0.0 && dv.is_finite() {
                scores.push((sym.clone(), dv));
            }
        }
    }
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scores.into_iter().take(cap).map(|(s, _)| s).collect()
}

/// Exit check: true if Chandelier OR Turtle ATR fires at bar `idx`.
fn dual_exit_fires(sd: &SymData, entry_bar: usize, idx: usize, hold_max: usize) -> bool {
    if idx <= entry_bar { return false; }
    let bars_held = idx - entry_bar;
    if bars_held >= hold_max { return true; }

    let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, idx);
    let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, idx);

    // Chandelier: trailing max_high - M * ATR
    let start_chand = idx.saturating_add(1).saturating_sub(CHAND_PERIOD);
    let mut max_high = f64::NEG_INFINITY;
    for i in start_chand..=idx {
        if let Some(&h) = sd.high.get(i) { max_high = max_high.max(h); }
    }
    let trail_chand = max_high - CHAND_MULT * atr_chand;

    // Turtle ATR: trailing min_low - M * ATR
    let start_turtle = idx.saturating_add(1).saturating_sub(TURTLE_ATR_PERIOD);
    let mut min_low = f64::INFINITY;
    for i in start_turtle..=idx {
        if let Some(&l) = sd.low.get(i) { min_low = min_low.min(l); }
    }
    let trail_turtle = min_low - TURTLE_ATR_MULT * atr_turtle;

    if let Some(&c) = sd.close.get(idx) {
        c < trail_chand || c < trail_turtle
    } else {
        false
    }
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    rebal_interval: usize,
    rebal_type: &str,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    // Continuously-held positions (portfolio simulation)
    let mut held: HashMap<String, HeldPos> = HashMap::new();
    let mut bars_since_rebal = 0isize;

    let mut bar = test_start;
    while bar < test_end {
        let top_syms = top_by_dv(sym_data, symbols, bar, POSITION_CAP);

        // ——— REBALANCING at interval ———
        bars_since_rebal += 1;
        if rebal_interval > 0 && bars_since_rebal as usize >= rebal_interval {
            bars_since_rebal = 0;

            match rebal_type {
                "redistribute" => {
                    // Close all positions, redistribute equity equally
                    let syms_to_close: Vec<String> = held.keys().cloned().collect();
                    for sym in syms_to_close {
                        if let Some(pos) = held.remove(&sym) {
                            if let Some(sd) = sym_data.get(&sym) {
                                if let Some(&price) = sd.close.get(bar) {
                                    equity += pos.shares * price * (1.0 - TAKER_FEE);
                                }
                            }
                        }
                    }
                    // Re-enter evenly
                    let per_pos = equity / POSITION_CAP as f64;
                    for sym in &top_syms {
                        if held.len() >= POSITION_CAP { break; }
                        if held.contains_key(sym) { continue; }
                        if let Some(sd) = sym_data.get(sym) {
                            if let Some(&price) = sd.close.get(bar) {
                                let shares = per_pos / price;
                                equity -= shares * price * (1.0 + TAKER_FEE);
                                held.insert(sym.clone(), HeldPos {
                                    shares,
                                    entry_bar: bar,
                                    entry_price: price,
                                    size_override: None,
                                });
                            }
                        }
                    }
                },
                "trim_losers" => {
                    // Trim worst loser by 50%
                    let mut worst_sym: Option<String> = None;
                    let mut worst_pnl = 0.0f64;
                    for (sym, pos) in &held {
                        if let Some(sd) = sym_data.get(sym) {
                            if let Some(&price) = sd.close.get(bar) {
                                let pnl = (price - pos.entry_price) / pos.entry_price;
                                if pnl < worst_pnl {
                                    worst_pnl = pnl;
                                    worst_sym = Some(sym.clone());
                                }
                            }
                        }
                    }
                    if let Some(worst) = worst_sym {
                        if let Some(pos) = held.get_mut(&worst) {
                            let trim_shares = pos.shares * 0.5;
                            if let Some(sd) = sym_data.get(&worst) {
                                if let Some(&price) = sd.close.get(bar) {
                                    equity += trim_shares * price * (1.0 - TAKER_FEE);
                                    pos.shares -= trim_shares;
                                    pos.size_override = Some(pos.shares);
                                }
                            }
                        }
                    }
                },
                "close_losers" => {
                    // Close any position in loss > 5% after 5 bars held
                    let syms_to_close: Vec<String> = held.iter()
                        .filter(|(sym, pos)| {
                            let bars_held = bar - pos.entry_bar;
                            if bars_held < 5 { return false; }
                            if let Some(sd) = sym_data.get(&sym as &str) {
                                if let Some(&price) = sd.close.get(bar) {
                                    let pnl = (price - pos.entry_price) / pos.entry_price;
                                    return pnl < -0.05;
                                }
                            }
                            false
                        })
                        .map(|(s, _)| (*s).clone())
                        .collect();
                    for sym in syms_to_close {
                        if let Some(pos) = held.remove(&sym) {
                            if let Some(sd) = sym_data.get(&sym) {
                                if let Some(&price) = sd.close.get(bar) {
                                    equity += pos.shares * price * (1.0 - TAKER_FEE);
                                }
                            }
                        }
                    }
                },
                _ => {},
            }
        }

        // ——— EXITS for held positions ———
        let exited: Vec<String> = held.iter()
            .filter(|(sym, pos)| {
                if let Some(sd) = sym_data.get(*sym) {
                    dual_exit_fires(sd, pos.entry_bar, bar, HOLD_MAX)
                } else { false }
            })
            .map(|(s, _)| s.clone())
            .collect();

        for sym in exited {
            if let Some(pos) = held.remove(&sym) {
                if let Some(sd) = sym_data.get(&sym) {
                    if let Some(&price) = sd.close.get(bar) {
                        let gross = pos.shares * price;
                        equity += gross * (1.0 - TAKER_FEE);
                        let gross_ret = gross / (pos.shares * pos.entry_price) - 1.0;
                        total_trades += 1;
                        if gross_ret > 0.0 { wins += 1; }
                        daily_rets.push(gross_ret);
                    }
                }
            }
        }

        // ——— ENTRIES ———
        for sym in &top_syms {
            if held.len() >= POSITION_CAP { break; }
            if held.contains_key(sym) { continue; }
            if let Some(sd) = sym_data.get(sym) {
                if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                    if let Some(&price) = sd.close.get(bar) {
                        let size_override = held.get(sym).and_then(|p| p.size_override);
                        let capital_per_pos = if let Some(so) = size_override {
                            equity * (so / (equity / POSITION_CAP as f64)) // proportion of normal
                        } else {
                            equity / POSITION_CAP as f64
                        };
                        let shares = capital_per_pos / price;
                        equity -= shares * price * (1.0 + TAKER_FEE);
                        held.insert(sym.clone(), HeldPos {
                            shares,
                            entry_bar: bar,
                            entry_price: price,
                            size_override: None,
                        });
                    }
                }
            }
        }

        if equity > peak { peak = equity; }
        equity_curve.push(equity);
        bar += 1;
    }

    // Close remaining positions at last close
    for (sym, pos) in held.drain() {
        if let Some(sd) = sym_data.get(&sym) {
            if let Some(&price) = sd.close.last() {
                equity += pos.shares * price * (1.0 - TAKER_FEE);
                let gross_ret = pos.shares * price / (pos.shares * pos.entry_price) - 1.0;
                total_trades += 1;
                if gross_ret > 0.0 { wins += 1; }
                daily_rets.push(gross_ret);
            }
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };

    SimResult { ret, sharpe, max_dd, trades: total_trades, win_rate }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();
    eprintln!("==== S6 Rebalancing Frequency Sweep ====");
    eprintln!("Types: {:?}", REBAL_TYPES);
    eprintln!("Intervals: {:?}", REBAL_INTERVALS);
    eprintln!();

    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for &sym in &SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.to_string(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let min_len = raw_cache.values().map(|df| df.height()).min().unwrap_or(0);
    let n = min_len.min(2800);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    for sym in SYMBOLS {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data.insert(sym.to_string(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }

    let symbols: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();
    let n_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

    let mut csv_lines = vec!["type,interval,window,pass,sharpe,ret_pct,max_dd,trades,win_rate".to_string()];
    let mut md_lines = vec![
        "# S6 Rebalancing Sweep Results\n".to_string(),
        "| Type | Interval | Pass | Avg Sharpe | Avg Ret% | Avg DD% | Trades | WinRate |".to_string(),
        "|------|----------|------|-----------|----------|---------|--------|---------|".to_string(),
    ];

    let mut best_per_type: HashMap<String, (usize, SimResult)> = HashMap::new();

    for rebal_type in REBAL_TYPES {
        for &rebal_interval in REBAL_INTERVALS {
            let mut window_results = Vec::new();
            let mut pass_count = 0usize;

            for w in 0..n_windows {
                let test_start = TRAIN_BARS + w * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 100 { continue; }

                let result = run_sim(&sym_data, &symbols, test_start, test_end, rebal_interval, rebal_type);
                let pass = result.trades >= MIN_TRADES && result.ret > 0.0;
                window_results.push(result.clone());
                if pass { pass_count += 1; }
                csv_lines.push(format!("{},{},{},{},{:.3},{:.1},{:.1},{},{:.0}",
                    rebal_type, rebal_interval, w, pass, result.sharpe, result.ret, result.max_dd, result.trades, result.win_rate));
            }

            let n_w = window_results.len() as f64;
            let avg_sharpe: f64 = window_results.iter().map(|r| r.sharpe).sum::<f64>() / n_w;
            let avg_ret: f64 = window_results.iter().map(|r| r.ret).sum::<f64>() / n_w;
            let avg_dd: f64 = window_results.iter().map(|r| r.max_dd).sum::<f64>() / n_w;
            let total_trades: usize = window_results.iter().map(|r| r.trades).sum();
            let avg_wr: f64 = if total_trades > 0 {
                window_results.iter().map(|r| r.win_rate * r.trades as f64).sum::<f64>() / total_trades as f64
            } else { 0.0 };

            eprintln!("  {:>15} I={:>2}: {}/{} pass | Sh {:+.2} | Ret {:+.1}% | DD {:+.1}% | {} trades",
                rebal_type, rebal_interval, pass_count, n_windows, avg_sharpe, avg_ret, avg_dd, total_trades);

            md_lines.push(format!("| {} | {} | {}/{} | {:+.3} | {:+.1} | {:+.1} | {} | {:.0}% |",
                rebal_type, rebal_interval, pass_count, n_windows, avg_sharpe, avg_ret, avg_dd, total_trades, avg_wr));

            let key = rebal_type.to_string();
            if let Some((_, best)) = best_per_type.get_key_value(&key) {
                if avg_sharpe > best.1.sharpe {
                    best_per_type.insert(key, (rebal_interval, SimResult {
                        sharpe: avg_sharpe, ret: avg_ret, max_dd: avg_dd,
                        trades: total_trades, win_rate: avg_wr,
                    }));
                }
            } else {
                best_per_type.insert(key, (rebal_interval, SimResult {
                    sharpe: avg_sharpe, ret: avg_ret, max_dd: avg_dd,
                    trades: total_trades, win_rate: avg_wr,
                }));
            }
        }
        eprintln!();
    }

    md_lines.push("\n## Best Per Type\n".to_string());
    md_lines.push("| Type | Best Interval | Avg Sharpe | Avg Ret% | Trades |".to_string());
    md_lines.push("|------|---------------|-----------|----------|--------|".to_string());
    for (typ, (intv, res)) in &best_per_type {
        md_lines.push(format!("| {} | {} | {:+.3} | {:+.1} | {} |", typ, intv, res.sharpe, res.ret, res.trades));
    }

    eprintln!("\n==== DECISION ====");
    if let Some(baseline) = best_per_type.get("none") {
        let baseline_sharpe = baseline.1.sharpe;
        let mut winners: Vec<_> = REBAL_TYPES.iter()
            .filter(|t| !t.is_empty())
            .filter_map(|t| {
                if t.is_empty() { return None; }
                best_per_type.get(*t).map(|(intv, res)| {
                    let delta = res.sharpe - baseline_sharpe;
                    (*t, *intv, res.clone(), delta)
                })
            })
            .collect();
        winners.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());

        for (typ, intv, res, delta) in &winners {
            let verdict = if res.sharpe > baseline_sharpe && res.sharpe > 0.0 { "CANDIDATE" } else { "REJECTED" };
            eprintln!("  {} I={}: DeltaSharpe {:+.3} ({} trades) -- {}", typ, intv, delta, res.trades, verdict);
        }

        md_lines.push("\n## VERDICT\n".to_string());
        if let Some((best_type, best_intv, best_res, best_delta)) = winners.first() {
            if *best_delta > 0.0 && best_res.sharpe > 0.0 {
                md_lines.push(format!("**WINNER: {} interval={}** -- DeltaSharpe {:+.3} vs no-rebalancing baseline.\n",
                    best_type, best_intv, best_delta));
                md_lines.push(format!("Avg Sharpe {:+.3}, Avg Return {:+.1}%, {} trades.\n",
                    best_res.sharpe, best_res.ret, best_res.trades));
            } else {
                md_lines.push("**REJECTED.** No rebalancing type improved Sharpe.\n".to_string());
            }
        } else {
            md_lines.push("**REJECTED.** No rebalancing type improved Sharpe.\n".to_string());
        }
    }

    std::fs::write(CSV_OUT, csv_lines.join("\n"))?;
    std::fs::write(MD_OUT, md_lines.join("\n"))?;

    eprintln!("\nCSV: {} | MD: {} | Runtime: {:.1}s", CSV_OUT, MD_OUT, t0.elapsed().as_secs_f64());
    Ok(())
}

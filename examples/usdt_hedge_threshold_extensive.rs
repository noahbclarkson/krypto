//! T58: USDT Hedge Activation Threshold — Extensive Hyperopt
//!
//! Parameter: HEDGE_PCT — BTC 21d ATR percentile threshold for USDT hedge overlay.
//! When BTC 21d ATR > HEDGE_PCT-th percentile of 252-bar history → size *= 0.70.
//!
//! Hardcoded magic number: `0.75` (75th percentile) in bot.rs line 286.
//! Never systematically optimized — just assumed correct.
//!
//! Sweep: HEDGE_PCT ∈ [0..=99] step 1 (100 values) × 9 universes × 7 WF windows.
//! Also HEDGE_PCT=100 as "disabled" baseline (never fires).
//! Total: 101 × 9 × 7 = 6,363 simulations.
//!
//! Exports:
//!   - snapshots/hedge_threshold_extensive_sweep.csv  (full sweep results)
//!   - snapshots/hedge_threshold_extensive_equity.csv  (Base5 per-window equity for selected values)
//!   - snapshots/hedge_threshold_extensive_timeseries.csv  (daily equity time-series for charting)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 96;

const HEDGE_SIZE_MULT: f64 = 0.70; // fixed — only sweep the threshold

const REGIME_ATR_PERIOD: usize = 12;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 5.0;

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
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    vals[idx.saturating_sub(window - 1)..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if atr_mult > 0.0 {
            let atr = atr_at(high, low, close, atr_period, idx);
            if close[idx] < max_close + atr * atr_mult {
                return false;
            }
        }
        return true;
    }
    false
}

fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    let mut hist = Vec::with_capacity(lookback);
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = 1.0;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

#[derive(Default, Clone)]
struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
    /// Per-bar equity curve (for time-series export)
    equity_curve: Vec<f64>,
}

/// Run a single walk-forward window simulation with the given hedge_pct threshold.
/// hedge_pct = 0..100 (percentile). 100 = disabled (never fires).
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hedge_pct: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };
        
        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
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
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        // === HEDGE OVERLAY ===
                        let mut size_mult = 1.0;
                        if hedge_pct < 100.0 {
                            if let Some(b) = btc {
                                if bar >= 252 + 21 {
                                    let atr_21 = atr_at(&b.high, &b.low, &b.close, 21, bar);
                                    let mut hist = Vec::with_capacity(252);
                                    for j in (bar + 1 - 252)..=bar {
                                        let h = b.high[j];
                                        let l = b.low[j];
                                        let c0 = b.close[j.saturating_sub(1)];
                                        hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
                                    }
                                    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                    let pct_idx = ((hedge_pct / 100.0) * hist.len() as f64) as usize;
                                    let pct_idx = pct_idx.min(hist.len().saturating_sub(1));
                                    if let Some(&pct_val) = hist.get(pct_idx) {
                                        if atr_21 > pct_val {
                                            size_mult = HEDGE_SIZE_MULT;
                                        }
                                    }
                                }
                            }
                        }

                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        
                        let mut atr_buf: std::collections::VecDeque<f64> = std::collections::VecDeque::new();
                        
                        for b in entry_bar_next..=max_bar {
                            if sd.high[b] > highest_high { highest_high = sd.high[b]; }
                            let c0 = sd.close[b.saturating_sub(1)];
                            let tr = (sd.high[b] - sd.low[b]).max((sd.high[b] - c0).abs()).max((sd.low[b] - c0).abs());
                            atr_buf.push_back(tr);
                            if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.pop_front(); }
                            
                            if atr_buf.len() == TURTLE_ATR_PERIOD {
                                let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                                let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                                if sd.low[b] <= turtle_stop {
                                    exit_bar = b;
                                    break;
                                }
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let pct_ret = exit / entry - 1.0;
                            let gross_ret = pct_ret * size_mult;
                            
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                                equity_curve.push(equity); // approximate daily equity
                            }

                            if equity > peak { peak = equity; }
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }
        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }
    
    WfResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
        equity_curve,
    }
}

#[derive(Default)]
struct SweepRow {
    hedge_pct: usize,
    pass_count: usize,
    total: usize,
    avg_sharpe: f64,
    avg_return: f64,
    avg_dd: f64,
    total_trades: usize,
    positive_universes: usize,
    base5_agg_equity: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T58: USDT Hedge Threshold Extensive Hyperopt ===");
    println!("Sweep: HEDGE_PCT ∈ [0..=100] step 1 (101 values) × 9 universes × 7 WF windows");
    println!("Loading data...");
    
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols = UNIVERSES.iter().flat_map(|(_, s)| s.iter()).map(|&s| s).collect::<std::collections::HashSet<_>>();
    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    if windows == 0 { println!("No windows available!"); return Ok(()); }
    println!("Data loaded. min_len={}, windows={}", min_len, windows);

    // =========================================================================
    // SWEEP: HEDGE_PCT from 0 to 100
    // =========================================================================
    let mut sweep_results: Vec<SweepRow> = Vec::new();
    
    // Store Base5 equity curves for selected values (for charting)
    let mut selected_equity_curves: HashMap<usize, Vec<f64>> = HashMap::new();

    for hedge_pct_int in 0..=100 {
        let hedge_pct = hedge_pct_int as f64;
        let mut pass_count = 0usize;
        let mut total = 0usize;
        let mut sharpes = Vec::new();
        let mut rets = Vec::new();
        let mut dds = Vec::new();
        let mut total_trades = 0usize;
        let mut positive_universes = 0usize;

        for (u_name, u_syms) in UNIVERSES {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            let mut u_sharpes = Vec::new();
            
            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, hedge_pct);
                
                if res.trades >= MIN_TRADES && res.sharpe > 0.0 { pass_count += 1; }
                total += 1;
                sharpes.push(res.sharpe);
                u_sharpes.push(res.sharpe);
                rets.push((res.equity - 1.0) * 100.0);
                dds.push(res.dd);
                total_trades += res.trades;
            }
            
            let u_avg = u_sharpes.iter().sum::<f64>() / u_sharpes.len() as f64;
            if u_avg > 0.0 { positive_universes += 1; }
        }

        // Base5 aggregate equity (compounded across windows)
        let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
        let mut agg_equity = 1.0_f64;
        let mut base5_ts: Vec<f64> = vec![1.0]; // time-series for charting
        
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let res = run_sim(&sym_data, &base5_syms, start + TRAIN_BARS, end, hedge_pct);
            agg_equity *= res.equity;
            // Append per-bar equity curve scaled by previous aggregate
            let prev_agg = if w == 0 { 1.0 } else { base5_ts.last().copied().unwrap_or(1.0) };
            for &e in &res.equity_curve[1..] {
                base5_ts.push(prev_agg * e);
            }
        }
        
        // Store equity curves for selected values (baseline=100, default=75, and we'll pick winner later)
        if hedge_pct_int == 0 || hedge_pct_int == 25 || hedge_pct_int == 50 || 
           hedge_pct_int == 75 || hedge_pct_int == 100 || hedge_pct_int % 10 == 0 {
            selected_equity_curves.insert(hedge_pct_int, base5_ts.clone());
        }

        let n = total as f64;
        let avg_sharpe = sharpes.iter().sum::<f64>() / n;
        let avg_ret = rets.iter().sum::<f64>() / n;
        let avg_dd = dds.iter().sum::<f64>() / n;

        sweep_results.push(SweepRow {
            hedge_pct: hedge_pct_int,
            pass_count,
            total,
            avg_sharpe,
            avg_return: avg_ret,
            avg_dd,
            total_trades,
            positive_universes,
            base5_agg_equity: agg_equity,
        });

        if hedge_pct_int % 10 == 0 || hedge_pct_int == 75 {
            println!("HEDGE_PCT={:3}: {}/{} pass ({:.1}%), Sharpe {:.3}, Ret {:.1}%, DD {:.1}%, Trades {}, PosUniv {}/9, Base5 {:.2}x",
                hedge_pct_int, pass_count, total, (pass_count as f64/total as f64)*100.0,
                avg_sharpe, avg_ret, avg_dd, total_trades, positive_universes, agg_equity);
        }
    }

    // =========================================================================
    // FIND WINNER (robustness-first: highest pass rate, then highest Sharpe)
    // =========================================================================
    let best = sweep_results.iter()
        .max_by(|a, b| {
            a.pass_count.cmp(&b.pass_count)
                .then(a.avg_sharpe.partial_cmp(&b.avg_sharpe).unwrap_or(std::cmp::Ordering::Equal))
        })
        .unwrap();

    // Runner-ups: top 5 by pass rate then Sharpe (excluding winner)
    let mut sorted = sweep_results.clone();
    sorted.sort_by(|a, b| {
        b.pass_count.cmp(&a.pass_count)
            .then(b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap_or(std::cmp::Ordering::Equal))
    });

    println!("\n=== RESULTS ===");
    println!("WINNER: HEDGE_PCT={} → {}/{} pass ({:.1}%), Sharpe {:.3}, Ret {:.1}%, DD {:.1}%, Base5 {:.2}x",
        best.hedge_pct, best.pass_count, best.total, 
        (best.pass_count as f64/best.total as f64)*100.0,
        best.avg_sharpe, best.avg_return, best.avg_dd, best.base5_agg_equity);
    println!("BASELINE (75): {}/{} pass ({:.1}%), Sharpe {:.3}",
        sweep_results[75].pass_count, sweep_results[75].total,
        (sweep_results[75].pass_count as f64/sweep_results[75].total as f64)*100.0,
        sweep_results[75].avg_sharpe);
    println!("DISABLED (100): {}/{} pass ({:.1}%), Sharpe {:.3}",
        sweep_results[100].pass_count, sweep_results[100].total,
        (sweep_results[100].pass_count as f64/sweep_results[100].total as f64)*100.0,
        sweep_results[100].avg_sharpe);
    
    println!("\nTop 10:");
    for (i, row) in sorted.iter().take(10).enumerate() {
        println!("  #{}: PCT={:3} → {}/{} pass, Sharpe {:.3}, Ret {:.1}%, DD {:.1}%, Base5 {:.2}x",
            i+1, row.hedge_pct, row.pass_count, row.total, row.avg_sharpe, row.avg_return, row.avg_dd, row.base5_agg_equity);
    }

    // Ensure winner's equity curve is stored
    if !selected_equity_curves.contains_key(&best.hedge_pct) {
        let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
        let mut base5_ts: Vec<f64> = vec![1.0];
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let res = run_sim(&sym_data, &base5_syms, start + TRAIN_BARS, end, best.hedge_pct as f64);
            let prev_agg = base5_ts.last().copied().unwrap_or(1.0);
            for &e in &res.equity_curve[1..] {
                base5_ts.push(prev_agg * e);
            }
        }
        selected_equity_curves.insert(best.hedge_pct, base5_ts);
    }
    // Also ensure runner-up #2 and #3 are stored
    for row in sorted.iter().skip(1).take(2) {
        if !selected_equity_curves.contains_key(&row.hedge_pct) {
            let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
            let mut base5_ts: Vec<f64> = vec![1.0];
            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &base5_syms, start + TRAIN_BARS, end, row.hedge_pct as f64);
                let prev_agg = base5_ts.last().copied().unwrap_or(1.0);
                for &e in &res.equity_curve[1..] {
                    base5_ts.push(prev_agg * e);
                }
            }
            selected_equity_curves.insert(row.hedge_pct, base5_ts);
        }
    }

    // =========================================================================
    // EXPORT: Sweep CSV
    // =========================================================================
    {
        let mut f = File::create("snapshots/hedge_threshold_extensive_sweep.csv")?;
        writeln!(f, "hedge_pct,pass_count,total,pass_rate,avg_sharpe,avg_return,avg_dd,total_trades,positive_universes,base5_agg_equity")?;
        for row in &sweep_results {
            writeln!(f, "{},{},{},{:.1},{:.6},{:.4},{:.4},{},{},{:.6}",
                row.hedge_pct, row.pass_count, row.total,
                (row.pass_count as f64/row.total as f64)*100.0,
                row.avg_sharpe, row.avg_return, row.avg_dd,
                row.total_trades, row.positive_universes, row.base5_agg_equity)?;
        }
        println!("\nSweep CSV written to snapshots/hedge_threshold_extensive_sweep.csv");
    }

    // =========================================================================
    // EXPORT: Time-series equity curves for charting
    // =========================================================================
    {
        // Find the max length across all selected curves
        let max_len = selected_equity_curves.values().map(|v| v.len()).max().unwrap_or(0);
        let mut keys: Vec<usize> = selected_equity_curves.keys().copied().collect();
        keys.sort();
        
        let mut f = File::create("snapshots/hedge_threshold_extensive_timeseries.csv")?;
        // Header
        let header: Vec<String> = std::iter::once("bar".to_string())
            .chain(keys.iter().map(|k| format!("pct_{}", k)))
            .collect();
        writeln!(f, "{}", header.join(","))?;
        
        for i in 0..max_len {
            let mut row_vals: Vec<String> = vec![i.to_string()];
            for &k in &keys {
                let curve = selected_equity_curves.get(&k).unwrap();
                if i < curve.len() {
                    row_vals.push(format!("{:.6}", curve[i]));
                } else {
                    // Pad with last value
                    row_vals.push(format!("{:.6}", curve.last().unwrap_or(&1.0)));
                }
            }
            writeln!(f, "{}", row_vals.join(","))?;
        }
        println!("Time-series CSV written to snapshots/hedge_threshold_extensive_timeseries.csv");
    }

    // =========================================================================
    // EXPORT: Summary markdown
    // =========================================================================
    {
        let mut f = File::create("snapshots/hedge_threshold_extensive_summary.md")?;
        writeln!(f, "# T58: USDT Hedge Threshold Extensive Hyperopt")?;
        writeln!(f, "")?;
        writeln!(f, "**Parameter:** HEDGE_PCT — BTC 21d ATR percentile threshold for position size reduction")?;
        writeln!(f, "**Range:** 0..=100 step 1 (101 values) × 9 universes × {} WF windows = {} simulations", windows, 101 * 9 * windows)?;
        writeln!(f, "**Size mult when active:** {}", HEDGE_SIZE_MULT)?;
        writeln!(f, "")?;
        writeln!(f, "## Winner")?;
        writeln!(f, "HEDGE_PCT={}: {}/{} pass ({:.1}%), Sharpe {:.3}, Ret {:.1}%, DD {:.1}%, Base5 {:.2}x",
            best.hedge_pct, best.pass_count, best.total,
            (best.pass_count as f64/best.total as f64)*100.0,
            best.avg_sharpe, best.avg_return, best.avg_dd, best.base5_agg_equity)?;
        writeln!(f, "")?;
        writeln!(f, "## Baseline (75th percentile — current production)")?;
        let bl = &sweep_results[75];
        writeln!(f, "HEDGE_PCT=75: {}/{} pass ({:.1}%), Sharpe {:.3}, Ret {:.1}%, DD {:.1}%, Base5 {:.2}x",
            bl.pass_count, bl.total,
            (bl.pass_count as f64/bl.total as f64)*100.0,
            bl.avg_sharpe, bl.avg_return, bl.avg_dd, bl.base5_agg_equity)?;
        writeln!(f, "")?;
        writeln!(f, "## Top 10")?;
        writeln!(f, "| Rank | HEDGE_PCT | Pass | Pass% | Sharpe | Return% | DD% | Base5 Eq |")?;
        writeln!(f, "|------|-----------|------|-------|--------|---------|-----|----------|")?;
        for (i, row) in sorted.iter().take(10).enumerate() {
            writeln!(f, "| {} | {} | {}/{} | {:.1}% | {:.3} | {:.1}% | {:.1}% | {:.2}x |",
                i+1, row.hedge_pct, row.pass_count, row.total,
                (row.pass_count as f64/row.total as f64)*100.0,
                row.avg_sharpe, row.avg_return, row.avg_dd, row.base5_agg_equity)?;
        }
        println!("Summary written to snapshots/hedge_threshold_extensive_summary.md");
    }

    println!("\n=== DONE ===");
    println!("Next: python3 charts/plot_hedge_threshold.py to generate comparison_chart.png");

    Ok(())
}

impl Clone for SweepRow {
    fn clone(&self) -> Self {
        SweepRow {
            hedge_pct: self.hedge_pct,
            pass_count: self.pass_count,
            total: self.total,
            avg_sharpe: self.avg_sharpe,
            avg_return: self.avg_return,
            avg_dd: self.avg_dd,
            total_trades: self.total_trades,
            positive_universes: self.positive_universes,
            base5_agg_equity: self.base5_agg_equity,
        }
    }
}

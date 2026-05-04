//! T57: Funding Rate Regime Filter — Walk-Forward Validation
//!
//! PURPOSE: Test whether BTC aggregate funding rate works as a REGIME FILTER
//! on Turtle entries. This is NOT trading on funding (GRAVEYARD'd) — it's using
//! funding as a market condition gate.
//!
//! Hypothesis: When BTC funding rate is elevated (> threshold), the market is
//! overleveraged long. Liquidation cascades become more likely. Turtle breakout
//! entries during these periods should be skipped to reduce crash exposure.
//!
//! Mechanism: compute rolling N-day average of BTC funding rate. If avg > T,
//! skip new Turtle entries for ALL symbols (BTC-driven regime gate).
//!
//! Sweep: threshold T across range, fixed rolling window.
//! Compare to T=∞ baseline (no funding gate, standard Turtle).
//!
//! Uses live_compatible_wf.rs strategy (Turtle-only exit, ATR_RANK=5 gate).

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

const REGIME_ATR_PERIOD: usize = 12;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 5.0;

// Funding regime filter params
const FUNDING_ROLLING_DAYS: usize = 3; // 3-day rolling average of 8h funding rates

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

const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT",
    "LTCUSDT","BNBUSDT","EOSUSDT","BCHUSDT",
];

struct SymData {
    closes: Vec<f64>,
    highs: Vec<f64>,
    lows: Vec<f64>,
    volumes: Vec<f64>,
    timestamps: Vec<i64>,
}

#[derive(Default)]
struct WFResult {
    universe: String,
    window: usize,
    sharpe: f64,
    ret_pct: f64,
    dd_pct: f64,
    trades: usize,
    pass: bool,
    entries_blocked: usize,
}

fn load_btc_funding_daily(btc_timestamps: &[i64]) -> Vec<f64> {
    // Load BTC funding from parquet cache
    let path = "examples/funding_cache/btcusdt_funding.parquet";
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("WARNING: Could not load funding data from {path}: {e}");
            return vec![0.0; btc_timestamps.len()];
        }
    };
    
    let df = match polars::io::parquet::ParquetReader::new(file).finish() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("WARNING: Could not parse funding parquet: {e}");
            return vec![0.0; btc_timestamps.len()];
        }
    };
    
    // Extract funding times and rates
    let fund_times_col = df.column("time").unwrap().cast(&DataType::Int64).unwrap();
    let fund_times: Vec<i64> = fund_times_col.i64().unwrap().into_iter()
        .map(|v| v.unwrap_or(0)).collect();
    let fund_rates: Vec<f64> = df.column("funding_rate").unwrap().f64().unwrap()
        .into_iter().map(|v| v.unwrap_or(0.0)).collect();
    
    // For each daily bar timestamp, compute rolling average of recent funding rates
    // Rolling window: FUNDING_ROLLING_DAYS days = FUNDING_ROLLING_DAYS * 3 funding periods (8h each)
    let _funding_periods = FUNDING_ROLLING_DAYS * 3;
    
    let mut daily_funding = vec![0.0f64; btc_timestamps.len()];
    
    for (i, &bar_ts) in btc_timestamps.iter().enumerate() {
        // Find all funding rates in the rolling window before this bar
        // bar_ts is in milliseconds (datetime)
        let window_start = bar_ts - (FUNDING_ROLLING_DAYS as i64) * 24 * 3600 * 1000;
        
        let mut sum = 0.0;
        let mut count = 0;
        
        for (j, &ft) in fund_times.iter().enumerate() {
            if ft > window_start && ft <= bar_ts {
                sum += fund_rates[j];
                count += 1;
            }
        }
        
        if count > 0 {
            daily_funding[i] = sum / count as f64;
        }
    }
    
    daily_funding
}

fn vol_rank_pct(closes: &[f64], idx: usize, atr_period: usize, lookback: usize) -> f64 {
    if idx < atr_period + lookback { return 50.0; }
    
    let current_atr = {
        let mut atr = 0.0;
        for k in (idx - atr_period + 1)..=idx {
            atr += (closes[k] - closes[k-1]).abs();
        }
        atr / atr_period as f64
    };
    let current_atr_pct = current_atr / closes[idx] * 100.0;
    
    let mut count_below = 0usize;
    let total = lookback;
    for h in 1..=lookback {
        let hi = idx - h;
        if hi < atr_period { break; }
        let hist_atr = {
            let mut a = 0.0;
            for k in (hi - atr_period + 1)..=hi {
                a += (closes[k] - closes[k-1]).abs();
            }
            a / atr_period as f64
        };
        let hist_pct = hist_atr / closes[hi] * 100.0;
        if current_atr_pct >= hist_pct { count_below += 1; }
    }
    count_below as f64 / total as f64 * 100.0
}

fn run_window(
    sym_data: &HashMap<String, SymData>,
    symbols: &[&str],
    btc_funding: &[f64],
    test_start: usize,
    test_end: usize,
    funding_threshold: f64,
) -> WFResult {
    let btc = &sym_data["BTCUSDT"];
    let n = btc.closes.len();
    
    // Compute dollar volume lookback for ranking
    let mut sym_scores: Vec<(&str, f64)> = Vec::new();
    for &sym in symbols {
        if sym == "BTCUSDT" { continue; }
        if let Some(sd) = sym_data.get(sym) {
            if test_start >= sd.closes.len() { continue; }
            let start = test_start.saturating_sub(VOL_LOOKBACK);
            let end = test_start.min(sd.closes.len());
            if start >= end { continue; }
            let avg_dv: f64 = (start..end).map(|i| sd.closes[i] * sd.volumes[i]).sum::<f64>() / (end - start) as f64;
            sym_scores.push((sym, avg_dv));
        }
    }
    sym_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let tradeable: Vec<&str> = sym_scores.iter().take(5).map(|s| s.0).collect();
    
    // Track positions
    struct Position {
        sym: String,
        entry_px: f64,
        entry_bar: usize,
        highest_high: f64,
        atr_buf: Vec<f64>,
    }
    
    let mut positions: Vec<Position> = Vec::new();
    let mut equity = 1.0f64;
    let mut peak = 1.0f64;
    let mut max_dd = 0.0f64;
    let mut daily_rets: Vec<f64> = Vec::new();
    let mut total_trades = 0usize;
    let mut entries_blocked = 0usize;
    
    let actual_end = test_end.min(n);
    
    for bar in test_start..actual_end {
        // --- EXIT existing positions ---
        let mut to_remove = Vec::new();
        for (pi, pos) in positions.iter_mut().enumerate() {
            let sd = &sym_data[pos.sym.as_str()];
            if bar >= sd.closes.len() { to_remove.push(pi); continue; }
            
            let hold_len = bar - pos.entry_bar;
            
            // Update highest high
            if sd.highs[bar] > pos.highest_high {
                pos.highest_high = sd.highs[bar];
            }
            
            // ATR buffer update
            if bar > 0 {
                let tr = (sd.highs[bar] - sd.lows[bar])
                    .max((sd.highs[bar] - sd.closes[bar-1]).abs())
                    .max((sd.lows[bar] - sd.closes[bar-1]).abs());
                pos.atr_buf.push(tr);
                if pos.atr_buf.len() > TURTLE_ATR_PERIOD {
                    pos.atr_buf.remove(0);
                }
            }
            
            // Check HOLD_MAX first (enforced before ATR warmup)
            if hold_len >= HOLD_MAX {
                let exit_px = sd.closes[bar] * (1.0 - TAKER_FEE);
                let entry_cost = pos.entry_px * (1.0 + TAKER_FEE);
                let pnl = (exit_px / entry_cost) - 1.0;
                let pos_weight = 1.0 / POSITION_CAP as f64;
                equity *= 1.0 + pnl * pos_weight;
                daily_rets.push(pnl * pos_weight);
                to_remove.push(pi);
                continue;
            }
            
            // ATR trailing stop (Turtle-only)
            if pos.atr_buf.len() >= TURTLE_ATR_PERIOD {
                let atr: f64 = pos.atr_buf.iter().sum::<f64>() / pos.atr_buf.len() as f64;
                let stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                if sd.closes[bar] <= stop {
                    let exit_px = sd.closes[bar] * (1.0 - TAKER_FEE);
                    let entry_cost = pos.entry_px * (1.0 + TAKER_FEE);
                    let pnl = (exit_px / entry_cost) - 1.0;
                    let pos_weight = 1.0 / POSITION_CAP as f64;
                    equity *= 1.0 + pnl * pos_weight;
                    daily_rets.push(pnl * pos_weight);
                    to_remove.push(pi);
                    continue;
                }
            }
        }
        to_remove.sort_unstable();
        to_remove.dedup();
        for &pi in to_remove.iter().rev() {
            positions.remove(pi);
            total_trades += 1;
        }
        
        // --- ENTRY ---
        if positions.len() < POSITION_CAP && bar >= TURTLE_ENTRY + 1 {
            // ATR rank gate (BTC regime filter)
            let btc_rank = vol_rank_pct(&btc.closes, bar, REGIME_ATR_PERIOD, REGIME_LOOKBACK);
            if btc_rank < ATR_RANK_T {
                // ATR rank too low — skip
            } else {
                // FUNDING REGIME GATE — the new filter being tested
                let funding_blocked = if funding_threshold < 999.0 {
                    bar < btc_funding.len() && btc_funding[bar] > funding_threshold
                } else {
                    false // no gate
                };
                
                if funding_blocked {
                    entries_blocked += 1;
                } else {
                    // Scan tradeable symbols for breakout
                    for &sym in &tradeable {
                        if positions.len() >= POSITION_CAP { break; }
                        if positions.iter().any(|p| p.sym == sym) { continue; }
                        
                        let sd = match sym_data.get(sym) {
                            Some(d) => d,
                            None => continue,
                        };
                        if bar >= sd.closes.len() { continue; }
                        
                        // Turtle breakout: close > max(close) over EP bars
                        let lookback_start = bar.saturating_sub(TURTLE_ENTRY);
                        let max_close = (lookback_start..bar).map(|i| sd.closes[i]).fold(f64::NEG_INFINITY, f64::max);
                        
                        if sd.closes[bar] > max_close {
                            // ATR entry filter (EM=0.00 means no filter)
                            let entry_ok = if ATR_ENTRY_MULT > 0.0 {
                                let atr_sum: f64 = (bar.saturating_sub(TURTLE_ATR_PERIOD)..bar)
                                    .map(|i| {
                                        if i == 0 { return sd.highs[i] - sd.lows[i]; }
                                        let tr = (sd.highs[i] - sd.lows[i])
                                            .max((sd.highs[i] - sd.closes[i-1]).abs())
                                            .max((sd.lows[i] - sd.closes[i-1]).abs());
                                        tr
                                    }).sum();
                                let atr = atr_sum / TURTLE_ATR_PERIOD as f64;
                                (sd.closes[bar] - max_close) >= ATR_ENTRY_MULT * atr
                            } else {
                                true
                            };
                            
                            if entry_ok {
                                // Size overlay (high-vol reduction)
                                // Not modeled in position weight for simplicity — same as live_compatible_wf
                                
                                // Seed ATR buffer with entry data
                                let mut atr_buf = Vec::with_capacity(TURTLE_ATR_PERIOD);
                                let seed_start = bar.saturating_sub(TURTLE_ATR_PERIOD);
                                for k in seed_start..=bar {
                                    if k == 0 { atr_buf.push(sd.highs[k] - sd.lows[k]); continue; }
                                    if k >= sd.closes.len() { break; }
                                    let tr = (sd.highs[k] - sd.lows[k])
                                        .max((sd.highs[k] - sd.closes[k-1]).abs())
                                        .max((sd.lows[k] - sd.closes[k-1]).abs());
                                    atr_buf.push(tr);
                                }
                                
                                positions.push(Position {
                                    sym: sym.to_string(),
                                    entry_px: sd.closes[bar],
                                    entry_bar: bar,
                                    highest_high: sd.highs[bar],
                                    atr_buf,
                                });
                            }
                        }
                    }
                }
            }
        }
        
        // Track equity
        if equity > peak { peak = equity; }
        let dd = (peak - equity) / peak * 100.0;
        if dd > max_dd { max_dd = dd; }
    }
    
    // Close remaining positions at end
    for pos in &positions {
        let sd = &sym_data[pos.sym.as_str()];
        let last = actual_end.min(sd.closes.len()) - 1;
        let exit_px = sd.closes[last] * (1.0 - TAKER_FEE);
        let entry_cost = pos.entry_px * (1.0 + TAKER_FEE);
        let pnl = (exit_px / entry_cost) - 1.0;
        let pos_weight = 1.0 / POSITION_CAP as f64;
        daily_rets.push(pnl * pos_weight);
        total_trades += 1;
    }
    
    let ret_pct = (equity - 1.0) * 100.0;
    let sharpe = if daily_rets.len() >= 2 {
        let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
        let var = daily_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (daily_rets.len() - 1) as f64;
        let std = var.sqrt();
        if std > 1e-10 { mean / std * (252f64).sqrt() } else { 0.0 }
    } else {
        0.0
    };
    
    let pass = total_trades >= MIN_TRADES && sharpe > 0.0;
    
    WFResult {
        universe: String::new(),
        window: 0,
        sharpe,
        ret_pct,
        dd_pct: max_dd,
        trades: total_trades,
        pass,
        entries_blocked,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("T57: Funding Rate Regime Filter Walk-Forward");
    println!("============================================\n");
    
    // Load OHLCV data
    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    
    for &sym in LOAD_SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let closes: Vec<f64> = df.column("close")?.f64()?.into_iter().map(|v| v.unwrap_or(0.0)).collect();
        let highs: Vec<f64> = df.column("high")?.f64()?.into_iter().map(|v| v.unwrap_or(0.0)).collect();
        let lows: Vec<f64> = df.column("low")?.f64()?.into_iter().map(|v| v.unwrap_or(0.0)).collect();
        let volumes: Vec<f64> = df.column("volume")?.f64()?.into_iter().map(|v| v.unwrap_or(0.0)).collect();
        let timestamps: Vec<i64> = df.column("time")?.cast(&DataType::Int64)?.i64()?
            .into_iter().map(|v| v.unwrap_or(0)).collect();
        
        sym_data.insert(sym.to_string(), SymData { closes, highs, lows, volumes, timestamps });
    }
    
    let btc = &sym_data["BTCUSDT"];
    let n = btc.closes.len();
    println!("Loaded {n} daily bars for {} symbols", sym_data.len());
    
    // Load BTC funding data aligned to daily bars
    let btc_funding = load_btc_funding_daily(&btc.timestamps);
    
    // Show funding stats
    let nonzero: Vec<&f64> = btc_funding.iter().filter(|&&f| f != 0.0).collect();
    if !nonzero.is_empty() {
        let avg = nonzero.iter().copied().sum::<f64>() / nonzero.len() as f64;
        println!("BTC funding: {}/{n} bars with data, avg {:.6}% per 8h", nonzero.len(), avg * 100.0);
        
        let mut sorted: Vec<f64> = nonzero.iter().copied().cloned().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = sorted[sorted.len() / 2];
        let p90 = sorted[(sorted.len() as f64 * 0.90) as usize];
        let p95 = sorted[(sorted.len() as f64 * 0.95) as usize];
        let p99 = sorted[(sorted.len() as f64 * 0.99) as usize];
        println!("  Percentiles: p50={:.6}%, p90={:.6}%, p95={:.6}%, p99={:.6}%", 
            p50*100.0, p90*100.0, p95*100.0, p99*100.0);
    }
    
    // Sweep funding thresholds
    // Using BTC funding rate percentiles as thresholds
    // Dense sweep: 51 thresholds from 0 to 0.0025 step 0.00005
    let mut thresholds: Vec<(String, f64)> = vec![
        ("no_gate".into(), 999.0),       // baseline — no funding gate
    ];
    for i in 0..=50 {
        let t = i as f64 * 0.00005;
        thresholds.push((format!("t_{:.5}", t), t));
    }
    
    println!("\nSweeping {} thresholds × {} universes × walk-forward windows...\n",
        thresholds.len(), UNIVERSES.len() as usize);
    
    // Walk-forward windows
    let warmup = TRAIN_BARS + TURTLE_ATR_PERIOD + 50;
    let mut windows: Vec<(usize, usize)> = Vec::new();
    let mut start = warmup;
    while start + TEST_BARS <= n {
        windows.push((start, start + TEST_BARS));
        start += TEST_BARS;
    }
    let n_windows = windows.len();
    
    println!("Walk-forward: {n_windows} windows of {TEST_BARS} bars each\n");
    
    // Results storage
    struct ThresholdResult {
        label: String,
        threshold: f64,
        total_pass: usize,
        total_windows: usize,
        avg_sharpe: f64,
        avg_ret: f64,
        avg_dd: f64,
        total_trades: usize,
        total_blocked: usize,
        positive_universes: usize,
        base5_equities: Vec<f64>,
    }
    
    let mut results: Vec<ThresholdResult> = Vec::new();
    
    for (label, threshold) in &thresholds {
        let mut total_pass = 0usize;
        let mut total_windows = 0usize;
        let mut sharpes = Vec::new();
        let mut rets = Vec::new();
        let mut dds = Vec::new();
        let mut total_trades = 0usize;
        let mut total_blocked = 0usize;
        let mut positive_universes = 0usize;
        let mut base5_equities = Vec::new();
        
        for &(uni_name, uni_syms) in UNIVERSES {
            let mut uni_sharpes = Vec::new();
            let mut uni_equity = 1.0f64;
            
            for &(ws, we) in &windows {
                let mut r = run_window(&sym_data, uni_syms, &btc_funding, ws, we, *threshold);
                r.universe = uni_name.to_string();
                
                if r.pass { total_pass += 1; }
                total_windows += 1;
                sharpes.push(r.sharpe);
                rets.push(r.ret_pct);
                dds.push(r.dd_pct);
                total_trades += r.trades;
                total_blocked += r.entries_blocked;
                uni_sharpes.push(r.sharpe);
                
                if uni_name == "Base5" {
                    uni_equity *= 1.0 + r.ret_pct / 100.0;
                    base5_equities.push(uni_equity);
                }
            }
            
            let uni_avg = uni_sharpes.iter().sum::<f64>() / uni_sharpes.len().max(1) as f64;
            if uni_avg > 0.0 { positive_universes += 1; }
        }
        
        let avg_sharpe = sharpes.iter().sum::<f64>() / sharpes.len().max(1) as f64;
        let avg_ret = rets.iter().sum::<f64>() / rets.len().max(1) as f64;
        let avg_dd = dds.iter().sum::<f64>() / dds.len().max(1) as f64;
        
        results.push(ThresholdResult {
            label: label.clone(),
            threshold: *threshold,
            total_pass,
            total_windows,
            avg_sharpe,
            avg_ret,
            avg_dd,
            total_trades,
            total_blocked,
            positive_universes,
            base5_equities,
        });
    }
    
    // Print summary table
    println!("\n{:-<120}", "");
    println!("{:<12} {:>8} {:>10} {:>8} {:>10} {:>8} {:>8} {:>10} {:>8}",
        "Threshold", "Pass", "Pass%", "Sharpe", "Return%", "DD%", "Trades", "Blocked", "PosUni");
    println!("{:-<120}", "");
    
    for r in &results {
        println!("{:<12} {:>4}/{:<4} {:>8.1}% {:>8.3} {:>9.1}% {:>7.1}% {:>7} {:>9} {:>5}/9",
            r.label,
            r.total_pass, r.total_windows,
            r.total_pass as f64 / r.total_windows as f64 * 100.0,
            r.avg_sharpe,
            r.avg_ret,
            r.avg_dd,
            r.total_trades,
            r.total_blocked,
            r.positive_universes,
        );
    }
    
    // Base5 aggregate equity
    println!("\nBase5 Aggregate Equity (compounded over {n_windows} windows):");
    for r in &results {
        let final_eq = r.base5_equities.last().copied().unwrap_or(1.0);
        println!("  {:<12}: {:.2}x", r.label, final_eq);
    }
    
    // Compare to baseline
    let baseline = &results[0]; // no_gate
    println!("\n--- Comparison to Baseline (no funding gate) ---");
    for r in results.iter().skip(1) {
        let pass_delta = r.total_pass as i64 - baseline.total_pass as i64;
        let sharpe_delta = r.avg_sharpe - baseline.avg_sharpe;
        let ret_delta = r.avg_ret - baseline.avg_ret;
        let dd_delta = r.avg_dd - baseline.avg_dd;
        
        println!("{:<12}: pass {:+3}, Sharpe {:+.3}, ret {:+.1}%, DD {:+.1}%, blocked {}",
            r.label, pass_delta, sharpe_delta, ret_delta, dd_delta, r.total_blocked);
    }
    
    // Write CSV
    let csv_path = "snapshots/funding_regime_sweep.csv";
    let mut f = File::create(csv_path)?;
    writeln!(f, "threshold_label,threshold_value,pass,total_windows,pass_pct,avg_sharpe,avg_ret_pct,avg_dd_pct,total_trades,total_blocked,positive_universes")?;
    for r in &results {
        writeln!(f, "{},{:.6},{},{},{:.1},{:.4},{:.2},{:.2},{},{},{}",
            r.label, r.threshold, r.total_pass, r.total_windows,
            r.total_pass as f64 / r.total_windows as f64 * 100.0,
            r.avg_sharpe, r.avg_ret, r.avg_dd,
            r.total_trades, r.total_blocked, r.positive_universes)?;
    }
    println!("\nCSV written to {csv_path}");
    
    // Write markdown summary
    let md_path = "snapshots/funding_regime_walkforward.md";
    let mut f = File::create(md_path)?;
    writeln!(f, "# T57: Funding Rate Regime Filter Walk-Forward\n")?;
    writeln!(f, "**Date:** 2026-05-04")?;
    writeln!(f, "**Sweep:** BTC {FUNDING_ROLLING_DAYS}-day rolling avg funding rate threshold")?;
    writeln!(f, "**Harness:** Turtle-only exit, ATR_RANK=5 gate, {} universes × {n_windows} WF windows\n", UNIVERSES.len())?;
    
    writeln!(f, "| Threshold | Pass | Pass% | Sharpe | Return% | DD% | Trades | Blocked | PosUni |")?;
    writeln!(f, "|-----------|------|-------|--------|---------|-----|--------|---------|--------|")?;
    for r in &results {
        writeln!(f, "| {} | {}/{} | {:.1}% | {:.3} | {:.1}% | {:.1}% | {} | {} | {}/9 |",
            r.label, r.total_pass, r.total_windows,
            r.total_pass as f64 / r.total_windows as f64 * 100.0,
            r.avg_sharpe, r.avg_ret, r.avg_dd,
            r.total_trades, r.total_blocked, r.positive_universes)?;
    }
    
    writeln!(f, "\n## Base5 Aggregate Equity")?;
    for r in &results {
        let final_eq = r.base5_equities.last().copied().unwrap_or(1.0);
        writeln!(f, "- {}: {:.2}x", r.label, final_eq)?;
    }
    
    println!("Markdown written to {md_path}");
    
    Ok(())
}

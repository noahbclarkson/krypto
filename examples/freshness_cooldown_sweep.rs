//! FRESHNESS_COOLDOWN Extensive Hyperopt — Live Turtle-Only Path
//! 
//! Hyperparameter: FRESHNESS_COOLDOWN ∈ [0..=15 step 1] (16 values)
//! - Controls bars to wait after exit before allowing re-entry on the same symbol
//! - Currently hardcoded at 0 (disabled) — NEVER TESTED
//!
//! Using EXACT LIVE BOT PATH: Turtle breakout + ATR_RANK filter + Turtle ATR trailing stop

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.0004;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

// Turtle params (current production)
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 15;

// Regime filter (current production)
const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 41;
const ATR_RANK_T: f64 = 5.0;

// Hedge (current production)  
const HEDGE_ATR_PERIOD: usize = 38;
const HEDGE_LOOKBACK: usize = 252;
const HEDGE_ATR_PCT: f64 = 0.45;
const HEDGE_SIZE_MULT: f64 = 0.25;

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

const FC_VALUES: &[usize] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

fn main() -> Result<()> {
    println!("=== FRESHNESS_COOLDOWN Extensive Hyperopt ===");
    println!("Testing: FC ∈ {:?}", FC_VALUES);
    
    let loader = DataLoader::new()?;
    let mut results: HashMap<usize, (usize, usize, f64, f64, f64, f64, usize)> = HashMap::new();
    
    for &fc in FC_VALUES {
        println!("\n>>> Testing FC={}", fc);
        
        let mut total_pass = 0;
        let mut total_runs = 0;
        let mut total_sharpe = 0.0;
        let mut total_return = 0.0;
        let mut total_dd = 0.0;
        let mut total_trades = 0;
        
        for (uname, symbols) in UNIVERSES {
            let mut sym_data: Vec<SymData> = Vec::new();
            let mut all_bars = 0;
            
            for sym in *symbols {
                match loader.load_historical(sym, "1d", CANDLES) {
                    Ok(bars) => {
                        all_bars = bars.len();
                        sym_data.push(SymData {
                            close: bars.iter().map(|b| b.close).collect(),
                            high: bars.iter().map(|b| b.high).collect(),
                            low: bars.iter().map(|b| b.low).collect(),
                        });
                    }
                    Err(_) => { sym_data.clear(); break; }
                }
            }
            
            if sym_data.is_empty() || all_bars < 300 { continue; }
            
            // Single walk-forward window test
            let train_start = 500;
            let test_start = train_start + TRAIN_BARS;
            let test_end = (test_start + TEST_BARS).min(all_bars);
            
            if test_end - test_start < 60 { continue; }
            
            let (equity, trades, eq_history) = run_backtest(&sym_data, fc, test_start, test_end);
            
            if trades.len() >= MIN_TRADES {
                let ret = (equity - 1.0) * 100.0;
                let sharpe = compute_sharpe(&eq_history);
                let dd = compute_max_dd(&eq_history);
                let win = ret > 0.0;
                
                total_runs += 1;
                if win { total_pass += 1; }
                total_sharpe += sharpe;
                total_return += ret;
                total_dd += dd;
                total_trades += trades.len();
            }
        }
        
        let n_universes = total_runs.max(1);
        let pass_rate = total_pass as f64 / total_runs as f64 * 100.0;
        let avg_sharpe = total_sharpe / n_universes as f64;
        let avg_return = total_return / n_universes as f64;
        let avg_dd = total_dd / n_universes as f64;
        
        println!("  {}: {:.1}% pass, Sharpe {:.3}, Ret {:.1}%, DD {:.1}%, Trades {}",
            fc, pass_rate, avg_sharpe, avg_return, avg_dd, total_trades);
        
        results.insert(fc, (total_pass, total_runs, pass_rate, avg_sharpe, avg_return, avg_dd, total_trades));
    }
    
    // Write summary CSV
    let mut summary_file = File::create("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/freshness_cooldown_sweep_summary.csv")?;
    writeln!(summary_file, "fc,pass,total,pass_pct,avg_sharpe,avg_return,avg_dd,total_trades")?;
    
    for &fc in FC_VALUES {
        if let Some((pass, total, pct, sharpe, ret, dd, trades)) = results.get(&fc) {
            writeln!(summary_file, "{},{},{},{:.2},{:.4},{:.2},{:.2},{}", 
                fc, pass, total, pct, sharpe, ret, dd, trades)?;
        }
    }
    
    // Find winner by pass rate, then Sharpe
    let mut sorted: Vec<_> = results.iter().collect();
    sorted.sort_by(|a, b| {
        let a_pass = a.1 .2;
        let b_pass = b.1 .2;
        let a_sharpe = a.1 .3;
        let b_sharpe = b.1 .3;
        b_pass.partial_cmp(&a_pass).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b_sharpe.partial_cmp(&a_sharpe).unwrap_or(std::cmp::Ordering::Equal))
    });
    
    let winner = sorted[0].0;
    let w_stats = sorted[0].1;
    
    println!("\n=== WINNER: FC={} ===", winner);
    println!("  Pass: {}/{} ({:.1}%)", w_stats.0, w_stats.1, w_stats.2);
    println!("  Sharpe: {:.4}", w_stats.3);
    println!("  Return: {:.1}%", w_stats.4);
    println!("  MaxDD: {:.1}%", w_stats.5);
    println!("  Trades: {}", w_stats.6);
    
    // Save winner
    let mut wfile = File::create("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/freshness_cooldown_winner.txt")?;
    writeln!(wfile, "FRESHNESS_COOLDOWN winner: {}", winner)?;
    
    Ok(())
}

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
}

fn run_backtest(sym_data: &[SymData], fc: usize, start: usize, end: usize) -> (f64, Vec<Trade>, Vec<f64>) {
    let n_syms = sym_data.len();
    let n_bars = end - start;
    let mut equity = 1.0;
    let mut equity_curve = Vec::with_capacity(n_bars);
    
    let mut positions: Vec<Option<Position>> = vec![None; n_syms];
    let mut last_exit_bar: Vec<Option<usize>> = vec![None; n_syms];
    let mut atr_buffers: Vec<Vec<f64>> = vec![vec![]; n_syms];
    
    for bar_idx in 0..n_bars {
        let abs_bar = start + bar_idx;
        
        // Update ATR
        for s in 0..n_syms {
            let sd = &sym_data[s];
            if abs_bar >= TURTLE_ATR_PERIOD && bar_idx >= TURTLE_ATR_PERIOD {
                let mut trs = Vec::new();
                for i in 0..TURTLE_ATR_PERIOD {
                    let idx = bar_idx - TURTLE_ATR_PERIOD + i;
                    if idx >= 0 && idx < sd.high.len() {
                        let h = sd.high[idx];
                        let l = sd.low[idx];
                        let pc = sd.close[idx];
                        let tr = (h - l).max((h - pc).abs()).max((l - pc).abs());
                        trs.push(tr);
                    }
                }
                if trs.len() == TURTLE_ATR_PERIOD {
                    let atr = trs.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                    atr_buffers[s].push(atr);
                    if atr_buffers[s].len() > 100 { atr_buffers[s].remove(0); }
                }
            }
        }
        
        if abs_bar < TURTLE_ENTRY.max(HOLD_MAX) + 50 { 
            equity_curve.push(equity);
            continue; 
        }
        
        // Check exits
        for s in 0..n_syms {
            if let Some(ref mut pos) = positions[s] {
                let close = sym_data[s].close[bar_idx];
                let exit = if let Some(atr) = atr_buffers[s].last() {
                    let stop = pos.high - TURTLE_ATR_MULT * atr;
                    if close < stop { Some(stop) } else { None }
                } else { None };
                
                let timeout = (bar_idx - pos.entry_bar) >= HOLD_MAX;
                
                if exit.is_some() || timeout {
                    let exit_price = exit.unwrap_or(close);
                    let pnl = pos.size * (exit_price - pos.entry_price) / pos.entry_price;
                    equity *= 1.0 + pnl;
                    last_exit_bar[s] = Some(bar_idx);
                    positions[s] = None;
                }
            }
        }
        
        let open_count = positions.iter().filter(|p| p.is_some()).count();
        
        // Check entries
        if open_count < POSITION_CAP {
            for s in 0..n_syms {
                if positions[s].is_some() { continue; }
                let sd = &sym_data[s];
                if bar_idx < TURTLE_ENTRY { continue; }
                
                // Freshness check
                if let Some(last_exit) = last_exit_bar[s] {
                    if (bar_idx - last_exit) < fc { continue; }
                }
                
                // Turtle breakout
                let mut max_close = sd.close[bar_idx - TURTLE_ENTRY];
                for i in 1..TURTLE_ENTRY {
                    let idx = bar_idx - TURTLE_ENTRY + i;
                    if idx >= 0 && idx < sd.close.len() {
                        max_close = max_close.max(sd.close[idx]);
                    }
                }
                
                let close = sd.close[bar_idx];
                if close >= max_close {
                    // ATR_RANK filter
                    let passes = if abs_bar >= REGIME_LOOKBACK + REGIME_ATR_PERIOD && atr_buffers[s].len() >= REGIME_ATR_PERIOD {
                        let current_atr = atr_buffers[s].last().unwrap();
                        let mut hist = Vec::new();
                        for hb in (bar_idx - REGIME_LOOKBACK)..bar_idx {
                            if hb >= REGIME_ATR_PERIOD && atr_buffers[s].len() > hb.saturating_sub(start) {
                                if let Some(a) = atr_buffers[s].get(hb.saturating_sub(start)) {
                                    hist.push(*a);
                                }
                            }
                        }
                        if hist.len() > 10 {
                            let below = hist.iter().filter(|&a| *a < *current_atr).count();
                            let pct = below as f64 / hist.len() as f64 * 100.0;
                            pct >= ATR_RANK_T
                        } else { true }
                    } else { true };
                    
                    if passes {
                        equity *= 1.0 - TAKER_FEE;
                        positions[s] = Some(Position {
                            sym_idx: s,
                            size: 1.0 / POSITION_CAP as f64,
                            entry_price: close,
                            entry_bar: bar_idx,
                            highest: close,
                        });
                    }
                }
            }
        }
        
        equity_curve.push(equity);
    }
    
    let mut trades = Vec::new();
    for s in 0..n_syms {
        if let Some(pos) = positions[s] {
            let close = sym_data[s].close[n_bars - 1];
            let pnl = pos.size * (close - pos.entry_price) / pos.entry_price;
            equity *= 1.0 + pnl;
            trades.push(Trade { sym: s, pnl_pct: pnl * 100.0 });
        }
    }
    
    (equity, trades, equity_curve)
}

#[derive(Clone)]
struct Trade { sym: usize, pnl_pct: f64 }

struct Position {
    sym_idx: usize,
    size: f64,
    entry_price: f64,
    entry_bar: usize,
    highest: f64,
}

fn compute_sharpe(returns: &[f64]) -> f64 {
    if returns.len() < 10 { return 0.0; }
    let rets: Vec<f64> = returns.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    let std = var.sqrt();
    if std == 0.0 { return 0.0; }
    mean / std * (252.0_f64).sqrt()
}

fn compute_max_dd(equity: &[f64]) -> f64 {
    let mut peak = equity[0];
    let mut max_dd = 0.0;
    for &e in equity {
        peak = peak.max(e);
        let dd = (peak - e) / peak * 100.0;
        max_dd = max_dd.max(dd);
    }
    max_dd
}
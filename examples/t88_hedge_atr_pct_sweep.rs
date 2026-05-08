//! T88: HEDGE_ATR_PCT extensive hyperopt.
//!
//! Hardcoded assumption: HEDGE_ATR_PCT = 0.45 (45th percentile).
//! This is never independently tested. The sweep tests values around it.
//!
//! Current production config: Turtle-only path with HEDGE_ATR_PERIOD=38, HSM=0.25.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const WARMUP_BARS: usize = 300;
const MIN_TRADES: usize = 3;
const FEE: f64 = 0.0004;
const FRESHNESS_COOLDOWN: usize = 0;

// Current config
const TURTLE_EP: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.00;
const HOLD_MAX: usize = 15;
const POSITION_CAP: usize = 3;
const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 41;
const ATR_RANK_THRESHOLD: f64 = 5.0;
const HEDGE_ATR_PERIOD: usize = 38;
const HEDGE_LOOKBACK: usize = 252;
const HEDGE_SIZE_MULT: f64 = 0.25;

// Sweep: 15 values from 0.10 to 0.90 step 0.05
const PCT_MIN: usize = 10;
const PCT_MAX: usize = 90;
const PCT_STEP: usize = 5;

const SUMMARY_OUT: &str =
    "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t88_hedge_atr_pct_summary.csv";
const WINDOWS_OUT: &str =
    "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t88_hedge_atr_pct_windows.csv";
const EQUITY_OUT: &str =
    "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t88_hedge_atr_pct_equity.csv";

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "NoDOGE",
        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"],
    ),
    (
        "Legacy4",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"],
    ),
    (
        "Legacy5BNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
        ],
    ),
    (
        "OldGuardNoBNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
    ),
    (
        "LargeCaps5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT",
        ],
    ),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    (
        "OldGuard4",
        &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
];

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
}

#[derive(Clone)]
struct PositionState {
    entry_bar: usize,
    entry_exec: f64,
    size: f64,
    highest_high: f64,
    lowest_low: f64,
    bars_held: usize,
    atr_buf: VecDeque<f64>,
}

#[derive(Clone)]
struct SimResult {
    final_equity: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    wins: usize,
    equity_curve: Vec<f64>,
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

/// Mirrors LiveBot::btc_atr_percentile exactly.
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
    for i in start..=idx {
        let hist_atr = atr_at(btc, atr_period, i);
        if hist_atr > 0.0 {
            let hist_pct = hist_atr / btc.close[i];
            if hist_pct < curr_pct {
                below += 1;
            }
            total += 1;
        }
    }
    if total == 0 {
        return 50.0;
    }
    below as f64 / total as f64 * 100.0
}

/// Simulates exact Turtle-only live path with configurable HEDGE_ATR_PCT.
fn simulate_window(
    symbols: &[&str],
    btc: &SymData,
    data: &HashMap<String, SymData>,
    hedge_pct: f64,
    window_idx: usize,
    total_windows: usize,
) -> SimResult {
    let n = CANDLES as usize;
    let test_len = TEST_BARS;
    let test_end = n - test_len * (total_windows - 1 - window_idx);
    let test_start = test_end.saturating_sub(test_len);
    let warmup = WARMUP_BARS;

    let mut equity = 1.0;
    let mut equity_curve = Vec::with_capacity(test_end - test_start);
    let mut peak = 1.0;
    let mut max_dd = 0.0;
    let mut trades = 0;
    let mut wins = 0;

    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut last_exit: HashMap<String, usize> = HashMap::new();

    // Pre-compute BTC ATR percentile for all bars
    let mut btc_pct: Vec<f64> = Vec::with_capacity(n);
    for i in 0..n {
        btc_pct.push(btc_atr_percentile(
            btc,
            REGIME_ATR_PERIOD,
            REGIME_LOOKBACK,
            i,
        ));
    }

    // Pre-compute BTC hedge ATR for hedge overlay
    let mut btc_hedge_atr: Vec<f64> = Vec::with_capacity(n);
    for i in 0..n {
        btc_hedge_atr.push(atr_at(btc, HEDGE_ATR_PERIOD, i));
    }

    for idx in test_start..test_end {
        if idx < warmup {
            equity_curve.push(equity);
            continue;
        }

        // === ENTRY FILTER: ATR_RANK ===
        let pct_rank = btc_pct.get(idx).copied().unwrap_or(50.0);
        if pct_rank < ATR_RANK_THRESHOLD {
            equity_curve.push(equity);
            continue;
        }

        // === POSITION MANAGEMENT ===
        // Find positions to close (Turtle exit)
        let mut to_close: Vec<String> = Vec::new();
        for (sym, pos) in positions.iter_mut() {
            pos.bars_held += 1;
            let sd = data.get(*sym)?;
            let curr = &sd.close[idx];
            let entry = pos.entry_exec;
            
            // Update trailing stop
            if *curr > pos.highest_high {
                pos.highest_high = *curr;
            }
            if *curr < pos.lowest_low {
                pos.lowest_low = *curr;
            }

            // Turtle ATR exit
            let exit_trigger = pos.lowest_low
                - atr_at(sd, TURTLE_ATR_PERIOD, idx).max(0.0) * TURTLE_ATR_MULT * pos.size * pos.entry_exec;
            if *curr <= exit_trigger || pos.bars_held >= HOLD_MAX {
                let ret = (*curr - entry) / entry;
                equity *= 1.0 + ret;
                trades += 1;
                if ret > 0.0 {
                    wins += 1;
                }
                to_close.push(sym.clone());
                last_entry.insert(sym.clone(), idx);
            }
        }

        // Close positions
        for sym in &to_close {
            positions.remove(sym);
        }

        // === ENTRY: Turtle breakout ===
        if positions.len() < POSITION_CAP {
            for sym in symbols {
                let can_enter = last_exit
                    .get(*sym)
                    .map(|le| idx.saturating_sub(*le) > FRESHNESS_COOLDOWN)
                    .unwrap_or(true);

                if !can_enter {
                    continue;
                }

                let Some(sd) = data.get(*sym) else {
                    continue;
                };

                // Check if already have position
                if positions.contains_key(*sym) {
                    continue;
                }

                // Turtle breakout: price > highest of last EP bars
                if idx >= TURTLE_EP + 1 {
                    let mut breakout = false;
                    let ep_start = idx + 1 - TURTLE_EP;
                    for i in ep_start..=idx {
                        if i == 0 {
                            continue;
                        }
                        if sd.close[i] > sd.high[ep_start] {
                            breakout = true;
                            break;
                        }
                    }

                    if breakout {
                        let mut size = 1.0 / POSITION_CAP as f64;

                        // Hedge overlay
                        let hedge_fire = hedge_should_fire(
                            btc, &btc_hedge_atr, hedge_pct, idx,
                        );
                        if hedge_fire {
                            size *= HEDGE_SIZE_MULT;
                        }

                        positions.insert(
                            (*sym).to_string(),
                            PositionState {
                                entry_bar: idx,
                                entry_exec: sd.close[idx],
                                size,
                                highest_high: sd.close[idx],
                                lowest_low: sd.close[idx],
                                bars_held: 0,
                                atr_buf: VecDeque::new(),
                            },
                        );
                        last_exit.insert((*sym).to_string(), idx);
                    }
                }
            }
        }

        // Update equity
        if equity > peak {
            peak = equity;
        }
        let dd = (peak - equity) / peak;
        if dd > max_dd {
            max_dd = dd;
        }

        equity_curve.push(equity);
    }

    // Sharpe
    let mut returns = Vec::new();
    for i in 1..equity_curve.len() {
        let r = (equity_curve[i] - equity_curve[i - 1]) / equity_curve[i - 1];
        if r.is_finite() {
            returns.push(r);
        }
    }

    let sharpe = if returns.len() > 10 {
        let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
        let var: f64 = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
        let std = var.sqrt();
        if std > 0.0 {
            (mean / std) * (252.0_f64.sqrt())
        } else {
            0.0
        }
    } else {
        0.0
    };

    SimResult {
        final_equity: equity,
        sharpe,
        max_dd_pct: max_dd * 100.0,
        trades,
        wins,
        equity_curve,
    }
}

/// Check if hedge should fire (BTC ATR > HEDGE_LOOKBACK percentile of hedge ATR history).
fn hedge_should_fire(
    btc: &SymData,
    btc_hedge_atr: &[f64],
    pct: f64,
    idx: usize,
) -> bool {
    if idx < HEDGE_LOOKBACK + HEDGE_ATR_PERIOD || idx >= btc.close.len() {
        return false;
    }

    let curr_atr = btc_hedge_atr.get(idx).copied().unwrap_or(0.0);
    if curr_atr <= 0.0 {
        return false;
    }

    // Build history of hedge ATR values (38-bar ATR = the same calculation)
    let mut history: Vec<f64> = Vec::with_capacity(HEDGE_LOOKBACK);
    for i in (idx.saturating_sub(HEDGE_LOOKBACK) + 1)..=idx {
        if let Some(&atr) = btc_hedge_atr.get(i) {
            if atr > 0.0 {
                history.push(atr);
            }
        }
    }

    if history.len() < 100 {
        return false;
    }

    history.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (pct * history.len() as f64) as usize;
    let pct_idx = pct_idx.min(history.len() - 1);

    if let Some(&threshold) = history.get(pct_idx) {
        return curr_atr > threshold;
    }

    false
}

fn main() -> Result<()> {
    // Load data
    let mut loader = DataLoader::new();
    loader.load_cached_parquet(
        std::path::Path::new("/home/ubuntu/.openclaw/workspace-krypto/krypto/data/cache"),
    )?;

    let symbols = [
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        "LTCUSDT", "EOSUSDT", "BCHUSDT", "BNBUSDT",
    ];

    let mut data: HashMap<String, SymData> = HashMap::new();
    for sym in symbols {
        if let Ok(bars) = loader.load_symbol(sym, CANDLES) {
            let close: Vec<f64> = bars.iter().map(|b| b.close).collect();
            let high: Vec<f64> = bars.iter().map(|b| b.high).collect();
            let low: Vec<f64> = bars.iter().map(|b| b.low).collect();
            data.insert(
                sym.to_string(),
                SymData { close, high, low },
            );
        }
    }

    let btc = data.get("BTCUSDT").cloned().unwrap();
    let total_windows = 6;

    println!("T88: HEDGE_ATR_PCT hyperoptimization");
    println!("Sweep: {}..={} step {} ({} values)", PCT_MIN, PCT_MAX, PCT_STEP,
        (PCT_MAX - PCT_MIN) / PCT_STEP + 1);
    println!("Loaded {} symbols, {} windows", data.len(), total_windows);

    // Run sweep
    let pct_values: Vec<f64> = (PCT_MIN..=PCT_MAX)
        .step_by(PCT_STEP)
        .map(|v| v as f64 / 100.0)
        .collect();

    let mut results: Vec<(f64, usize, f64, f64, f64)> = Vec::new();
    let mut window_results: Vec<(String, f64, usize, usize, f64, f64, f64)> = Vec::new();
    let mut equity_by_pct: HashMap<String, Vec<f64>> = HashMap::new();

    for (pIdx, pct) in pct_values.iter().enumerate() {
        println!("Testing PCT={:.2} ({}/{})", pct, pIdx + 1, pct_values.len());

        let mut total_pass = 0;
        let mut total_windows = 0;
        let mut total_sharpe = 0.0;
        let mut total_dd = 0.0;
        let mut total_ret = 0.0;

        for (universe, symbols) in UNIVERSES.iter() {
            let sym_refs: Vec<&str> = symbols.iter().map(|s| *s).collect();

            for window_idx in 0..total_windows {
                let result = simulate_window(
                    &sym_refs,
                    &btc,
                    &data,
                    *pct,
                    window_idx,
                    total_windows,
                );

                total_windows += 1;
                if result.trades >= MIN_TRADES && result.sharpe > 0.0 {
                    total_pass += 1;
                }
                total_sharpe += result.sharpe;
                total_dd += result.max_dd_pct;
                total_ret += (result.final_equity - 1.0) * 100.0;

                // Store window results
                window_results.push((
                    universe.to_string(),
                    *pct,
                    window_idx,
                    result.trades,
                    result.sharpe,
                    result.max_dd_pct,
                    result.final_equity,
                ));

                // Store equity for baseline (0.45) and winner candidate (first/last only)
                if (*pct - 0.45_f64).abs() < 0.001 || pIdx == 0 || pIdx == pct_values.len() / 2 || pIdx == pct_values.len() - 1 {
                    let key = format!("{}_{}_{}", universe, pct, window_idx);
                    equity_by_pct.insert(key, result.equity_curve);
                }
            }
        }

        let avg_sharpe = total_sharpe / total_windows as f64;
        let avg_dd = total_dd / total_windows as f64;
        let avg_ret = total_ret / total_windows as f64;

        results.push((*pct, total_pass, total_windows, avg_sharpe, avg_dd, avg_ret));

        println!("  {}: {}/{} pass, Sharpe {:.3}, DD {:.1}%",
            pct, total_pass, total_windows, avg_sharpe, avg_dd);
    }

    // Sort by Sharpe descending
    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    // Write CSVs
    let mut f = File::create(SUMMARY_OUT)?;
    writeln!(f, "pct,pass,total,sharpe,dd,ret")?;
    for (pct, pass, total, sharpe, dd, ret) in &results {
        writeln!(f, "{},{},{},{:.4},{:.2},{:.2}", pct, pass, total, sharpe, dd, ret)?;
    }

    let mut f = File::create(WINDOWS_OUT)?;
    writeln!(f, "universe,pct,window,trades,sharpe,dd,equity")?;
    for (universe, pct, window, trades, sharpe, dd, equity) in &window_results {
        writeln!(f, "{},{:.2},{},{},{:.4},{:.2},{:.4}", universe, pct, window, trades, sharpe, dd, equity)?;
    }

    let mut f = File::create(EQUITY_OUT)?;
    writeln!(f, "pct,universe,window,equity")?;
    for (key, curve) in equity_by_pct.iter() {
        let parts: Vec<&str> = key.split('_').collect();
        if parts.len() >= 3 {
            let equity_str = curve.last().copied().unwrap_or(1.0);
            writeln!(f, "{},{},{},{:.4}", parts[1], parts[0], parts[2], equity_str)?;
        }
    }

    println!("\n>>> Results written");
    println!("Summary: {}", SUMMARY_OUT);
    println!("Windows: {}", WINDOWS_OUT);
    println!("Equity: {}", EQUITY_OUT);

    // Show winner
    let winner = results[0];
    println!("\n=== TOP 5 ===");
    for (i, r) in results.iter().take(5).enumerate() {
        let marker = if i == 0 { " <-- WINNER" } else { "" };
        println!("PCT={:.2}: {}/63 pass, Sharpe {:.3}{}", r.0, r.1, r.2, marker);
    }

    let baseline = results.iter().find(|(p, ..)| (*p - 0.45).abs() < 0.001).copied();
    let baseline_sharpe = baseline.map(|(_, _, s, _, _)| s).unwrap_or(0.0);

    println!("\nBaseline PCT=0.45: Sharpe {:.3}", baseline_sharpe);
    println!("Winner  PCT={:.2}: Sharpe {:.3}", winner.0, winner.2);

    if winner.2 > baseline_sharpe + 0.05 {
        println!("\n>>> RECOMMENDATION: Change HEDGE_ATR_PCT from 0.45 to {:.2}", winner.0);
        println!("    Delta Sharpe: +{:.3}", winner.2 - baseline_sharpe);
    } else {
        println!("\n>>> No change: winner within tolerance");
    }

    Ok(())
}
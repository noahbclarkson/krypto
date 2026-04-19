//! Hyperparameter Optimization: BollingerReversion RSI Filter Threshold
//!
//! TARGET: RSI filter threshold (hardcoded at 20.0 in code default, only tested
//!         with 3 values on DOGE via full-sample backtest — NEVER walk-forward validated)
//!
//! CONTEXT: BollingerReversion with ATR×0.30 stop is our strongest Sharpe strategy.
//!          The RSI filter controls entry selectivity:
//!          - Low (10-15): Only enter deeply oversold conditions → few, high-conviction trades
//!          - Medium (20-30): Standard Bollinger oversold → balanced
//!          - High (40-50): Enter even mild oversold → many trades, lower selectivity
//!
//! SWEEP: rsi_filter ∈ {5,10,15,20,25,30,35,40,45,50} (10 values, full logical range)
//!
//! STRATEGY: BollingerReversion
//!   - bb_period: 30 (code default)
//!   - bb_std: 2.5 (code default)
//!   - rsi_filter: SWEEP VARIABLE
//!   - Stop: ATR×0.30 (proven optimal)
//!   - TP: 0 (proven irrelevant)
//!
//! SYMBOLS: All 5 FDUSD symbols (BTC, ETH, SOL, XRP, DOGE)
//! METHOD: Walk-forward 252/252, ~6 windows per symbol
//! FEE: 0.1% taker each side
//!
//! EXPORTS:
//!   - snapshots/bollinger_rsi_filter_results.csv (all per-window results)
//!   - snapshots/bollinger_rsi_filter_summary.csv (aggregated)
//!   - snapshots/bollinger_rsi_filter_equity.csv (equity curves for charting)
//!
//! Usage:
//!   cargo run --profile sweep --example bollinger_rsi_filter_hyperopt

use anyhow::Result;
use krypto::{
    algo::strategies::BollingerReversion,
    backtest::engine::Backtester,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write as IoWrite;

const CANDLES: u32 = 2500;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30; // Proven optimal — fixed
const BB_PERIOD: usize = 20; // HOF-validated settings (not code defaults of 30/2.5)
const BB_STD: f64 = 2.0; // HOF-validated settings
const INTERVAL: &str = "1d";
const TP_PCT: f64 = 0.0; // Proven irrelevant

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES: usize = 5;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

// Extensive RSI filter sweep — full logical range
const RSI_FILTERS: &[f64] = &[5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0];
const BASELINE_RSI: f64 = 20.0; // Code default

// ── Result structures ────────────────────────────────────────────────────────

#[derive(Clone)]
struct WindowResult {
    symbol: String,
    window: usize,
    rsi_filter: f64,
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate: f64,
    passed: bool,
}

#[derive(Clone)]
struct AggResult {
    rsi_filter: f64,
    avg_return: f64,
    avg_sharpe: f64,
    avg_dd: f64,
    total_trades: usize,
    avg_win_rate: f64,
    pass_rate: f64,
    windows_positive: usize,
    total_windows: usize,
    is_baseline: bool,
}

// ── Generate signals with custom RSI filter ──────────────────────────────────

fn generate_signals_with_rsi(df: &DataFrame, bb_period: usize, bb_std: f64, rsi_filter: f64) -> Vec<f64> {
    let close_col = df.column("close").unwrap().f64().unwrap();
    let rsi_col = df.column("rsi").unwrap().f64().unwrap();
    let n = df.height();

    let mut queue = std::collections::VecDeque::with_capacity(bb_period + 1);
    let mut sum = 0.0_f64;
    let mut sum_sq = 0.0_f64;

    let mut upper = vec![f64::NAN; n];
    let mut lower = vec![f64::NAN; n];

    for i in 0..n {
        let price = close_col.get(i).unwrap_or(0.0);
        queue.push_back(price);
        sum += price;
        sum_sq += price * price;

        if queue.len() > bb_period {
            if let Some(old) = queue.pop_front() {
                sum -= old;
                sum_sq -= old * old;
            }
        }

        if queue.len() == bb_period {
            let count = bb_period as f64;
            let mean = sum / count;
            let var = (sum_sq / count) - (mean * mean);
            let std = var.max(0.0).sqrt();
            upper[i] = mean + std * bb_std;
            lower[i] = mean - std * bb_std;
        }
    }

    let mut signals = vec![0.0; n];
    for i in 0..n {
        let c = close_col.get(i).unwrap_or(0.0);
        let l = lower[i];
        let u = upper[i];
        let r = rsi_col.get(i).unwrap_or(50.0);

        if l.is_finite() && c < l && r < rsi_filter {
            signals[i] = 1.0; // long when below lower band AND RSI < threshold
        } else if u.is_finite() && c > u {
            signals[i] = -1.0; // short when above upper band (no RSI filter for shorts)
        }
    }
    signals
}

// ── Walk-forward window equity curve extraction ──────────────────────────────

struct WfEquityResult {
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate: f64,
    equity_curve: Vec<f64>,
}

fn backtest_window_equity(
    close: &[f64],
    signals: &[f64],
    stop_pct: f64,
    train_end: usize,
    test_end: usize,
) -> WfEquityResult {
    let n = close.len();
    let test_end = test_end.min(n);
    let mut equity = CAPITAL;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut daily_rets: Vec<f64> = Vec::new();
    let mut equity_curve: Vec<f64> = vec![equity];

    let mut i = train_end;
    while i < test_end {
        let sig = if i > 0 { signals[i - 1] } else { 0.0 };

        if sig != 0.0 {
            let entry_price = close[i];
            if entry_price <= 0.0 {
                i += 1;
                continue;
            }
            let direction = sig.signum();

            let mut exit_price = entry_price;
            let mut exited = false;
            let mut exit_bar = i + 1;

            for j in (i + 1)..test_end {
                let pnl_pct = direction * (close[j] / entry_price - 1.0);

                // Stop hit
                if pnl_pct < -stop_pct {
                    exit_price = close[j];
                    exited = true;
                    exit_bar = j;
                    break;
                }
                // Signal reversal
                if signals[j] == -sig {
                    exit_price = close[j];
                    exited = true;
                    exit_bar = j;
                    break;
                }
            }

            if !exited {
                exit_price = close[test_end - 1];
                exit_bar = test_end - 1;
            }

            let gross = direction * (exit_price / entry_price - 1.0) - 2.0 * TAKER_FEE;
            equity *= 1.0 + gross;
            trades += 1;
            if gross > 0.0 { wins += 1; }
            daily_rets.push(gross);

            // Fill equity curve
            for _ in i..=exit_bar.min(test_end - 1) {
                equity_curve.push(equity);
            }

            peak = peak.max(equity);
            max_dd = max_dd.min(equity / peak - 1.0);
            i = exit_bar + 1;
        } else {
            equity_curve.push(equity);
            i += 1;
        }
    }

    let ret = (equity / CAPITAL - 1.0) * 100.0;
    let sh = if daily_rets.len() >= 2 {
        let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
        let var = daily_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
        let std = var.sqrt();
        if std > 0.0 { mean / std * 252.0_f64.sqrt() } else { 0.0 }
    } else {
        0.0
    };

    WfEquityResult {
        return_pct: ret,
        sharpe: sh,
        max_dd_pct: max_dd * 100.0,
        trades,
        win_rate: if trades > 0 { wins as f64 / trades as f64 * 100.0 } else { 0.0 },
        equity_curve,
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ BollingerReversion RSI Filter Hyperopt ═══");
    println!("Target: RSI filter threshold (current default: {}, only tested 3 values on DOGE)", BASELINE_RSI);
    println!("Sweep: {} values from {:.0} to {:.0}", RSI_FILTERS.len(), RSI_FILTERS[0], RSI_FILTERS[RSI_FILTERS.len()-1]);
    println!("Fixed: bb_period={}, bb_std={:.1}, ATR×{:.2}, TP=0\n", BB_PERIOD, BB_STD, ATR_MULT);

    let loader = DataLoader::new(None, None);

    // Load data and compute features
    let mut data_cache: HashMap<String, (Vec<f64>, DataFrame)> = HashMap::new();
    for &sym in SYMBOLS {
        let raw = loader.fetch_with_cache(sym, INTERVAL, CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        let close: Vec<f64> = {
            let col = df.column("close")?.f64()?;
            (0..col.len()).map(|i| col.get(i).unwrap_or(0.0)).collect()
        };
        println!("  {} loaded: {} bars", sym, df.height());
        data_cache.insert(sym.to_string(), (close, df));
    }

    // Pre-compute stop percentage per symbol (using ATR×0.30 from feature engine ATR(14))
    let mut stop_cache: HashMap<String, f64> = HashMap::new();
    for &sym in SYMBOLS {
        let (_, df) = data_cache.get(sym).unwrap();
        let atr_col = df.column("atr").unwrap().f64().unwrap();
        let close_col = df.column("close").unwrap().f64().unwrap();
        let n = df.height();
        let mut atr_sum = 0.0;
        let mut close_sum = 0.0;
        let mut count = 0usize;
        for i in (n / 2)..n { // Use second half for representative stop calc
            let a = atr_col.get(i).unwrap_or(0.0);
            let c = close_col.get(i).unwrap_or(0.0);
            if a > 0.0 && c > 0.0 {
                atr_sum += a;
                close_sum += c;
                count += 1;
            }
        }
        let avg_stop = if count > 0 {
            (atr_sum / count as f64) * ATR_MULT / (close_sum / count as f64)
        } else {
            0.02
        };
        let stop_pct = avg_stop.clamp(0.005, 0.30);
        stop_cache.insert(sym.to_string(), stop_pct);
        println!("  {} stop: {:.2}%", sym, stop_pct * 100.0);
    }

    // Walk-forward sweep
    println!("\n═══ Walk-Forward Sweep ═══");
    let mut all_results: Vec<WindowResult> = Vec::new();
    let mut all_equity_curves: Vec<(f64, String, Vec<f64>)> = Vec::new();

    for &rsi_filter in RSI_FILTERS {
        let is_bl = (rsi_filter - BASELINE_RSI).abs() < 0.01;
        print!("  RSI({:5.1}): ", rsi_filter);

        for &sym in SYMBOLS {
            let (close, df) = data_cache.get(sym).unwrap();
            let stop_pct = *stop_cache.get(sym).unwrap();
            let n = close.len();

            // Generate signals with this RSI filter
            let signals = generate_signals_with_rsi(df, BB_PERIOD, BB_STD, rsi_filter);

            let total_windows = if n > TRAIN_BARS + TEST_BARS {
                (n - TRAIN_BARS - TEST_BARS) / TEST_BARS
            } else {
                0
            };

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (train_end + TEST_BARS).min(n);
                if test_end <= train_end + 10 { continue; }

                let result = backtest_window_equity(close, &signals, stop_pct, train_end, test_end);
                let passed = result.trades >= MIN_TRADES && result.return_pct > 0.0;

                all_results.push(WindowResult {
                    symbol: sym.to_string(),
                    window: wi,
                    rsi_filter,
                    return_pct: result.return_pct,
                    sharpe: result.sharpe,
                    max_dd_pct: result.max_dd_pct,
                    trades: result.trades,
                    win_rate: result.win_rate,
                    passed,
                });

                // Save equity curves for baseline, winner candidates, and a few key values
                if is_bl || rsi_filter <= 10.0 || rsi_filter == 30.0 || rsi_filter == 40.0 || rsi_filter >= 45.0 {
                    all_equity_curves.push((
                        rsi_filter,
                        format!("{}_W{}", sym, wi),
                        result.equity_curve,
                    ));
                }
            }
        }
        print!("done\n");
    }

    // ── Aggregate results ────────────────────────────────────────────────────
    println!("\n═══ Aggregate Results (All Symbols) ═══");
    let mut agg_results: Vec<AggResult> = Vec::new();

    for &rsi in RSI_FILTERS {
        let windows: Vec<&WindowResult> = all_results.iter().filter(|r| (r.rsi_filter - rsi).abs() < 0.01).collect();
        if windows.is_empty() { continue; }

        let n_wins = windows.len();
        let avg_ret = windows.iter().map(|r| r.return_pct).sum::<f64>() / n_wins as f64;
        let avg_sh = windows.iter().map(|r| r.sharpe).sum::<f64>() / n_wins as f64;
        let avg_dd = windows.iter().map(|r| r.max_dd_pct).sum::<f64>() / n_wins as f64;
        let total_tr = windows.iter().map(|r| r.trades).sum();
        let avg_wr = windows.iter().map(|r| r.win_rate).sum::<f64>() / n_wins as f64;
        let n_pass = windows.iter().filter(|r| r.passed).count();
        let n_pos = windows.iter().filter(|r| r.return_pct > 0.0).count();

        agg_results.push(AggResult {
            rsi_filter: rsi,
            avg_return: avg_ret,
            avg_sharpe: avg_sh,
            avg_dd: avg_dd,
            total_trades: total_tr,
            avg_win_rate: avg_wr,
            pass_rate: n_pass as f64 / n_wins as f64 * 100.0,
            windows_positive: n_pos,
            total_windows: n_wins,
            is_baseline: (rsi - BASELINE_RSI).abs() < 0.01,
        });
    }

    // Sort by Sharpe
    agg_results.sort_by(|a, b| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap_or(std::cmp::Ordering::Equal));

    println!("{:>3} {:>6} {:>10} {:>10} {:>10} {:>8} {:>8} {:>8} {:>8}  {}",
        "#", "RSI", "AvgRet%", "AvgSharpe", "AvgDD%", "Trades", "WinRate", "Pass%", "PosWin", "Status");
    println!("{}", "─".repeat(95));

    for (rank, ar) in agg_results.iter().enumerate() {
        let status = if ar.is_baseline { "← BASELINE" }
                     else if rank == 0 { "★ WINNER" }
                     else if rank <= 3 { "↑ TOP" }
                     else { "" };
        println!("{:3}: RSI={:5.1} {:>+9.1}% {:>+10.2} {:>9.1}% {:>8} {:>7.1}% {:>7.1}% {:>5}/{}  {}",
            rank + 1, ar.rsi_filter, ar.avg_return, ar.avg_sharpe, ar.avg_dd,
            ar.total_trades, ar.avg_win_rate, ar.pass_rate,
            ar.windows_positive, ar.total_windows, status);
    }

    // ── Per-symbol breakdown ─────────────────────────────────────────────────
    println!("\n═══ Per-Symbol Breakdown ═══");
    for &sym in SYMBOLS {
        println!("\n  {}:", sym);
        let mut sym_aggs: Vec<(f64, f64, f64, usize)> = Vec::new();
        for &rsi in RSI_FILTERS {
            let wins: Vec<&WindowResult> = all_results.iter()
                .filter(|r| r.symbol == sym && (r.rsi_filter - rsi).abs() < 0.01)
                .collect();
            if wins.is_empty() { continue; }
            let avg_sh = wins.iter().map(|r| r.sharpe).sum::<f64>() / wins.len() as f64;
            let avg_ret = wins.iter().map(|r| r.return_pct).sum::<f64>() / wins.len() as f64;
            let total_tr = wins.iter().map(|r| r.trades).sum();
            sym_aggs.push((rsi, avg_sh, avg_ret, total_tr));
        }
        sym_aggs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (rank, (rsi, sh, ret, tr)) in sym_aggs.iter().enumerate() {
            let bl = if (*rsi - BASELINE_RSI).abs() < 0.01 { " ←BL" } else if rank == 0 { " ★" } else { "" };
            println!("    {:2}: RSI={:5.1} Sharpe={:+.2} Ret={:+.1}% Trades={}{}", rank+1, rsi, sh, ret, tr, bl);
        }
    }

    // ── Export results ───────────────────────────────────────────────────────
    let snap_dir = std::path::Path::new("snapshots");
    std::fs::create_dir_all(snap_dir)?;

    // Full results CSV
    {
        let mut csv = File::create(snap_dir.join("bollinger_rsi_filter_results.csv"))?;
        writeln!(csv, "symbol,window,rsi_filter,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,passed")?;
        for r in &all_results {
            writeln!(csv, "{},{},{:.1},{:.2},{:.2},{:.2},{},{:.1},{}",
                r.symbol, r.window, r.rsi_filter, r.return_pct, r.sharpe,
                r.max_dd_pct, r.trades, r.win_rate, r.passed)?;
        }
    }

    // Summary CSV
    {
        let mut csv = File::create(snap_dir.join("bollinger_rsi_filter_summary.csv"))?;
        writeln!(csv, "rank,rsi_filter,avg_return,avg_sharpe,avg_dd,total_trades,avg_win_rate,pass_rate,windows_positive,total_windows")?;
        for (rank, ar) in agg_results.iter().enumerate() {
            writeln!(csv, "{},{:.1},{:.2},{:.2},{:.2},{},{:.1},{:.1},{},{}",
                rank + 1, ar.rsi_filter, ar.avg_return, ar.avg_sharpe, ar.avg_dd,
                ar.total_trades, ar.avg_win_rate, ar.pass_rate,
                ar.windows_positive, ar.total_windows)?;
        }
    }

    // Equity curves for charting
    {
        let winner_rsi = agg_results[0].rsi_filter;
        let periods_to_export: Vec<f64> = {
            let mut set = vec![BASELINE_RSI, winner_rsi];
            for ar in agg_results.iter().take(4) {
                set.push(ar.rsi_filter);
            }
            set.sort_by(|a, b| a.partial_cmp(b).unwrap());
            set.dedup_by(|a, b| (*a - *b).abs() < 0.01);
            set
        };

        let mut csv = File::create(snap_dir.join("bollinger_rsi_filter_equity.csv"))?;
        writeln!(csv, "bar,rsi_filter,avg_equity")?;

        for &rsi in &periods_to_export {
            let curves: Vec<&Vec<f64>> = all_equity_curves.iter()
                .filter(|(r, _, _)| (r - rsi).abs() < 0.01)
                .map(|(_, _, curve)| curve)
                .collect();

            if curves.is_empty() { continue; }
            let max_len = curves.iter().map(|c| c.len()).max().unwrap_or(0);

            for bar in 0..max_len {
                let mut sum = 0.0;
                let mut count = 0;
                for curve in &curves {
                    if bar < curve.len() {
                        sum += curve[bar];
                        count += 1;
                    }
                }
                if count > 0 {
                    writeln!(csv, "{},{:.1},{:.2}", bar, rsi, sum / count as f64)?;
                }
            }
        }
    }

    println!("\n═══ Exports ═══");
    println!("  snapshots/bollinger_rsi_filter_results.csv");
    println!("  snapshots/bollinger_rsi_filter_summary.csv");
    println!("  snapshots/bollinger_rsi_filter_equity.csv");

    // ── Final verdict ────────────────────────────────────────────────────────
    let winner = &agg_results[0];
    let baseline = agg_results.iter().find(|r| r.is_baseline).unwrap_or(&agg_results[0]);
    let delta_sh = if baseline.avg_sharpe.abs() > 0.001 {
        (winner.avg_sharpe - baseline.avg_sharpe) / baseline.avg_sharpe.abs() * 100.0
    } else { 0.0 };

    println!("\n═══ VERDICT ═══");
    println!("  Baseline: RSI({:.0}) Sharpe={:.2} Pass={:.0}% Trades={}", BASELINE_RSI, baseline.avg_sharpe, baseline.pass_rate, baseline.total_trades);
    println!("  Winner:   RSI({:.0}) Sharpe={:.2} Pass={:.0}% Trades={} ({:+.1}%)", winner.rsi_filter, winner.avg_sharpe, winner.pass_rate, winner.total_trades, delta_sh);

    if (winner.rsi_filter - BASELINE_RSI).abs() > 0.01 {
        println!("  → RSI {:.0} beats baseline {:.0} — candidate for default update", winner.rsi_filter, BASELINE_RSI);
    } else {
        println!("  → Baseline RSI({:.0}) is already optimal — no change needed", BASELINE_RSI);
    }

    // Robustness check: how many symbols agree with the winner?
    let winner_rsi = winner.rsi_filter;
    let mut sym_wins = 0usize;
    let mut sym_total = 0usize;
    for &sym in SYMBOLS {
        let mut best_sh = f64::NEG_INFINITY;
        let mut best_rsi = 0.0_f64;
        for &rsi in RSI_FILTERS {
            let wins: Vec<&WindowResult> = all_results.iter()
                .filter(|r| r.symbol == sym && (r.rsi_filter - rsi).abs() < 0.01)
                .collect();
            if wins.is_empty() { continue; }
            let avg_sh = wins.iter().map(|r| r.sharpe).sum::<f64>() / wins.len() as f64;
            if avg_sh > best_sh {
                best_sh = avg_sh;
                best_rsi = rsi;
            }
        }
        sym_total += 1;
        let agrees = (best_rsi - winner_rsi).abs() < 1.0;
        println!("  {}: best RSI={:.0} ({})", sym, best_rsi, if agrees { "AGREES" } else { "DISAGREES" });
        if agrees { sym_wins += 1; }
    }
    println!("  Symbol agreement: {}/{}", sym_wins, sym_total);

    Ok(())
}

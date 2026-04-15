//! Hyperparameter Optimization: BollingerReversion ATR Period for Stop-Loss
//!
//! TARGET: ATR EWM Period (hardcoded at 14 — Wilder 1978 default, NEVER validated)
//! CONTEXT: BollingerReversion with ATR_MULT=0.30 is our strongest Sharpe strategy.
//!          The multiplier was optimized, but the ATR lookback period was NOT.
//!          Different ATR periods change stop reactivity to volatility.
//!
//! SWEEP: atr_period ∈ {3,5,7,9,10,12,14,16,18,20,24,28,32,36,42,50,60,75,100} (19 values)
//!        Covers the full logical range: very reactive (3) to very smooth (100).
//!
//! STRATEGY: BollingerReversion (bb_period=20, bb_std=2.0, rsi_filter=30)
//!           Stop = ATR(period) × 0.30
//!           No take profit (proven irrelevant)
//!
//! SYMBOLS: All 5 FDUSD symbols (BTC, ETH, SOL, XRP, DOGE)
//! METHOD: Walk-forward 252/252, ~6 windows per symbol
//! FEE: 0.1% taker each side
//!
//! EXPORTS:
//!   - snapshots/bollinger_atr_period_results.csv (all sweeps)
//!   - snapshots/bollinger_atr_period_equity.csv (equity curves for top configs)
//!   - charts/bollinger_atr_period_chart.png (via Python script)
//!
//! Usage:
//!   cargo run --profile sweep --example bollinger_atr_period_hyperopt

use anyhow::Result;
use krypto::{
    algo::{strategies::BollingerReversion, SignalGenerator},
    backtest::engine::Backtester,
    data::loader::DataLoader,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write as IoWrite;

const CANDLES: u32 = 2500;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30; // Proven optimal — NOT being re-optimized here
const INTERVAL: &str = "1d";

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES_PER_WINDOW: usize = 5;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

// Extensive ATR period sweep — covers very reactive (3) to very smooth (100)
const ATR_PERIODS: &[usize] = &[3, 5, 7, 9, 10, 12, 14, 16, 18, 20, 24, 28, 32, 36, 42, 50, 60, 75, 100];
const BASELINE_PERIOD: usize = 14; // Wilder 1978 default — current hardcoded value

// ── Result structures ────────────────────────────────────────────────────────

#[derive(Clone)]
struct WindowResult {
    symbol: String,
    window: usize,
    period: usize,
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate: f64,
    passed: bool,
}

#[derive(Clone)]
struct AggResult {
    period: usize,
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

// ── ATR computation with configurable EWM span ───────────────────────────────

fn compute_tr(high: &[f64], low: &[f64], close: &[f64]) -> Vec<f64> {
    let n = high.len();
    let mut tr = vec![0.0; n];
    for i in 0..n {
        let hl = high[i] - low[i];
        if i == 0 {
            tr[i] = hl;
        } else {
            let hc = (high[i] - close[i - 1]).abs();
            let lc = (low[i] - close[i - 1]).abs();
            tr[i] = hl.max(hc).max(lc);
        }
    }
    tr
}

fn compute_ewm_atr(tr: &[f64], period: usize) -> Vec<f64> {
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut atr = vec![0.0; tr.len()];
    if tr.is_empty() {
        return atr;
    }
    // SMA seed
    let seed_len = period.min(tr.len());
    let seed: f64 = tr[..seed_len].iter().sum::<f64>() / seed_len as f64;
    for i in 0..seed_len {
        atr[i] = seed;
    }
    // EWM
    for i in seed_len..tr.len() {
        atr[i] = alpha * tr[i] + (1.0 - alpha) * atr[i - 1];
    }
    atr
}

// ── Compute average stop size for a given ATR series ──────────────────────────

fn avg_stop_pct(atr: &[f64], close: &[f64], start: usize, end: usize) -> f64 {
    let mut sum = 0.0;
    let mut count = 0usize;
    for i in start..end.min(atr.len()).min(close.len()) {
        if close[i] > 0.0 && atr[i] > 0.0 {
            sum += (atr[i] * ATR_MULT / close[i]) * 100.0;
            count += 1;
        }
    }
    if count > 0 { sum / count as f64 } else { 0.0 }
}

// ── Single walk-forward window backtest ───────────────────────────────────────

struct WfResult {
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate: f64,
    equity_curve: Vec<f64>, // time-series for charting
}

fn backtest_window(
    close: &[f64],
    signals: &[i32],
    atr: &[f64],
    train_end: usize,
    test_end: usize,
) -> WfResult {
    let n = close.len();
    let test_end = test_end.min(n);

    let stop_pct_base = |bar: usize| -> f64 {
        if bar < atr.len() && bar < close.len() && close[bar] > 0.0 {
            (atr[bar] * ATR_MULT / close[bar]).clamp(0.005, 0.30)
        } else {
            0.03 // fallback
        }
    };

    let mut equity = CAPITAL;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut daily_rets: Vec<f64> = Vec::new();
    let mut equity_curve: Vec<f64> = vec![equity];

    let mut i = train_end;
    while i < test_end {
        let sig = if i > 0 { signals[i - 1] } else { 0 };

        if sig != 0 {
            let entry_price = close[i];
            if entry_price <= 0.0 {
                i += 1;
                continue;
            }
            let stop = stop_pct_base(i);
            let direction = sig as f64; // 1 or -1

            let mut exit_price = entry_price;
            let mut exited = false;

            for j in (i + 1)..test_end {
                let pnl_pct = direction * (close[j] / entry_price - 1.0);

                // Stop hit
                if pnl_pct < -stop {
                    exit_price = close[j] * (1.0 - direction.max(0.0) * TAKER_FEE);
                    exited = true;
                }
                // Natural exit (signal reversal or opposite signal)
                if signals[j] == -sig {
                    exit_price = close[j];
                    exited = true;
                }

                if exited {
                    let gross = direction * (exit_price / entry_price - 1.0) - 2.0 * TAKER_FEE;
                    equity *= 1.0 + gross;
                    trades += 1;
                    if gross > 0.0 { wins += 1; }
                    daily_rets.push(gross);

                    // Fill equity curve for bars in this trade
                    for _ in i..=j {
                        equity_curve.push(equity);
                    }

                    i = j + 1;
                    break;
                }
            }

            if !exited {
                // Force exit at test_end
                exit_price = close[test_end - 1];
                let gross = direction * (exit_price / entry_price - 1.0) - 2.0 * TAKER_FEE;
                equity *= 1.0 + gross;
                trades += 1;
                if gross > 0.0 { wins += 1; }
                daily_rets.push(gross);

                for _ in i..test_end {
                    equity_curve.push(equity);
                }
                i = test_end;
            }

            peak = peak.max(equity);
            max_dd = max_dd.min(equity / peak - 1.0);
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

    WfResult {
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
    println!("═══ BollingerReversion ATR Period Hyperopt ═══");
    println!("Target: ATR EWM period (current default: {}, never validated)", BASELINE_PERIOD);
    println!("Sweep: {} values from {} to {}", ATR_PERIODS.len(), ATR_PERIODS[0], ATR_PERIODS[ATR_PERIODS.len()-1]);
    println!("ATR_MULT: {} (fixed, proven optimal)\n", ATR_MULT);

    let loader = DataLoader::new(None, None);

    // Load data
    let mut raw_cache: HashMap<String, (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>)> = HashMap::new();
    for sym in SYMBOLS {
        let df = loader.fetch_with_cache(sym, INTERVAL, CANDLES).await?;
        let n = df.height();
        let close: Vec<f64> = df.column("close")?.f64()?.iter().map(|v| v.unwrap_or(0.0)).collect();
        let high: Vec<f64> = df.column("high")?.f64()?.iter().map(|v| v.unwrap_or(0.0)).collect();
        let low: Vec<f64> = df.column("low")?.f64()?.iter().map(|v| v.unwrap_or(0.0)).collect();
        let open: Vec<f64> = df.column("open")?.f64()?.iter().map(|v| v.unwrap_or(0.0)).collect();
        println!("  {} loaded: {} bars", sym, n);
        raw_cache.insert(sym.to_string(), (close, high, low, open));
    }

    // Generate BollingerReversion signals (same for all ATR periods — signal is independent of stop)
    // Use the library strategy with default params (bb_period=20, bb_std=2.0)
    let mut signals_cache: HashMap<String, Vec<i32>> = HashMap::new();
    for sym in SYMBOLS {
        let df = loader.fetch_with_cache(sym, INTERVAL, CANDLES).await?;
        let strategy = BollingerReversion::default();
        let signals_df = strategy.generate(&df)?;
        let sig_col = signals_df.column("signal")?.i32()?;
        let sigs: Vec<i32> = (0..sig_col.len()).map(|i| sig_col.get(i).unwrap_or(0)).collect();
        signals_cache.insert(sym.to_string(), sigs);
        println!("  {} signals: {} bars, longs={} shorts={}",
            sym, sigs.len(),
            sigs.iter().filter(|&&s| s == 1).count(),
            sigs.iter().filter(|&&s| s == -1).count()
        );
    }

    // Walk-forward sweep
    let mut all_results: Vec<WindowResult> = Vec::new();
    let mut all_equity_curves: Vec<(usize, String, Vec<f64>)> = Vec::new(); // (period, sym_window, curve)

    println!("\n═══ Walk-Forward Sweep ═══");

    for &period in ATR_PERIODS {
        let is_bl = period == BASELINE_PERIOD;
        print!("  ATR({:3}): ", period);

        for sym in SYMBOLS {
            let (close, high, low, _open) = raw_cache.get(sym).unwrap();
            let signals = signals_cache.get(sym).unwrap();
            let tr = compute_tr(high, low, close);
            let atr = compute_ewm_atr(&tr, period);

            let n = close.len();
            let total_windows = if n > TRAIN_BARS + TEST_BARS {
                (n - TRAIN_BARS - TEST_BARS) / TEST_BARS
            } else {
                0
            };

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (train_end + TEST_BARS).min(n);
                if test_end <= train_end + 10 { continue; }

                let result = backtest_window(close, signals, &atr, train_end, test_end);
                let passed = result.trades >= MIN_TRADES_PER_WINDOW && result.return_pct > 0.0;

                all_results.push(WindowResult {
                    symbol: sym.to_string(),
                    window: wi,
                    period,
                    return_pct: result.return_pct,
                    sharpe: result.sharpe,
                    max_dd_pct: result.max_dd_pct,
                    trades: result.trades,
                    win_rate: result.win_rate,
                    passed,
                });

                // Save equity curves for baseline + top contenders
                if is_bl || period <= 5 || period == 20 || period == 28 || period == 50 || period == 75 {
                    all_equity_curves.push((
                        period,
                        format!("{}_W{}", sym, wi),
                        result.equity_curve,
                    ));
                }
            }
        }
        print!("done\n");
    }

    // ── Aggregate results ────────────────────────────────────────────────────
    println!("\n═══ Aggregate Results ═══");
    let mut agg_results: Vec<AggResult> = Vec::new();

    for &period in ATR_PERIODS {
        let windows: Vec<&WindowResult> = all_results.iter().filter(|r| r.period == period).collect();
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
            period,
            avg_return: avg_ret,
            avg_sharpe: avg_sh,
            avg_dd: avg_dd,
            total_trades: total_tr,
            avg_win_rate: avg_wr,
            pass_rate: n_pass as f64 / n_wins as f64 * 100.0,
            windows_positive: n_pos,
            total_windows: n_wins,
            is_baseline: period == BASELINE_PERIOD,
        });
    }

    // Sort by Sharpe
    agg_results.sort_by(|a, b| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap_or(std::cmp::Ordering::Equal));

    println!("{:>6} {:>10} {:>10} {:>10} {:>8} {:>8} {:>8} {:>8}  {}",
        "Period", "AvgRet%", "AvgSharpe", "AvgDD%", "Trades", "WinRate", "Pass%", "PosWin", "Status");
    println!("{}", "─".repeat(90));

    for (rank, ar) in agg_results.iter().enumerate() {
        let status = if ar.is_baseline { "← BASELINE" }
                     else if rank == 0 { "★ WINNER" }
                     else if rank <= 3 { "↑ TOP" }
                     else { "" };
        println!("{:3}: P={:3} {:>+9.1}% {:>+10.2} {:>9.1}% {:>8} {:>7.1}% {:>7.1}% {:>5}/{}  {}",
            rank + 1, ar.period, ar.avg_return, ar.avg_sharpe, ar.avg_dd,
            ar.total_trades, ar.avg_win_rate, ar.pass_rate,
            ar.windows_positive, ar.total_windows, status);
    }

    // ── Per-symbol breakdown ─────────────────────────────────────────────────
    println!("\n═══ Per-Symbol Breakdown ═══");
    for sym in SYMBOLS {
        println!("\n  {}:", sym);
        let mut sym_aggs: Vec<(&usize, f64, f64, usize)> = Vec::new(); // (period, sharpe, ret, trades)
        for &period in ATR_PERIODS {
            let wins: Vec<&WindowResult> = all_results.iter()
                .filter(|r| r.symbol == sym && r.period == period)
                .collect();
            if wins.is_empty() { continue; }
            let avg_sh = wins.iter().map(|r| r.sharpe).sum::<f64>() / wins.len() as f64;
            let avg_ret = wins.iter().map(|r| r.return_pct).sum::<f64>() / wins.len() as f64;
            let total_tr = wins.iter().map(|r| r.trades).sum();
            sym_aggs.push((&period, avg_sh, avg_ret, total_tr));
        }
        sym_aggs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (rank, (p, sh, ret, tr)) in sym_aggs.iter().enumerate() {
            let bl = if **p == BASELINE_PERIOD { " ←BL" } else if rank == 0 { " ★" } else { "" };
            println!("    {:2}: P={:3} Sharpe={:+.2} Ret={:+.1}% Trades={}{}", rank+1, p, sh, ret, tr, bl);
        }
    }

    // ── Export results ───────────────────────────────────────────────────────
    let snap_dir = std::path::Path::new("snapshots");
    std::fs::create_dir_all(snap_dir)?;

    // Full results CSV
    {
        let mut csv = File::create(snap_dir.join("bollinger_atr_period_results.csv"))?;
        writeln!(csv, "symbol,window,period,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,passed")?;
        for r in &all_results {
            writeln!(csv, "{},{},{},{:.2},{:.2},{:.2},{},{:.1},{}",
                r.symbol, r.window, r.period, r.return_pct, r.sharpe,
                r.max_dd_pct, r.trades, r.win_rate, r.passed)?;
        }
    }

    // Summary CSV
    {
        let mut csv = File::create(snap_dir.join("bollinger_atr_period_summary.csv"))?;
        writeln!(csv, "rank,period,avg_return,avg_sharpe,avg_dd,total_trades,avg_win_rate,pass_rate,windows_positive,total_windows")?;
        for (rank, ar) in agg_results.iter().enumerate() {
            writeln!(csv, "{},{},{:.2},{:.2},{:.2},{},{:.1},{:.1},{},{}",
                rank + 1, ar.period, ar.avg_return, ar.avg_sharpe, ar.avg_dd,
                ar.total_trades, ar.avg_win_rate, ar.pass_rate,
                ar.windows_positive, ar.total_windows)?;
        }
    }

    // Equity curves for charting — pick winner, baseline, and top 3 runner-ups
    {
        let winner_period = agg_results[0].period;
        let runner1 = agg_results.get(1).map(|r| r.period);
        let runner2 = agg_results.get(2).map(|r| r.period);
        let runner3 = agg_results.get(3).map(|r| r.period);

        let periods_to_export: Vec<usize> = vec![
            BASELINE_PERIOD,
            winner_period,
        ].into_iter()
        .chain(runner1.into_iter())
        .chain(runner2.into_iter())
        .chain(runner3.into_iter())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

        // Export combined equity curves (aggregated across all symbols per window)
        let mut csv = File::create(snap_dir.join("bollinger_atr_period_equity.csv"))?;
        writeln!(csv, "bar,period,avg_equity")?;

        // For each period, compute the average equity curve across all symbol-window combos
        for &period in &periods_to_export {
            let curves: Vec<&Vec<f64>> = all_equity_curves.iter()
                .filter(|(p, _, _)| *p == period)
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
                    writeln!(csv, "{},{},{:.2}", bar, period, sum / count as f64)?;
                }
            }
        }
    }

    println!("\n═══ Exports ═══");
    println!("  snapshots/bollinger_atr_period_results.csv");
    println!("  snapshots/bollinger_atr_period_summary.csv");
    println!("  snapshots/bollinger_atr_period_equity.csv");

    // ── Final verdict ────────────────────────────────────────────────────────
    let winner = &agg_results[0];
    let baseline = agg_results.iter().find(|r| r.is_baseline).unwrap();
    let delta_sh = (winner.avg_sharpe - baseline.avg_sharpe) / baseline.avg_sharpe.abs() * 100.0;

    println!("\n═══ VERDICT ═══");
    println!("  Baseline: ATR({}) Sharpe={:.2} Pass={:.0}%", BASELINE_PERIOD, baseline.avg_sharpe, baseline.pass_rate);
    println!("  Winner:   ATR({}) Sharpe={:.2} Pass={:.0}% ({:+.1}%)", winner.period, winner.avg_sharpe, winner.pass_rate, delta_sh);

    if winner.period != BASELINE_PERIOD {
        println!("  → ATR period {} beats baseline {} — candidate for default update", winner.period, BASELINE_PERIOD);
    } else {
        println!("  → Baseline ATR(14) is already optimal — no change needed");
    }

    Ok(())
}

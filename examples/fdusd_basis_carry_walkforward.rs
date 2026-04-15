//! FDUSD/USDT Basis Carry — Walk-Forward Validation
//!
//! PURPOSE: Test whether the FDUSD-USDT perp basis spread is mean-reverting
//! and contains a tradeable edge after realistic costs.
//!
//! This is a GENUINELY MARKET-NEUTRAL strategy:
//! - Long ETHFDUSD perp + Short ETHUSDT perp (or vice versa)
//! - Delta is zero — profit comes from basis convergence, not direction
//! - Orthogonal to every strategy in our current book (all trend-following)
//!
//! Strategy:
//! 1. Compute basis = (FDUSD_close - USDT_close) / USDT_close
//! 2. Track rolling z-score of basis over lookback window
//! 3. When z > entry_threshold: short the premium (short FDUSD, long USDT)
//! 4. When z < -entry_threshold: long the discount (long FDUSD, short USDT)
//! 5. Exit when z reverts toward zero (|z| < exit_threshold)
//!
//! Costs:
//! - FDUSD: 0% maker fee (Binance promo)
//! - USDT: 0.10% taker fee per side
//! - Opening spread: 0.10% (one taker leg)
//! - Closing spread: 0.10% (one taker leg)
//! - Total round-trip: 0.20%
//!
//! Walk-forward: 252/252 train/test, expanding windows

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::path::Path;

const CANDLES: u32 = 5000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const WARMUP: usize = 60;
const TAKER_FEE_USDT: f64 = 0.001; // 10bps taker on USDT perp
const MAKER_FEE_FDUSD: f64 = 0.000; // 0% maker on FDUSD perp
const ROUND_TRIP_COST: f64 = TAKER_FEE_USDT * 2.0; // open + close, one taker leg each

const PAIRS: &[(&str, &str)] = &[
    ("ETHFDUSD", "ETHUSDT"),
    ("BTCFDUSD", "BTCUSDT"),
];

#[derive(Clone, Copy, Debug, Default)]
struct Config {
    lookback: usize,
    entry_z: f64,
    exit_z: f64,
    max_hold: usize,
}

#[derive(Default, Clone, Debug)]
struct WindowResult {
    pair: String,
    window_idx: usize,
    config: Config,
    total_return_pct: f64,
    trades: usize,
    wins: usize,
    max_dd_pct: f64,
    avg_trade_pct: f64,
    sharpe: f64,
}

impl WindowResult {
    fn win_rate(&self) -> f64 {
        if self.trades == 0 { 0.0 } else { self.wins as f64 / self.trades as f64 }
    }
}

fn configs() -> Vec<Config> {
    let mut out = Vec::new();
    for &lookback in &[30usize, 60, 90, 120] {
        for &entry_z in &[1.5, 2.0, 2.5, 3.0] {
            for &exit_z in &[0.3, 0.5, 0.8] {
                for &max_hold in &[6usize, 12, 24, 48] {
                    out.push(Config { lookback, entry_z, exit_z, max_hold });
                }
            }
        }
    }
    out
}

fn run_single_window(
    pair_name: &str,
    basis: &[f64],
    train_start: usize,
    train_end: usize,
    test_start: usize,
    test_end: usize,
    cfg: Config,
) -> WindowResult {
    // Compute z-scores using rolling mean/std from the lookback window
    let test_len = test_end - test_start;
    let mut returns: Vec<f64> = Vec::with_capacity(test_len);
    let mut trade_count = 0usize;
    let mut wins = 0usize;
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut trade_returns: Vec<f64> = Vec::new();

    // Need lookback bars before test_start for z-score computation
    let z_start = if test_start >= cfg.lookback { test_start - cfg.lookback } else { 0 };

    let mut in_position = false;
    let mut position_dir: f64 = 0.0; // +1 = short premium, -1 = long premium
    let mut entry_basis = 0.0_f64;
    let mut hold_bars = 0usize;

    for i in test_start..test_end {
        // Compute rolling z-score of basis
        let window_start = if i >= cfg.lookback { i - cfg.lookback } else { 0 };
        if window_start >= train_start {
            // Use data up to bar i-1 for z-score (no look-ahead)
            let hist_start = window_start;
            let hist_end = i; // exclusive — only data BEFORE current bar

            if hist_end <= hist_start { continue; }

            let mut sum = 0.0_f64;
            let mut count = 0usize;
            for j in hist_start..hist_end {
                sum += basis[j];
                count += 1;
            }
            if count < 10 { continue; }
            let mean = sum / count as f64;

            let mut var_sum = 0.0_f64;
            for j in hist_start..hist_end {
                let d = basis[j] - mean;
                var_sum += d * d;
            }
            let std_dev = (var_sum / count as f64).sqrt();
            if std_dev < 1e-12 { continue; }

            let z = (basis[i] - mean) / std_dev;

            if !in_position {
                if z > cfg.entry_z {
                    // Short the premium: profit if basis decreases
                    in_position = true;
                    position_dir = 1.0; // short premium
                    entry_basis = basis[i];
                    hold_bars = 0;
                } else if z < -cfg.entry_z {
                    // Long the discount: profit if basis increases
                    in_position = true;
                    position_dir = -1.0; // long discount
                    entry_basis = basis[i];
                    hold_bars = 0;
                }
            } else {
                hold_bars += 1;
                let should_exit = if position_dir > 0.0 {
                    z < cfg.exit_z // short premium, exit when z drops
                } else {
                    z > -cfg.exit_z // long discount, exit when z rises
                };

                if should_exit || hold_bars >= cfg.max_hold {
                    // Close position
                    let basis_change = position_dir * (entry_basis - basis[i]);
                    let trade_ret = basis_change - ROUND_TRIP_COST;
                    equity *= 1.0 + trade_ret;
                    trade_returns.push(trade_ret);
                    trade_count += 1;
                    if trade_ret > 0.0 { wins += 1; }

                    // Track drawdown
                    if equity > peak { peak = equity; }
                    let dd = (peak - equity) / peak;
                    if dd > max_dd { max_dd = dd; }

                    in_position = false;
                }
            }
        }
    }

    // Close any open position at end of window
    if in_position && test_end > 0 {
        let last_basis = basis[test_end - 1];
        let basis_change = position_dir * (entry_basis - last_basis);
        let trade_ret = basis_change - ROUND_TRIP_COST;
        equity *= 1.0 + trade_ret;
        trade_returns.push(trade_ret);
        trade_count += 1;
        if trade_ret > 0.0 { wins += 1; }
    }

    let avg_trade = if trade_returns.is_empty() { 0.0 } else {
        trade_returns.iter().sum::<f64>() / trade_returns.len() as f64
    };

    // Sharpe from per-trade returns (annualized assuming ~6 trades per 252-bar window at 4h = 63 days)
    let sharpe = if trade_returns.len() >= 3 {
        let mean_ret = trade_returns.iter().sum::<f64>() / trade_returns.len() as f64;
        let var = trade_returns.iter()
            .map(|r| (r - mean_ret).powi(2))
            .sum::<f64>() / trade_returns.len() as f64;
        let std = var.sqrt();
        if std < 1e-12 { 0.0 } else {
            // Annualize: ~6 windows/year, ~6 trades/window = 36 trades/year
            (mean_ret / std) * (36.0_f64).sqrt()
        }
    } else { 0.0 };

    WindowResult {
        pair: pair_name.to_string(),
        window_idx: 0,
        config: cfg,
        total_return_pct: (equity - 1.0) * 100.0,
        trades: trade_count,
        wins,
        max_dd_pct: max_dd * 100.0,
        avg_trade_pct: avg_trade * 100.0,
        sharpe,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== FDUSD/USDT BASIS CARRY — WALK-FORWARD VALIDATION ===");
    println!("Strategy: Basis z-score mean-reversion (market-neutral)");
    println!("Costs: FDUSD 0% maker, USDT 0.1% taker → 0.20% round-trip");
    println!("Walk-forward: {}/{} train/test\n", TRAIN_BARS, TEST_BARS);

    let all_configs = configs();
    println!("Testing {} parameter configurations across {} pairs\n", all_configs.len(), PAIRS.len());

    let loader = DataLoader::new(None, None);

    // Load and prepare data
    let mut pair_data: Vec<(&str, Vec<f64>)> = Vec::new();

    for &(fdusd_sym, usdt_sym) in PAIRS {
        print!("Loading {} / {} ... ", fdusd_sym, usdt_sym);
        let fdusd_df = loader.fetch_with_cache(fdusd_sym, "4h", CANDLES).await?;
        let usdt_df = loader.fetch_with_cache(usdt_sym, "4h", CANDLES).await?;

        // Join on time
        let joined = usdt_df.lazy()
            .rename(["close"], ["usdt_close"])
            .select([col("time"), col("usdt_close")])
            .join(
                fdusd_df.lazy()
                    .rename(["close"], ["fdusd_close"])
                    .select([col("time"), col("fdusd_close")]),
                [col("time")],
                [col("time")],
                JoinArgs::new(JoinType::Inner),
            )
            .with_columns(vec![
                ((col("fdusd_close") - col("usdt_close")) / col("usdt_close")).alias("basis")
            ])
            .collect()?;

        let basis: Vec<f64> = joined.column("basis")?.f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect();

        let n = basis.len();
        let mean_bps = basis.iter().sum::<f64>() / n as f64 * 10000.0;
        println!("{} bars, avg basis {:.2} bps", n, mean_bps);
        pair_data.push((fdusd_sym, basis));
    }

    // Walk-forward sweep
    println!("\n--- Walk-Forward Sweep ---");

    let mut all_results: Vec<WindowResult> = Vec::new();
    let mut best_configs: Vec<(Config, f64, usize)> = Vec::new(); // (config, avg_ret, total_trades)

    for &cfg in &all_configs {
        let mut total_ret = 0.0_f64;
        let mut total_trades = 0usize;
        let mut total_wins = 0usize;
        let mut total_dd = 0.0_f64;
        let mut windows = 0usize;
        let mut window_results: Vec<WindowResult> = Vec::new();
        let mut positive_windows = 0usize;

        for (pair_name, basis) in &pair_data {
            let n = basis.len();
            if n < TRAIN_BARS + TEST_BARS + WARMUP {
                continue;
            }

            let start = WARMUP;
            let mut win_idx = 0;
            let mut pos = start;
            while pos + TRAIN_BARS + TEST_BARS <= n {
                let train_start = pos;
                let train_end = pos + TRAIN_BARS;
                let test_start = train_end;
                let test_end = test_start + TEST_BARS;

                let wr = run_single_window(
                    pair_name,
                    basis,
                    train_start,
                    train_end,
                    test_start,
                    test_end,
                    cfg,
                );

                let mut wr = wr;
                wr.window_idx = win_idx;
                window_results.push(wr.clone());

                total_ret += wr.total_return_pct;
                total_trades += wr.trades;
                total_wins += wr.wins;
                total_dd += wr.max_dd_pct;
                windows += 1;
                if wr.total_return_pct > 0.0 { positive_windows += 1; }

                win_idx += 1;
                pos += TEST_BARS; // non-overlapping
            }
        }

        if windows > 0 && total_trades >= 10 {
            let avg_ret = total_ret / windows as f64;
            best_configs.push((cfg, avg_ret, total_trades));

            // Keep detailed results for top configs
            all_results.extend(window_results);
        }
    }

    // Sort by average return descending
    best_configs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("\n=== TOP 20 CONFIGS (by avg OOS return per window) ===");
    println!("{:<6} {:<6} {:<6} {:<6} | {:>10} {:>8} {:>8} {:>8} {:>10}",
        "LB", "EntryZ", "ExitZ", "Hold", "AvgRet%", "Trades", "WinRate", "AvgDD%", "Sharpe");
    println!("{}", "-".repeat(80));

    for (i, (cfg, avg_ret, trades)) in best_configs.iter().take(20).enumerate() {
        // Aggregate stats for this config
        let cfg_results: Vec<&WindowResult> = all_results.iter()
            .filter(|r| r.config.lookback == cfg.lookback
                && r.config.entry_z == cfg.entry_z
                && r.config.exit_z == cfg.exit_z
                && r.config.max_hold == cfg.max_hold)
            .collect();

        let n_win = cfg_results.len();
        let pos_win = cfg_results.iter().filter(|r| r.total_return_pct > 0.0).count();
        let total_t: usize = cfg_results.iter().map(|r| r.trades).sum();
        let total_w: usize = cfg_results.iter().map(|r| r.wins).sum();
        let avg_dd = cfg_results.iter().map(|r| r.max_dd_pct).sum::<f64>() / n_win as f64;
        let avg_sharpe = cfg_results.iter().map(|r| r.sharpe).sum::<f64>() / n_win as f64;

        println!("{:<6} {:<6.1} {:<6.1} {:<6} | {:>10.2} {:>8} {:>7.1}% {:>8.2} {:>10.2}{}",
            cfg.lookback, cfg.entry_z, cfg.exit_z, cfg.max_hold,
            avg_ret, total_t,
            if total_t > 0 { total_w as f64 / total_t as f64 * 100.0 } else { 0.0 },
            avg_dd, avg_sharpe,
            if i == 0 { " ← BEST" } else { "" }
        );
    }

    // Detailed analysis of best config
    if let Some((best_cfg, _, _)) = best_configs.first() {
        println!("\n=== DETAILED ANALYSIS: BEST CONFIG ===");
        println!("Lookback={}, EntryZ={}, ExitZ={}, MaxHold={}",
            best_cfg.lookback, best_cfg.entry_z, best_cfg.exit_z, best_cfg.max_hold);

        let best_results: Vec<&WindowResult> = all_results.iter()
            .filter(|r| r.config.lookback == best_cfg.lookback
                && r.config.entry_z == best_cfg.entry_z
                && r.config.exit_z == best_cfg.exit_z
                && r.config.max_hold == best_cfg.max_hold)
            .collect();

        println!("\nPer-window results:");
        println!("{:<10} {:<5} {:>10} {:>8} {:>8} {:>8} {:>8}",
            "Pair", "Win", "Ret%", "Trades", "WinRate", "DD%", "Sharpe");
        println!("{}", "-".repeat(60));

        for r in &best_results {
            println!("{:<10} {:<5} {:>10.2} {:>8} {:>7.1}% {:>8.2} {:>8.2}{}",
                r.pair, r.window_idx, r.total_return_pct, r.trades,
                r.win_rate() * 100.0, r.max_dd_pct, r.sharpe,
                if r.total_return_pct > 0.0 { "" } else { " ✗" }
            );
        }

        let n = best_results.len();
        let pos = best_results.iter().filter(|r| r.total_return_pct > 0.0).count();
        let total_trades: usize = best_results.iter().map(|r| r.trades).sum();
        let avg_sharpe = best_results.iter().map(|r| r.sharpe).sum::<f64>() / n as f64;

        println!("\nSUMMARY:");
        println!("  Pass rate (positive return): {}/{} ({:.0}%)", pos, n, pos as f64 / n as f64 * 100.0);
        println!("  Total trades: {}", total_trades);
        println!("  Average OOS Sharpe: {:.2}", avg_sharpe);

        if pos as f64 / n as f64 >= 0.70 && total_trades >= 30 {
            println!("\n✅ VIABLE — pass rate ≥ 70% and ≥ 30 trades");
        } else if pos as f64 / n as f64 >= 0.60 {
            println!("\n⚠️ BORDERLINE — pass rate ≥ 60% but below 70% threshold");
        } else {
            println!("\n❌ FAILS — pass rate < 60%");
        }
    }

    // Basis statistics
    println!("\n=== BASIS STATISTICS ===");
    for (pair_name, basis) in &pair_data {
        let n = basis.len();
        let mean = basis.iter().sum::<f64>() / n as f64;
        let std = {
            let m = mean;
            (basis.iter().map(|b| (b - m).powi(2)).sum::<f64>() / n as f64).sqrt()
        };
        let min = basis.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = basis.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        println!("{}: mean={:.3} bps, std={:.3} bps, range=[{:.3}, {:.3}] bps",
            pair_name, mean * 10000.0, std * 10000.0, min * 10000.0, max * 10000.0);

        // Autocorrelation of basis (lag 1)
        if n > 1 {
            let m = mean;
            let mut cov = 0.0_f64;
            let mut var = 0.0_f64;
            for i in 1..n {
                cov += (basis[i] - m) * (basis[i-1] - m);
                var += (basis[i] - m).powi(2);
            }
            let ac1 = if var > 0.0 { cov / var } else { 0.0 };
            println!("  Lag-1 autocorrelation: {:.3}{}", ac1,
                if ac1 > 0.5 { " (high — mean-reverting)" } else { "" });
        }
    }

    Ok(())
}

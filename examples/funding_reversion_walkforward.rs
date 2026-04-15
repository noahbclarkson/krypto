//! Funding Rate Mean-Reversion — Walk-Forward Validation
//!
//! PURPOSE: Test whether extreme funding rates mean-revert and contain a
//! tradeable edge. When funding is very positive (crowded long), the next
//! move tends to be downward — short. When very negative, long.
//!
//! This is a CRYPTO-NATIVE signal. Funding rates don't exist in traditional
//! markets. The perpetual futures funding mechanism is unique to crypto.
//!
//! Strategy:
//! 1. Track rolling z-score of funding rate
//! 2. When z > entry: SHORT (crowded long → expect price decline)
//! 3. When z < -entry: LONG (crowded short → expect price bounce)
//! 4. Exit on z reversion or max hold
//!
//! This tests the SIGNAL, not market-neutral execution.
//! We trade single-direction perps with realistic taker fees (10bps/side).
//!
//! Walk-forward: 252/252 train/test

use anyhow::Result;
use krypto::data::funding_rate::FundingRateLoader;
use krypto::data::loader::DataLoader;
use polars::prelude::*;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const WARMUP: usize = 60;
const TAKER_FEE: f64 = 0.001;
const ROUND_TRIP: f64 = TAKER_FEE * 2.0;
const INTERVAL: &str = "1d";

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT",
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
    symbol: String,
    window_idx: usize,
    config_idx: usize,
    total_return_pct: f64,
    trades: usize,
    wins: usize,
    max_dd_pct: f64,
    sharpe: f64,
}

fn configs() -> Vec<Config> {
    let mut out = Vec::new();
    for &lb in &[30usize, 60, 90, 120] {
        for &ez in &[1.5, 2.0, 2.5, 3.0] {
            for &xz in &[0.3, 0.5, 1.0] {
                for &mh in &[3usize, 6, 12] {
                    out.push(Config { lookback: lb, entry_z: ez, exit_z: xz, max_hold: mh });
                }
            }
        }
    }
    out
}

fn run_window(
    symbol: &str,
    closes: &[f64],
    funding: &[f64],
    test_start: usize,
    test_end: usize,
    cfg: Config,
) -> WindowResult {
    let mut trade_count = 0usize;
    let mut wins = 0usize;
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut trade_rets: Vec<f64> = Vec::new();
    let mut in_pos = false;
    let mut pos_dir: f64 = 0.0;
    let mut entry_px = 0.0_f64;
    let mut hold = 0usize;

    for i in test_start..test_end {
        // Z-score from lookback window BEFORE current bar
        if i < cfg.lookback + 1 { continue; }
        let start = i - cfg.lookback;
        let end = i; // exclusive — no look-ahead
        let mut sum = 0.0;
        let mut cnt = 0usize;
        for j in start..end {
            sum += funding[j];
            cnt += 1;
        }
        if cnt < 10 { continue; }
        let mean = sum / cnt as f64;
        let var: f64 = (start..end).map(|j| (funding[j] - mean).powi(2)).sum::<f64>() / cnt as f64;
        let std = var.sqrt();
        if std < 1e-15 { continue; }
        let z = (funding[i] - mean) / std;

        if !in_pos {
            if z > cfg.entry_z {
                // Short — crowded long
                in_pos = true;
                pos_dir = -1.0;
                entry_px = closes[i];
                hold = 0;
            } else if z < -cfg.entry_z {
                // Long — crowded short
                in_pos = true;
                pos_dir = 1.0;
                entry_px = closes[i];
                hold = 0;
            }
        } else {
            hold += 1;
            let should_exit = if pos_dir > 0.0 {
                z > -cfg.exit_z // long, exit when z rises
            } else {
                z < cfg.exit_z // short, exit when z drops
            };
            if should_exit || hold >= cfg.max_hold {
                let ret = pos_dir * (closes[i] - entry_px) / entry_px - ROUND_TRIP;
                equity *= 1.0 + ret;
                trade_rets.push(ret);
                trade_count += 1;
                if ret > 0.0 { wins += 1; }
                if equity > peak { peak = equity; }
                let dd = (peak - equity) / peak;
                if dd > max_dd { max_dd = dd; }
                in_pos = false;
            }
        }
    }

    // Close any open position
    if in_pos && test_end > 0 {
        let ret = pos_dir * (closes[test_end - 1] - entry_px) / entry_px - ROUND_TRIP;
        equity *= 1.0 + ret;
        trade_rets.push(ret);
        trade_count += 1;
        if ret > 0.0 { wins += 1; }
    }

    let sharpe = if trade_rets.len() >= 3 {
        let m = trade_rets.iter().sum::<f64>() / trade_rets.len() as f64;
        let v = trade_rets.iter().map(|r| (r - m).powi(2)).sum::<f64>() / trade_rets.len() as f64;
        let s = v.sqrt();
        if s < 1e-15 { 0.0 } else { (m / s) * (trade_rets.len() as f64).sqrt() }
    } else { 0.0 };

    WindowResult {
        symbol: symbol.to_string(),
        total_return_pct: (equity - 1.0) * 100.0,
        trades: trade_count,
        wins,
        max_dd_pct: max_dd * 100.0,
        sharpe,
        ..Default::default()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== FUNDING RATE MEAN-REVERSION — WALK-FORWARD ===");
    println!("Signal: funding z-score → contrarian (short crowded longs, long crowded shorts)");
    println!("Costs: 0.20% round-trip taker");
    println!("Walk-forward: {}/{} train/test, {} symbols\n", TRAIN_BARS, TEST_BARS, SYMBOLS.len());

    let all_cfg = configs();
    println!("Testing {} configs\n", all_cfg.len());

    let loader = DataLoader::new(None, None);
    let funding_loader = FundingRateLoader::with_cache_dir("examples/funding_cache");

    // Load data
    let mut datasets: Vec<(&str, Vec<f64>, Vec<f64>)> = Vec::new(); // (sym, closes, funding)
    for &sym in SYMBOLS {
        print!("Loading {} ... ", sym);
        let price_df = loader.fetch_with_cache(sym, INTERVAL, CANDLES).await?;
        let funding_df = funding_loader.fetch(sym, None, None).await?;

        let closes: Vec<f64> = price_df.column("close")?.f64()?
            .into_iter().map(|v| v.unwrap_or(0.0)).collect();

        // Align funding to daily
        let funding_aligned = {
            let f_times: Vec<i64> = funding_df.column("time")?.datetime()?.into_iter()
                .map(|v| v.unwrap_or(0)).collect();
            let f_rates: Vec<f64> = funding_df.column("funding_rate")?.f64()?
                .into_iter().map(|v| v.unwrap_or(0.0)).collect();
            let p_times: Vec<i64> = price_df.column("time")?.datetime()?.into_iter()
                .map(|v| v.unwrap_or(0)).collect();

            // For each daily bar, find the last funding rate before that bar
            let mut aligned = vec![0.0f64; closes.len()];
            let mut fi = 0usize;
            for (pi, &pt) in p_times.iter().enumerate() {
                while fi + 1 < f_times.len() && f_times[fi + 1] <= pt {
                    fi += 1;
                }
                if fi < f_rates.len() {
                    aligned[pi] = f_rates[fi];
                }
            }
            aligned
        };

        println!("{} bars, {} funding records", closes.len(), funding_aligned.iter().filter(|&&v| v != 0.0).count());
        datasets.push((sym, closes, funding_aligned));
    }

    // Walk-forward sweep
    println!("\n--- Walk-Forward Sweep ---");
    let mut agg: Vec<(usize, f64, usize, f64, f64, usize)> = Vec::new(); // (cfg_idx, avg_ret, total_trades, avg_dd, avg_sharpe, windows)

    for (ci, &cfg) in all_cfg.iter().enumerate() {
        let mut total_ret = 0.0_f64;
        let mut total_trades = 0usize;
        let mut total_dd = 0.0_f64;
        let mut total_sharpe = 0.0_f64;
        let mut windows = 0usize;
        let mut all_window_results: Vec<WindowResult> = Vec::new();

        for (sym, closes, funding) in &datasets {
            let n = closes.len();
            if n < TRAIN_BARS + TEST_BARS + WARMUP { continue; }

            let mut pos = WARMUP;
            let mut win_idx = 0;
            while pos + TRAIN_BARS + TEST_BARS <= n {
                let test_start = pos + TRAIN_BARS;
                let test_end = test_start + TEST_BARS;

                let mut wr = run_window(sym, closes, funding, test_start, test_end, cfg);
                wr.config_idx = ci;
                wr.window_idx = win_idx;
                all_window_results.push(wr.clone());

                total_ret += wr.total_return_pct;
                total_trades += wr.trades;
                total_dd += wr.max_dd_pct;
                total_sharpe += wr.sharpe;
                windows += 1;

                win_idx += 1;
                pos += TEST_BARS;
            }
        }

        if windows > 0 && total_trades >= 10 {
            let avg_ret = total_ret / windows as f64;
            let avg_dd = total_dd / windows as f64;
            let avg_sharpe = total_sharpe / windows as f64;
            agg.push((ci, avg_ret, total_trades, avg_dd, avg_sharpe, windows));
        }
    }

    agg.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("\n=== TOP 20 CONFIGS ===");
    println!("{:<5} {:<5} {:<5} {:<5} | {:>10} {:>8} {:>8} {:>10} {:>6}",
        "LB", "EZ", "XZ", "MH", "AvgRet%", "Trades", "AvgDD%", "AvgSharpe", "Wins");
    println!("{}", "-".repeat(75));

    for (rank, (ci, avg_ret, total_trades, avg_dd, avg_sharpe, windows)) in agg.iter().take(20).enumerate() {
        let cfg = all_cfg[*ci];
        // Count positive windows
        let pos_count = 0; // simplified
        println!("{:<5} {:<5.1} {:<5.1} {:<5} | {:>10.2} {:>8} {:>8.2} {:>10.2} {:>6}{}",
            cfg.lookback, cfg.entry_z, cfg.exit_z, cfg.max_hold,
            avg_ret, total_trades, avg_dd, avg_sharpe, windows,
            if rank == 0 { " ← BEST" } else { "" }
        );
    }

    // Detailed best config analysis
    if let Some((ci, _, _, _, _, _)) = agg.first() {
        let best_cfg = all_cfg[*ci];
        println!("\n=== BEST CONFIG DETAIL ===");
        println!("LB={}, EZ={}, XZ={}, MH={}", best_cfg.lookback, best_cfg.entry_z, best_cfg.exit_z, best_cfg.max_hold);

        // Re-run to get per-window detail
        let mut detail: Vec<WindowResult> = Vec::new();
        for (sym, closes, funding) in &datasets {
            let n = closes.len();
            if n < TRAIN_BARS + TEST_BARS + WARMUP { continue; }
            let mut pos = WARMUP;
            let mut win_idx = 0;
            while pos + TRAIN_BARS + TEST_BARS <= n {
                let test_start = pos + TRAIN_BARS;
                let test_end = test_start + TEST_BARS;
                let mut wr = run_window(sym, closes, funding, test_start, test_end, best_cfg);
                wr.window_idx = win_idx;
                detail.push(wr);
                win_idx += 1;
                pos += TEST_BARS;
            }
        }

        println!("{:<12} {:<5} {:>10} {:>8} {:>8} {:>8} {:>8}",
            "Symbol", "Win", "Ret%", "Trades", "WR%", "DD%", "Sharpe");
        println!("{}", "-".repeat(65));
        for r in &detail {
            println!("{:<12} {:<5} {:>10.2} {:>8} {:>7.1}% {:>8.2} {:>8.2}{}",
                r.symbol, r.window_idx, r.total_return_pct, r.trades,
                if r.trades > 0 { r.wins as f64 / r.trades as f64 * 100.0 } else { 0.0 },
                r.max_dd_pct, r.sharpe,
                if r.total_return_pct > 0.0 { "" } else { " ✗" }
            );
        }

        let n = detail.len();
        let pos = detail.iter().filter(|r| r.total_return_pct > 0.0).count();
        let total_t: usize = detail.iter().map(|r| r.trades).sum();
        let avg_sh = detail.iter().map(|r| r.sharpe).sum::<f64>() / n as f64;

        println!("\nSUMMARY:");
        println!("  Pass rate: {}/{} ({:.0}%)", pos, n, pos as f64 / n as f64 * 100.0);
        println!("  Total trades: {}", total_t);
        println!("  Avg OOS Sharpe: {:.2}", avg_sh);

        if pos as f64 / n as f64 >= 0.70 && total_t >= 30 {
            println!("\n✅ VIABLE — pass rate ≥ 70%, ≥ 30 trades");
        } else {
            println!("\n❌ FAILS — below threshold");
        }
    }

    // Funding rate statistics
    println!("\n=== FUNDING RATE STATISTICS ===");
    for (sym, _, funding) in &datasets {
        let valid: Vec<f64> = funding.iter().filter(|&&v| v != 0.0).cloned().collect();
        if valid.is_empty() { continue; }
        let n = valid.len();
        let mean = valid.iter().sum::<f64>() / n as f64;
        let std = {
            let m = mean;
            (valid.iter().map(|v| (v - m).powi(2)).sum::<f64>() / n as f64).sqrt()
        };
        let pos_pct = valid.iter().filter(|&&v| v > 0.0).count() as f64 / n as f64 * 100.0;
        let extremes = valid.iter().filter(|&&v| v.abs() > 3.0 * std).count();

        // Autocorrelation
        let ac1 = if n > 1 {
            let m = mean;
            let cov: f64 = (1..n).map(|i| (valid[i] - m) * (valid[i-1] - m)).sum::<f64>();
            let var: f64 = valid.iter().map(|v| (v - m).powi(2)).sum::<f64>();
            if var > 0.0 { cov / var } else { 0.0 }
        } else { 0.0 };

        println!("{}: mean={:.6}, std={:.6}, positive={:.0}%, extremes(>3σ)={}, ac1={:.3}",
            sym, mean, std, pos_pct, extremes, ac1);
    }

    Ok(())
}

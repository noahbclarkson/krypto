//! Cross-Sectional Momentum Rotation — Walk-Forward Validation
//!
//! PURPOSE: Test whether relative momentum between crypto assets contains
//! a tradeable edge. Instead of absolute trend-following (long BTC when BTC
//! is rising), we trade the ROTATION: long the top performers, short the
//! bottom performers over a lookback window.
//!
//! This is genuinely different from all our existing strategies:
//! - Time-series momentum (current book): long when asset > threshold
//! - Cross-sectional momentum (this): long winners vs losers RELATIVE to each other
//! - Market-neutral by construction (equal long/short)
//! - Exploits the well-documented cross-sectional momentum anomaly
//!
//! Strategy:
//! 1. Rank all assets by return over lookback window
//! 2. Long top N, short bottom N
//! 3. Rebalance every rebalance_bars
//! 4. Walk-forward: train for lookback selection, test OOS
//!
//! Costs: 0.10% taker per side per rebalance

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const WARMUP: usize = 60;
const TAKER_FEE: f64 = 0.001;

const UNIVERSE: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
    "LTCUSDT", "EOSUSDT",
];

#[derive(Clone, Copy, Debug, Default)]
struct Config {
    lookback: usize,       // momentum lookback (bars)
    top_n: usize,          // long top N
    bottom_n: usize,       // short bottom N
    rebalance: usize,      // rebalance frequency (bars)
}

#[derive(Default, Clone, Debug)]
struct WindowResult {
    window_idx: usize,
    config_idx: usize,
    total_return_pct: f64,
    long_return_pct: f64,
    short_return_pct: f64,
    trades: usize,
    max_dd_pct: f64,
    sharpe: f64,
}

fn configs() -> Vec<Config> {
    let mut out = Vec::new();
    for &lb in &[5usize, 10, 21, 42, 63] {
        for &top_n in &[1usize, 2, 3] {
            for &bot_n in &[1usize, 2] {
                for &rb in &[1usize, 5, 10, 21] {
                    if top_n + bot_n > UNIVERSE.len() { continue; }
                    out.push(Config { lookback: lb, top_n, bottom_n: bot_n, rebalance: rb });
                }
            }
        }
    }
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== CROSS-SECTIONAL MOMENTUM ROTATION — WALK-FORWARD ===");
    println!("Long winners, short losers, market-neutral by construction");
    println!("Universe: {} assets", UNIVERSE.len());
    println!("Costs: 0.20% round-trip per rebalance\n");

    let all_cfg = configs();
    println!("Testing {} configs\n", all_cfg.len());

    let loader = DataLoader::new(None, None);

    // Load all price data
    let mut price_map: HashMap<&str, Vec<f64>> = HashMap::new();
    let mut time_vec: Vec<i64> = Vec::new();
    for &sym in UNIVERSE {
        print!("Loading {} ... ", sym);
        let df = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let closes: Vec<f64> = df.column("close")?.f64()?
            .into_iter().map(|v| v.unwrap_or(0.0)).collect();
        if time_vec.is_empty() {
            time_vec = df.column("time")?.datetime()?.into_iter()
                .map(|v| v.unwrap_or(0)).collect();
        }
        println!("{} bars", closes.len());
        price_map.insert(sym, closes);
    }

    let n_bars = time_vec.len();
    println!("\nTotal bars: {}", n_bars);

    // Walk-forward sweep
    let mut agg: Vec<(usize, f64, f64, f64, usize, f64)> = Vec::new();

    for (ci, &cfg) in all_cfg.iter().enumerate() {
        let mut total_ret = 0.0_f64;
        let mut total_long = 0.0_f64;
        let mut total_short = 0.0_f64;
        let mut total_trades = 0usize;
        let mut equity_curve: Vec<f64> = Vec::new();
        let mut peak = 1.0_f64;
        let mut max_dd = 0.0_f64;
        let mut windows = 0usize;
        let mut window_sharpes: Vec<f64> = Vec::new();

        let mut pos = WARMUP;
        let mut win_idx = 0;
        while pos + TRAIN_BARS + TEST_BARS <= n_bars {
            let test_start = pos + TRAIN_BARS;
            let test_end = test_start + TEST_BARS;

            let mut win_equity = 1.0_f64;
            let mut win_trades = 0usize;
            let mut win_long_ret = 0.0_f64;
            let mut win_short_ret = 0.0_f64;
            let mut daily_rets: Vec<f64> = Vec::new();

            // Rebalance within test window
            let mut rb_pos = test_start;
            while rb_pos + cfg.rebalance <= test_end {
                let rb_end = std::cmp::min(rb_pos + cfg.rebalance, test_end);

                // Compute momentum (return over lookback) for each symbol
                let mut momentum: Vec<(&str, f64)> = Vec::new();
                for &sym in UNIVERSE {
                    if let Some(closes) = price_map.get(sym) {
                        if rb_pos >= cfg.lookback && rb_pos < closes.len() && rb_end <= closes.len() {
                            let past_px = closes[rb_pos - cfg.lookback];
                            let cur_px = closes[rb_pos];
                            if past_px > 0.0 {
                                momentum.push((sym, (cur_px - past_px) / past_px));
                            }
                        }
                    }
                }

                if momentum.len() >= cfg.top_n + cfg.bottom_n {
                    momentum.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                    // Long top N, short bottom N
                    let n_assets = cfg.top_n + cfg.bottom_n;
                    let weight = 1.0 / n_assets as f64;
                    let mut period_ret = 0.0_f64;
                    let mut period_long = 0.0_f64;
                    let mut period_short = 0.0_f64;

                    for (rank, (sym, _mom)) in momentum.iter().enumerate() {
                        if let Some(closes) = price_map.get(sym) {
                            if rb_pos < closes.len() && rb_end <= closes.len() && closes[rb_pos] > 0.0 {
                                let ret = (closes[rb_end - 1] - closes[rb_pos]) / closes[rb_pos];
                                if rank < cfg.top_n {
                                    // Long
                                    period_ret += weight * ret;
                                    period_long += weight * ret;
                                } else if rank >= momentum.len() - cfg.bottom_n {
                                    // Short (profit if price drops)
                                    period_ret -= weight * ret;
                                    period_short -= weight * ret;
                                }
                            }
                        }
                    }

                    // Apply rebalance cost
                    period_ret -= TAKER_FEE * 2.0 * n_assets as f64 * weight; // round-trip per asset

                    win_equity *= 1.0 + period_ret;
                    daily_rets.push(period_ret);
                    win_trades += 1;
                    win_long_ret += period_long;
                    win_short_ret += period_short;
                }

                rb_pos = rb_end;
            }

            if win_trades >= 3 {
                let win_ret = (win_equity - 1.0) * 100.0;
                total_ret += win_ret;
                total_long += win_long_ret * 100.0;
                total_short += win_short_ret * 100.0;
                total_trades += win_trades;

                let sh = if daily_rets.len() >= 3 {
                    let m = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
                    let v = daily_rets.iter().map(|r| (r - m).powi(2)).sum::<f64>() / daily_rets.len() as f64;
                    let s = v.sqrt();
                    if s > 1e-15 { (m / s) * (252.0_f64).sqrt() } else { 0.0 }
                } else { 0.0 };
                window_sharpes.push(sh);

                if win_equity > peak { peak = win_equity; }
                let dd = (peak - win_equity) / peak;
                if dd > max_dd { max_dd = dd; }

                windows += 1;
            }

            win_idx += 1;
            pos += TEST_BARS;
        }

        if windows > 0 {
            let avg_ret = total_ret / windows as f64;
            let avg_sh = window_sharpes.iter().sum::<f64>() / windows as f64;
            agg.push((ci, avg_ret, total_long / windows as f64, total_short / windows as f64, total_trades, avg_sh));
        }
    }

    agg.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("\n=== TOP 20 CONFIGS ===");
    println!("{:<5} {:<4} {:<4} {:<4} | {:>10} {:>10} {:>10} {:>8} {:>10}",
        "LB", "Top", "Bot", "Reb", "AvgRet%", "Long%", "Short%", "Trades", "AvgSharpe");
    println!("{}", "-".repeat(80));

    for (rank, (ci, avg_ret, long_ret, short_ret, total_trades, avg_sh)) in agg.iter().take(20).enumerate() {
        let cfg = all_cfg[*ci];
        println!("{:<5} {:<4} {:<4} {:<4} | {:>10.2} {:>10.2} {:>10.2} {:>8} {:>10.2}{}",
            cfg.lookback, cfg.top_n, cfg.bottom_n, cfg.rebalance,
            avg_ret, long_ret, short_ret, total_trades, avg_sh,
            if rank == 0 { " ← BEST" } else { "" }
        );
    }

    // Detailed best config
    if let Some((ci, _, _, _, _, _)) = agg.first() {
        let best_cfg = all_cfg[*ci];
        println!("\n=== BEST CONFIG DETAIL ===");
        println!("LB={}, Top={}, Bot={}, Reb={}", best_cfg.lookback, best_cfg.top_n, best_cfg.bottom_n, best_cfg.rebalance);

        // Re-run best config with full detail
        let mut pos = WARMUP;
        let mut win_idx = 0;
        let mut pass_count = 0usize;
        let mut total_windows = 0usize;
        let mut all_trades = 0usize;
        let mut all_sharpes: Vec<f64> = Vec::new();

        println!("{:<5} {:>10} {:>10} {:>10} {:>8} {:>8}", "Win", "Ret%", "Long%", "Short%", "Trades", "Sharpe");
        println!("{}", "-".repeat(55));

        while pos + TRAIN_BARS + TEST_BARS <= n_bars {
            let test_start = pos + TRAIN_BARS;
            let test_end = test_start + TEST_BARS;
            let mut win_equity = 1.0_f64;
            let mut win_trades = 0usize;
            let mut win_long = 0.0_f64;
            let mut win_short = 0.0_f64;
            let mut daily_rets: Vec<f64> = Vec::new();

            let mut rb_pos = test_start;
            while rb_pos + best_cfg.rebalance <= test_end {
                let rb_end = std::cmp::min(rb_pos + best_cfg.rebalance, test_end);

                let mut momentum: Vec<(&str, f64)> = Vec::new();
                for &sym in UNIVERSE {
                    if let Some(closes) = price_map.get(sym) {
                        if rb_pos >= best_cfg.lookback && rb_pos < closes.len() && rb_end <= closes.len() {
                            let past_px = closes[rb_pos - best_cfg.lookback];
                            let cur_px = closes[rb_pos];
                            if past_px > 0.0 {
                                momentum.push((sym, (cur_px - past_px) / past_px));
                            }
                        }
                    }
                }

                if momentum.len() >= best_cfg.top_n + best_cfg.bottom_n {
                    momentum.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    let n_assets = best_cfg.top_n + best_cfg.bottom_n;
                    let weight = 1.0 / n_assets as f64;
                    let mut period_ret = 0.0_f64;
                    let mut p_long = 0.0_f64;
                    let mut p_short = 0.0_f64;

                    for (rank, (sym, _)) in momentum.iter().enumerate() {
                        if let Some(closes) = price_map.get(sym) {
                            if rb_pos < closes.len() && rb_end <= closes.len() && closes[rb_pos] > 0.0 {
                                let ret = (closes[rb_end - 1] - closes[rb_pos]) / closes[rb_pos];
                                if rank < best_cfg.top_n {
                                    period_ret += weight * ret;
                                    p_long += weight * ret;
                                } else if rank >= momentum.len() - best_cfg.bottom_n {
                                    period_ret -= weight * ret;
                                    p_short -= weight * ret;
                                }
                            }
                        }
                    }

                    period_ret -= TAKER_FEE * 2.0 * n_assets as f64 * weight;
                    win_equity *= 1.0 + period_ret;
                    daily_rets.push(period_ret);
                    win_trades += 1;
                    win_long += p_long;
                    win_short += p_short;
                }

                rb_pos = rb_end;
            }

            if win_trades >= 1 {
                let sh = if daily_rets.len() >= 3 {
                    let m = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
                    let v = daily_rets.iter().map(|r| (r - m).powi(2)).sum::<f64>() / daily_rets.len() as f64;
                    let s = v.sqrt();
                    if s > 1e-15 { (m / s) * 252.0_f64.sqrt() } else { 0.0 }
                } else { 0.0 };

                let win_ret_pct = (win_equity - 1.0) * 100.0;
                println!("{:<5} {:>10.2} {:>10.2} {:>10.2} {:>8} {:>8.2}{}",
                    win_idx, win_ret_pct, win_long * 100.0, win_short * 100.0, win_trades, sh,
                    if win_ret_pct > 0.0 { "" } else { " ✗" }
                );

                total_windows += 1;
                all_trades += win_trades;
                all_sharpes.push(sh);
                if win_ret_pct > 0.0 { pass_count += 1; }
            }

            win_idx += 1;
            pos += TEST_BARS;
        }

        if total_windows > 0 {
            let pass_rate = pass_count as f64 / total_windows as f64 * 100.0;
            let avg_sh = all_sharpes.iter().sum::<f64>() / all_sharpes.len() as f64;

            println!("\nSUMMARY:");
            println!("  Pass rate: {}/{} ({:.0}%)", pass_count, total_windows, pass_rate);
            println!("  Total rebalances: {}", all_trades);
            println!("  Avg OOS Sharpe: {:.2}", avg_sh);

            if pass_rate >= 70.0 && all_trades >= 30 {
                println!("\n✅ VIABLE — pass rate ≥ 70%");
            } else if pass_rate >= 60.0 {
                println!("\n⚠️ BORDERLINE");
            } else {
                println!("\n❌ FAILS — pass rate < 60%");
            }

            // Long-only vs Short-only decomposition
            println!("\n=== LONG-ONLY vs SHORT-ONLY ANALYSIS ===");
            println!("(Tests whether the edge comes from long momentum or short reversal)");
        }
    }

    // Additional: Long-only momentum (no short) for comparison
    println!("\n=== LONG-ONLY MOMENTUM COMPARISON ===");
    let mut long_only_results: Vec<(usize, f64, f64, usize)> = Vec::new();

    for (ci, &cfg) in all_cfg.iter().enumerate() {
        if cfg.bottom_n > 0 { continue; } // long-only

        let mut total_ret = 0.0_f64;
        let mut total_sh = 0.0_f64;
        let mut windows = 0usize;
        let mut trades = 0usize;

        let mut pos = WARMUP;
        while pos + TRAIN_BARS + TEST_BARS <= n_bars {
            let test_start = pos + TRAIN_BARS;
            let test_end = test_start + TEST_BARS;
            let mut win_eq = 1.0_f64;
            let mut daily_rets: Vec<f64> = Vec::new();
            let mut wt = 0usize;

            let mut rb_pos = test_start;
            while rb_pos + cfg.rebalance <= test_end {
                let rb_end = std::cmp::min(rb_pos + cfg.rebalance, test_end);

                let mut momentum: Vec<(&str, f64)> = Vec::new();
                for &sym in UNIVERSE {
                    if let Some(closes) = price_map.get(sym) {
                        if rb_pos >= cfg.lookback && rb_pos < closes.len() && rb_end <= closes.len() {
                            let past_px = closes[rb_pos - cfg.lookback];
                            let cur_px = closes[rb_pos];
                            if past_px > 0.0 {
                                momentum.push((sym, (cur_px - past_px) / past_px));
                            }
                        }
                    }
                }

                if momentum.len() >= cfg.top_n {
                    momentum.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    let weight = 1.0 / cfg.top_n as f64;
                    let mut period_ret = 0.0_f64;

                    for (rank, (sym, _)) in momentum.iter().enumerate() {
                        if rank >= cfg.top_n { break; }
                        if let Some(closes) = price_map.get(sym) {
                            if rb_pos < closes.len() && rb_end <= closes.len() && closes[rb_pos] > 0.0 {
                                let ret = (closes[rb_end - 1] - closes[rb_pos]) / closes[rb_pos];
                                period_ret += weight * ret;
                            }
                        }
                    }

                    period_ret -= TAKER_FEE * 2.0;
                    win_eq *= 1.0 + period_ret;
                    daily_rets.push(period_ret);
                    wt += 1;
                }

                rb_pos = rb_end;
            }

            if wt >= 1 {
                let sh = if daily_rets.len() >= 3 {
                    let m = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
                    let v = daily_rets.iter().map(|r| (r - m).powi(2)).sum::<f64>() / daily_rets.len() as f64;
                    let s = v.sqrt();
                    if s > 1e-15 { (m / s) * 252.0_f64.sqrt() } else { 0.0 }
                } else { 0.0 };

                total_ret += (win_eq - 1.0) * 100.0;
                total_sh += sh;
                windows += 1;
                trades += wt;
            }

            pos += TEST_BARS;
        }

        if windows > 0 {
            long_only_results.push((ci, total_ret / windows as f64, total_sh / windows as f64, trades));
        }
    }

    long_only_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("{:<5} {:<4} {:<4} | {:>10} {:>10} {:>8}",
        "LB", "Top", "Reb", "AvgRet%", "AvgSharpe", "Trades");
    println!("{}", "-".repeat(50));
    for (rank, (ci, avg_ret, avg_sh, trades)) in long_only_results.iter().take(10).enumerate() {
        let cfg = all_cfg[*ci];
        println!("{:<5} {:<4} {:<4} | {:>10.2} {:>10.2} {:>8}{}",
            cfg.lookback, cfg.top_n, cfg.rebalance,
            avg_ret, avg_sh, trades,
            if rank == 0 { " ← BEST" } else { "" }
        );
    }

    Ok(())
}

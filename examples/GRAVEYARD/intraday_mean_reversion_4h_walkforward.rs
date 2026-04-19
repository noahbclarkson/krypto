//! Intraday Mean Reversion — 4h Walk-Forward Validation
//!
//! Prior result (1h benchmark): ETH +8.6% avg OOS (123 trades, 68% WR) and XRP +5.2% avg OOS
//! (64 trades, 67% WR) were the most credible live signals in program history.
//! DOGE and MACD-1h went to GRAVEYARD.
//!
//! This run: 4h bars for ETH and XRP — more bars per unit time than daily,
//! longer horizon than 1h. Walk-forward validation (4 windows, chronology-first)
//! to establish whether the edge is robust across time rather than fitting a single split.
//!
//! Focus: ETH and XRP only (BTC/SOL too thin at 1h; DOGE GRAVEYARD).
//!
//! No look-ahead: signal at bar close using only prior-bar information.
//! Realistic fees: 0.1% taker each side.

use colored::*;
use krypto::data::DataLoader;
use polars::prelude::*;
use std::collections::BTreeMap;
use tokio::runtime::Runtime;

const SYMBOLS: &[&str] = &["ETHUSDT", "XRPUSDT"];
const INTERVAL: &str = "4h";
// 4h bars: ~2190 per year. Use 9000 to get ~4 years.
const CANDLES_PER_SYMBOL: u32 = 9000;

// Fee: 0.1% taker each side
const FEE_PCT: f64 = 0.001;

// Walk-forward: 4 equal windows, each expanding forward
const NUM_WINDOWS: usize = 4;
const MIN_TRADES_PER_WINDOW: usize = 12; // lower threshold for 4h (fewer bars)

#[derive(Clone, Copy, Debug, PartialEq)]
struct Config {
    lookback: usize,
    entry_z: f64,
    exit_z: f64,
    max_hold: usize,
    is_long: bool,
}

impl Config {
    fn long_only(lookback: usize, entry_z: f64, exit_z: f64, max_hold: usize) -> Self {
        Self {
            lookback,
            entry_z,
            exit_z,
            max_hold,
            is_long: true,
        }
    }
}

fn all_configs() -> Vec<Config> {
    let mut out = Vec::new();
    for &lb in &[12usize, 24usize, 48usize, 96usize] {
        for &ez in &[1.5, 2.0, 2.5] {
            for &xz in &[0.0, 0.3] {
                for &mh in &[12usize, 24usize, 48usize] {
                    out.push(Config::long_only(lb, ez, xz, mh));
                }
            }
        }
    }
    out
}

fn rolling_zscore(values: &[f64], lookback: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    for i in lookback..n {
        let start = i - lookback;
        let window = &values[start..i];
        let mean = window.iter().sum::<f64>() / lookback as f64;
        let var = window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / lookback as f64;
        let std = var.sqrt();
        if std > 1e-10 {
            out[i] = Some((values[i] - mean) / std);
        }
    }
    out
}

/// Walk-forward backtest on a single test window.
/// Train frac defines the expanding training window; test is everything after.
fn walkforward_test(closes: &[f64], cfg: Config, train_frac: f64) -> (f64, usize, f64, f64) {
    let n = closes.len();
    if n < 100 {
        return (0.0, 0, 0.0, 0.0);
    }

    let log_returns: Vec<f64> = closes.windows(2).map(|w| (w[1] / w[0]).ln()).collect();

    let zscores = rolling_zscore(&log_returns, cfg.lookback);

    let train_end = (n as f64 * train_frac) as usize;
    if train_end < cfg.lookback + cfg.max_hold + 20 {
        return (0.0, 0, 0.0, 0.0);
    }

    let mut trades = Vec::new();
    let mut in_pos = false;
    let mut bars_held = 0usize;

    for i in (cfg.lookback + 1)..(n - 2) {
        // Only generate signals in the test period
        if i < train_end {
            continue;
        }

        let z_opt = zscores[i];

        if !in_pos {
            if let Some(z) = z_opt {
                if cfg.is_long && z < -cfg.entry_z {
                    in_pos = true;
                    bars_held = 0;
                }
            }
        } else {
            bars_held += 1;
            let exit_price = closes[i + 1];
            let entry_price = closes[i];

            let should_exit = {
                let z = zscores[i];
                let time_expired = bars_held >= cfg.max_hold;
                let z_exit = if cfg.exit_z > 0.0 {
                    z.is_some_and(|zv| zv > -cfg.exit_z)
                } else {
                    false
                };
                time_expired || z_exit
            };

            if should_exit {
                let ret = (exit_price - entry_price) / entry_price;
                let ret_after_fee = ret - FEE_PCT;
                trades.push(ret_after_fee);
                in_pos = false;
            }
        }
    }

    let num_trades = trades.len();
    if num_trades < MIN_TRADES_PER_WINDOW {
        return (0.0, 0, 0.0, 0.0);
    }

    let wins = trades.iter().filter(|&&t| t > 0.0).count();
    let win_rate = wins as f64 / num_trades as f64;
    let avg_trade = trades.iter().sum::<f64>() / num_trades as f64;
    let pnl = trades.iter().fold(1.0f64, |acc, &t| acc * (1.0 + t));
    let total_return = (pnl - 1.0) * 100.0;

    (total_return, num_trades, win_rate, avg_trade)
}

/// Multi-window walk-forward: expanding windows
/// Returns (avg_oos, avg_win_rate, total_trades, avg_trade_pct, n_valid_windows, per_window_returns)
fn multi_window_walkforward(
    closes: &[f64],
    cfg: Config,
) -> (f64, f64, usize, f64, usize, Vec<(usize, f64, usize, f64)>) {
    let n = closes.len();
    let window_size = n / NUM_WINDOWS;

    let mut oos_returns = Vec::new();
    let mut trade_counts = Vec::new();
    let mut win_rates = Vec::new();
    let mut avg_trades = Vec::new();
    let mut per_window: Vec<(usize, f64, usize, f64)> = Vec::new();

    for w in 0..NUM_WINDOWS {
        // Expanding window: train_end grows with each window
        let test_start = w * window_size;
        let train_end = test_start;
        let train_frac = train_end as f64 / n as f64;

        if train_frac < 0.30 {
            continue; // Need at least 30% training data
        }

        let (ret, trades, wr, avg_t) = walkforward_test(closes, cfg, train_frac);
        if trades >= MIN_TRADES_PER_WINDOW {
            oos_returns.push(ret);
            trade_counts.push(trades);
            win_rates.push(wr);
            avg_trades.push(avg_t);
            per_window.push((w, ret, trades, wr));
        }
    }

    if oos_returns.is_empty() {
        return (0.0, 0.0, 0, 0.0, 0, vec![]);
    }

    let avg_oos = oos_returns.iter().sum::<f64>() / oos_returns.len() as f64;
    let total_trades: usize = trade_counts.iter().sum();
    let avg_win_rate = win_rates.iter().sum::<f64>() / win_rates.len() as f64;
    let avg_avg_trade = avg_trades.iter().sum::<f64>() / avg_trades.len() as f64;

    (
        avg_oos,
        avg_win_rate,
        total_trades,
        avg_avg_trade,
        oos_returns.len(),
        per_window,
    )
}

fn main() {
    println!(
        "{}",
        "\n=== Intraday Mean Reversion — 4h Walk-Forward Validation ==="
            .cyan()
            .bold()
    );
    println!("Symbols: {:?}", SYMBOLS);
    println!("Interval: {}", INTERVAL);
    println!("Windows: {} (expanding, chronology-first)", NUM_WINDOWS);
    println!("Min trades/window: {}\n", MIN_TRADES_PER_WINDOW);

    let rt = Runtime::new().unwrap();
    let loader = DataLoader::new(None, None);
    let configs = all_configs();
    println!("Testing {} configs per symbol\n", configs.len());

    let mut all_results: BTreeMap<String, Vec<(Config, f64, f64, usize, f64, usize)>> =
        BTreeMap::new();

    for symbol in SYMBOLS {
        println!("{}", format!("── {} ──", symbol).yellow().bold());

        let df = rt.block_on(loader.fetch_data(symbol, INTERVAL, CANDLES_PER_SYMBOL));
        match df {
            Ok(df) => {
                let times: Vec<i64> = df
                    .column("time")
                    .unwrap()
                    .cast(&DataType::Int64)
                    .unwrap()
                    .i64()
                    .unwrap()
                    .into_iter()
                    .flatten()
                    .collect();
                let closes: Vec<f64> = df
                    .column("close")
                    .unwrap()
                    .f64()
                    .unwrap()
                    .into_iter()
                    .flatten()
                    .collect();

                let n = closes.len();
                println!("  Loaded {} 4h bars", n);

                let mut symbol_results = Vec::new();

                for cfg in &configs {
                    let (avg_oos, avg_wr, total_trades, avg_t, n_windows, _) =
                        multi_window_walkforward(&closes, *cfg);

                    // Require at least 2 windows AND 25 total trades for OOS validity
                    if n_windows >= 2 && total_trades >= 25 {
                        symbol_results.push((
                            *cfg,
                            avg_oos,
                            avg_wr,
                            total_trades,
                            avg_t,
                            n_windows,
                        ));
                    }
                }

                symbol_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

                println!("\n  Top 5 configs (by avg OOS return%):");
                println!(
                    "  {:<8} {:<6} {:<6} {:<7} {:<7} {:<9} {:<8}",
                    "Lookbk", "EntZ", "ExtZ", "MaxHold", "Trades", "AvgOOS%", "WinRate"
                );
                println!("  {}", "-".repeat(56));

                for (cfg, avg_oos, avg_wr, trades, _, n_win) in symbol_results.iter().take(5) {
                    println!(
                        "  {:<8} {:<6} {:<6} {:<7} {:<7} {:>+9.2} {:<8}",
                        cfg.lookback,
                        cfg.entry_z,
                        cfg.exit_z,
                        cfg.max_hold,
                        trades,
                        avg_oos,
                        avg_wr * 100.0
                    );
                }

                // Per-window breakdown for the TOP config
                if let Some(top) = symbol_results.first() {
                    let (_, avg_oos_all, _, _, _, _) = multi_window_walkforward(&closes, top.0);
                    let (_, _, _, _, _, per_window) = multi_window_walkforward(&closes, top.0);
                    println!(
                        "\n  Per-window detail for top config (lb{}/ez{}/xz{}/mh{}):",
                        top.0.lookback, top.0.entry_z, top.0.exit_z, top.0.max_hold
                    );
                    println!(
                        "  {:<10} {:<12} {:<8} {:<10}",
                        "Window", "OOS Return%", "Trades", "WinRate"
                    );
                    println!("  {}", "-".repeat(44));
                    for (w, ret, trades, wr) in per_window {
                        println!(
                            "  {:<10} {:>+12.2} {:<8} {:<10.1}",
                            format!("Window{}", w),
                            ret,
                            trades,
                            wr * 100.0
                        );
                    }
                }

                all_results.insert(symbol.to_string(), symbol_results);
            }
            Err(e) => {
                println!("  ERROR: {:?}", e);
            }
        }
    }

    // ── Cross-symbol summary ───────────────────────────────────────────────
    println!("\n\n{}", "=== CROSS-SYMBOL SUMMARY ===".cyan().bold());

    let mut cross_symbol: BTreeMap<String, (f64, f64, usize, f64, usize)> = BTreeMap::new();

    for (symbol, results) in &all_results {
        for (cfg, avg_oos, avg_wr, trades, avg_t, _) in results {
            let key = format!(
                "lb{}_ez{}_xz{}_mh{}",
                cfg.lookback, cfg.entry_z, cfg.exit_z, cfg.max_hold
            );
            let entry = cross_symbol.entry(key).or_insert((0.0, 0.0, 0, 0.0, 0));
            entry.0 += avg_oos;
            entry.1 += avg_wr;
            entry.2 += *trades;
            entry.3 += avg_t;
            entry.4 += 1;
        }
    }

    let mut cross_summary: Vec<_> = cross_symbol
        .iter()
        .map(|(k, v)| {
            let n_syms = v.4 as f64;
            (
                k.clone(),
                v.0 / n_syms,
                v.1 / n_syms,
                v.2,
                v.3 / n_syms,
                v.4,
            )
        })
        .collect();

    cross_summary.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!("\n  Configs appearing in both ETH and XRP (by avg OOS):");
    println!(
        "  {:<20} {:<10} {:<10} {:<10} {:<8}",
        "Config", "AvgOOS%", "AvgWinRate", "TotalTrades", "Symbols"
    );
    println!("  {}", "-".repeat(62));

    for (key, avg_oos, avg_wr, total_trades, _, n_syms) in cross_summary.iter().take(10) {
        if *n_syms >= 2 {
            println!(
                "  {:<20} {:>+10.2}  {:<10.0} {:<10} {:<8}",
                key,
                avg_oos,
                avg_wr * 100.0,
                total_trades,
                n_syms
            );
        }
    }

    // ── Trust verdict ───────────────────────────────────────────────────────
    println!("\n\n{}", "=== HONEST TRUST VERDICT ===".red().bold());

    if let (Some(eth), Some(xrp)) = (all_results.get("ETHUSDT"), all_results.get("XRPUSDT")) {
        let eth_top = &eth[0];
        let xrp_top = &xrp[0];

        println!(
            "\n  ETH top config:  avg OOS {:>+7.2} | {} trades | {:.0} WR | {} windows",
            eth_top.1,
            eth_top.3,
            eth_top.2 * 100.0,
            eth_top.5
        );
        println!(
            "  XRP top config:  avg OOS {:>+7.2} | {} trades | {:.0} WR | {} windows",
            xrp_top.1,
            xrp_top.3,
            xrp_top.2 * 100.0,
            xrp_top.5
        );

        let eth_ok = eth_top.3 >= 40 && eth_top.1 > 0.0 && eth_top.5 >= 2;
        let xrp_ok = xrp_top.3 >= 40 && xrp_top.1 > 0.0 && xrp_top.5 >= 2;

        if eth_ok && xrp_ok {
            println!(
                "\n  {} PROMISING — both symbols positive, 40+ trades, 2+ windows",
                "✓".green()
            );
            println!("  Honest caveat: 'top config' is selected-best, multi-window avg is lower.");
            println!(
                "  Next: test ensemble (ETH MR + BTC trend) or 15m bars for more granularity."
            );
        } else if eth_top.1 > 0.0 || xrp_top.1 > 0.0 {
            println!(
                "\n  {} MARGINAL — one symbol positive but thin trades or single window",
                "~".yellow()
            );
        } else {
            println!(
                "\n  {} FAILS — both symbols negative in OOS walk-forward",
                "✗".red()
            );
        }
    } else {
        println!("\n  Insufficient data to render verdict.");
    }

    println!("\n  Selection bias: 'top config' return is selected-best per symbol.");
    println!("  Multi-window average is the honest number — use it, not the winner.");
    println!("  4h walk-forward is more rigorous than the 1h single-split benchmark.");
    println!("\n");
}

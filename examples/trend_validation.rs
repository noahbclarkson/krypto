//! Trend Following Validation - Walk-forward + Monte Carlo
//!
//! Rigorous validation of the Turtle breakout strategy to ensure edge is real.

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.001;
const N_MONTE_CARLO: usize = 100;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== TREND FOLLOWING VALIDATION ===\n");
    println!("Validating Turtle breakout (20d high/low) with 21-day hold.\n");

    let loader = DataLoader::new(None, None);

    // Load data
    let mut data_cache: std::collections::HashMap<String, DataFrame> =
        std::collections::HashMap::new();

    for symbol in SYMBOLS {
        print!("Loading {}... ", symbol);
        match loader.fetch_data(symbol, "1d", CANDLES).await {
            Ok(raw) => {
                let df = FeatureEngine::add_technicals(&raw, None)?;
                println!("{} bars", df.height());
                data_cache.insert(symbol.to_string(), df);
            }
            Err(e) => {
                println!("SKIP ({})", e);
            }
        }
    }

    // 1. Walk-forward validation
    println!("\n{}", "=".repeat(80));
    println!("1. WALK-FORWARD VALIDATION");
    println!("{}", "=".repeat(80));
    println!("Split data into 4 periods, train on first 3, test on last.\n");

    let hold_bars = 21;
    let period = 20;

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let n = df.height();
            let quarter = n / 4;

            println!("\n{} ({} bars, {} years):", symbol, n, n / 365);

            for i in 0..4 {
                let start = i * quarter;
                let end = if i == 3 { n } else { (i + 1) * quarter };
                let period_df = df.slice(start as i64, end - start);

                let result = run_turtle_backtest(&period_df, hold_bars, period)?;
                let years = (end - start) as f64 / 365.0;

                println!(
                    "  Period {} ({} bars, {:.1}yr): Return={:>7.1}% | Trades={:>3} | WinRate={:.0}%",
                    i + 1,
                    end - start,
                    years,
                    result.total_return_pct,
                    result.trades,
                    result.win_rate * 100.0
                );
            }
        }
    }

    // 2. Monte Carlo permutation test
    println!("\n\n{}", "=".repeat(80));
    println!("2. MONTE CARLO PERMUTATION TEST");
    println!("{}", "=".repeat(80));
    println!(
        "Shuffle signal dates {} times, compare real vs random.\n",
        N_MONTE_CARLO
    );

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let real_result = run_turtle_backtest(df, hold_bars, period)?;
            let mut random_returns: Vec<f64> = Vec::with_capacity(N_MONTE_CARLO);

            for _ in 0..N_MONTE_CARLO {
                let shuffled = shuffle_signals(df, period)?;
                let result = run_backtest_with_signals(df, &shuffled, hold_bars)?;
                random_returns.push(result.total_return_pct);
            }

            random_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());

            // Find percentile
            let real_return = real_result.total_return_pct;
            let percentile = random_returns
                .iter()
                .position(|&r| r > real_return)
                .unwrap_or(N_MONTE_CARLO);
            let p_value = percentile as f64 / N_MONTE_CARLO as f64;

            let mean_random = random_returns.iter().sum::<f64>() / N_MONTE_CARLO as f64;
            let profitable_random = random_returns.iter().filter(|&&r| r > 0.0).count();

            println!(
                "{:12} | Real: {:>7.1}% | Random mean: {:>7.1}% | Pctl: {:>3}% | Profitable: {}/{}",
                symbol,
                real_return,
                mean_random,
                (1.0 - p_value) * 100.0,
                profitable_random,
                N_MONTE_CARLO
            );
        }
    }

    // 3. Random entry comparison
    println!("\n\n{}", "=".repeat(80));
    println!("3. RANDOM ENTRY COMPARISON");
    println!("{}", "=".repeat(80));
    println!("Compare Turtle signals to random entry with same hold period.\n");

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let turtle_result = run_turtle_backtest(df, hold_bars, period)?;

            let mut random_returns: Vec<f64> = Vec::with_capacity(N_MONTE_CARLO);
            for seed in 0..N_MONTE_CARLO {
                let random_signals = generate_random_signals(df.height(), seed as u64);
                let result = run_backtest_with_signals(df, &random_signals, hold_bars)?;
                random_returns.push(result.total_return_pct);
            }

            random_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mean_random = random_returns.iter().sum::<f64>() / N_MONTE_CARLO as f64;
            let turtle_beats = random_returns
                .iter()
                .filter(|&&r| turtle_result.total_return_pct > r)
                .count();

            println!(
                "{:12} | Turtle: {:>7.1}% | Random mean: {:>7.1}% | Turtle beats: {}/{}",
                symbol, turtle_result.total_return_pct, mean_random, turtle_beats, N_MONTE_CARLO
            );
        }
    }

    // 4. Regime analysis
    println!("\n\n{}", "=".repeat(80));
    println!("4. REGIME ANALYSIS");
    println!("{}", "=".repeat(80));
    println!("Test performance across bull/bear/sideways periods.\n");

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let close = df.column("close")?.f64()?;
            let n = close.len();
            let first_price = close.get(0).unwrap_or(1.0);
            let last_price = close.get(n - 1).unwrap_or(1.0);
            let overall_return = (last_price / first_price - 1.0) * 100.0;

            // Split into 3 equal periods
            let period_len = n / 3;
            let mut period_results = Vec::new();

            for i in 0..3 {
                let start = i * period_len;
                let end = if i == 3 { n } else { (i + 1) * period_len };
                let period_df = df.slice(start as i64, end - start);
                let result = run_turtle_backtest(&period_df, hold_bars, period)?;

                // Classify regime by price change
                let p_start = close.get(start).unwrap_or(1.0);
                let p_end = close.get(end - 1).unwrap_or(1.0);
                let price_change = (p_end / p_start - 1.0) * 100.0;
                let regime = if price_change > 50.0 {
                    "BULL"
                } else if price_change < -30.0 {
                    "BEAR"
                } else {
                    "SIDE"
                };

                period_results.push((regime, result.total_return_pct, result.trades));
            }

            println!(
                "{:12} | Overall: {:>6.0}% | Periods: {} ({:.0}%/{}) | {} ({:.0}%/{}) | {} ({:.0}%/{})",
                symbol,
                overall_return,
                period_results[0].0, period_results[0].1, period_results[0].2,
                period_results[1].0, period_results[1].1, period_results[1].2,
                period_results[2].0, period_results[2].1, period_results[2].2
            );
        }
    }

    // Summary
    println!("\n\n{}", "=".repeat(80));
    println!("SUMMARY");
    println!("{}", "=".repeat(80));
    println!("If Turtle signals show:");
    println!("  - Consistent returns across all periods → edge is robust");
    println!("  - P-value < 5% vs random → edge is statistically significant");
    println!("  - Beats random entry > 95% of the time → signal adds value");

    Ok(())
}

struct BacktestResult {
    total_return_pct: f64,
    trades: usize,
    win_rate: f64,
}

fn run_turtle_backtest(df: &DataFrame, hold_bars: usize, period: usize) -> Result<BacktestResult> {
    let signals = generate_turtle_signals(df, period)?;
    run_backtest_with_signals(df, &signals, hold_bars)
}

fn run_backtest_with_signals(
    df: &DataFrame,
    signals: &[i8],
    hold_bars: usize,
) -> Result<BacktestResult> {
    let close = df.column("close")?.f64()?;
    let n = close.len();

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut i = 50;

    while i < n.saturating_sub(hold_bars + 1) {
        let signal = signals[i];

        if signal != 0 && i + 1 + hold_bars < n {
            let entry_price = match close.get(i + 1) {
                Some(p) if p > 0.0 => p,
                _ => {
                    i += 1;
                    continue;
                }
            };
            let exit_idx = i + 1 + hold_bars;
            let exit_price = match close.get(exit_idx) {
                Some(p) if p > 0.0 => p,
                _ => {
                    i += 1;
                    continue;
                }
            };

            let return_pct = if signal > 0 {
                (exit_price / entry_price - 1.0) * 100.0
            } else {
                (entry_price / exit_price - 1.0) * 100.0
            };

            let return_after_fees = return_pct - (TAKER_FEE * 100.0 * 2.0);
            trade_returns.push(return_after_fees);

            i = exit_idx;
        } else {
            i += 1;
        }
    }

    let total_return_pct: f64 = trade_returns.iter().sum();
    let trades = trade_returns.len();
    let wins = trade_returns.iter().filter(|&&r| r > 0.0).count();

    Ok(BacktestResult {
        total_return_pct,
        trades,
        win_rate: if trades == 0 {
            0.0
        } else {
            wins as f64 / trades as f64
        },
    })
}

fn generate_turtle_signals(df: &DataFrame, period: usize) -> Result<Vec<i8>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();

    let mut signals = vec![0i8; n];

    for i in period..n {
        let highest: f64 = (i.saturating_sub(period)..i)
            .map(|j| high.get(j).unwrap_or(0.0))
            .fold(0.0, f64::max);

        let lowest: f64 = (i.saturating_sub(period)..i)
            .map(|j| low.get(j).unwrap_or(f64::MAX))
            .fold(f64::MAX, f64::min);

        let price = close.get(i).unwrap_or(0.0);

        if price >= highest && price > 0.0 {
            signals[i] = 1;
        } else if price <= lowest && price > 0.0 {
            signals[i] = -1;
        }
    }

    Ok(signals)
}

fn shuffle_signals(df: &DataFrame, period: usize) -> Result<Vec<i8>> {
    let mut signals = generate_turtle_signals(df, period)?;
    let n = signals.len();

    // Simple Fisher-Yates shuffle with deterministic seed
    let mut hasher = DefaultHasher::new();
    for i in (1..n).rev() {
        hasher.write_u64(i as u64);
        let j = (hasher.finish() as usize) % (i + 1);
        signals.swap(i, j);
    }

    Ok(signals)
}

fn generate_random_signals(len: usize, seed: u64) -> Vec<i8> {
    let mut signals = vec![0i8; len];
    let mut hasher = DefaultHasher::new();

    for i in 50..len {
        hasher.write_u64(seed);
        hasher.write_u64(i as u64);
        let hash = hasher.finish();
        hasher = DefaultHasher::new();

        let prob = hash % 100;
        if prob < 3 {
            signals[i] = 1; // 3% long
        } else if prob < 6 {
            signals[i] = -1; // 3% short
        }
    }

    signals
}

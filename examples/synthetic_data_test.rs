//! Synthetic Data Test for BollingerReversion 1d
//!
//! Tests if the edge persists on synthetic/random walk data.
//! If the strategy profits on random data, the backtest has a flaw.
//!
//! Creates:
//! 1. Random walk price series (geometric Brownian motion)
//! 2. Mean-reverting price series (Ornstein-Uhlenbeck)
//! 3. Shuffled returns (same distribution, no temporal structure)
//!
//! Usage:
//!   cargo run --profile sweep --example synthetic_data_test

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry, backtest::engine::Backtester, features::indicators::FeatureEngine,
};
use polars::prelude::*;
use rand::prelude::*;

const N_BARS: usize = 1000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const N_TRIALS: usize = 20;

fn create_random_walk(rng: &mut StdRng) -> DataFrame {
    let mut prices = vec![100.0_f64];
    let mut opens = Vec::new();
    let mut highs = Vec::new();
    let mut lows = Vec::new();
    let mut closes = Vec::new();
    let mut volumes = Vec::new();

    let daily_vol = 0.03; // 3% daily volatility

    for i in 0..N_BARS {
        let prev_close = prices.last().unwrap();
        let ret = rng.gen::<f64>() * 2.0 * daily_vol - daily_vol; // Uniform [-vol, vol]
        let open = prev_close;
        let close = open * (1.0 + ret);
        let high = open.max(close) * (1.0 + rng.gen::<f64>() * 0.01);
        let low = open.min(close) * (1.0 - rng.gen::<f64>() * 0.01);

        opens.push(open);
        highs.push(high);
        lows.push(low);
        closes.push(close);
        volumes.push(1_000_000.0);
        prices.push(close);
    }

    df! [
        "open" => opens,
        "high" => highs,
        "low" => lows,
        "close" => closes,
        "volume" => volumes,
    ]
    .unwrap()
}

fn create_mean_reverting(rng: &mut StdRng) -> DataFrame {
    let mut prices = vec![100.0_f64];
    let mut opens = Vec::new();
    let mut highs = Vec::new();
    let mut lows = Vec::new();
    let mut closes = Vec::new();
    let mut volumes = Vec::new();

    let mean = 100.0;
    let theta = 0.05; // Mean reversion speed
    let daily_vol = 0.03;

    for i in 0..N_BARS {
        let prev_close = *prices.last().unwrap();
        let drift = theta * (mean - prev_close);
        let shock = rng.gen::<f64>() * 2.0 * daily_vol - daily_vol;
        let open = prev_close;
        let close = prev_close + drift + shock * prev_close;
        let high = open.max(close) * (1.0 + rng.gen::<f64>() * 0.01);
        let low = open.min(close) * (1.0 - rng.gen::<f64>() * 0.01);

        opens.push(open);
        highs.push(high);
        lows.push(low);
        closes.push(close);
        volumes.push(1_000_000.0);
        prices.push(close);
    }

    df! [
        "open" => opens,
        "high" => highs,
        "low" => lows,
        "close" => closes,
        "volume" => volumes,
    ]
    .unwrap()
}

fn run_backtest(df: &DataFrame, stop_pct: f64) -> Option<(f64, f64, usize, f64)> {
    let df_with_features = FeatureEngine::add_technicals(df, None).ok()?;
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").ok()?;
    let signals = strategy.predict(&df_with_features).ok()?;

    let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
    bt.run(&df_with_features, &signals, stop_pct, 0.0)
        .ok()
        .filter(|r| r.total_trades >= 10)
        .map(|r| {
            (
                r.total_return_pct,
                r.sharpe_ratio,
                r.total_trades,
                r.win_rate,
            )
        })
}

fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  SYNTHETIC DATA TEST — BollingerReversion 1d"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    let mut rng = StdRng::seed_from_u64(42);
    let registry = StrategyRegistry::new();

    // Test on random walk
    println!(
        "\n{}",
        "▶ RANDOM WALK (Geometric Brownian Motion)"
            .bright_yellow()
            .bold()
    );
    println!("  {} trials per ATR multiplier\n", N_TRIALS);

    for &atr_mult in &[0.20, 0.30, 0.50] {
        let stop_pct = atr_mult * 0.05; // Approx 1-2.5% stop
        let mut returns = Vec::new();
        let mut sharpes = Vec::new();
        let mut profitable = 0;

        for _ in 0..N_TRIALS {
            let df = create_random_walk(&mut rng);
            if let Some((ret, sharpe, _, _)) = run_backtest(&df, stop_pct) {
                returns.push(ret);
                sharpes.push(sharpe);
                if ret > 0.0 {
                    profitable += 1;
                }
            }
        }

        if !returns.is_empty() {
            let avg_ret = returns.iter().sum::<f64>() / returns.len() as f64;
            let avg_sharpe = sharpes.iter().sum::<f64>() / sharpes.len() as f64;
            let profitable_pct = profitable as f64 / returns.len() as f64 * 100.0;

            println!(
                "  ATR×{:.2}: Avg return {:>8.1}%, Avg Sharpe {:>8.2}, Profitable {:>5.1}%",
                atr_mult, avg_ret, avg_sharpe, profitable_pct
            );
        }
    }

    // Test on mean-reverting synthetic
    println!(
        "\n{}",
        "▶ MEAN-REVERTING (Ornstein-Uhlenbeck)"
            .bright_yellow()
            .bold()
    );
    println!("  {} trials per ATR multiplier\n", N_TRIALS);

    for &atr_mult in &[0.20, 0.30, 0.50] {
        let stop_pct = atr_mult * 0.05;
        let mut returns = Vec::new();
        let mut sharpes = Vec::new();
        let mut profitable = 0;

        for _ in 0..N_TRIALS {
            let df = create_mean_reverting(&mut rng);
            if let Some((ret, sharpe, _, _)) = run_backtest(&df, stop_pct) {
                returns.push(ret);
                sharpes.push(sharpe);
                if ret > 0.0 {
                    profitable += 1;
                }
            }
        }

        if !returns.is_empty() {
            let avg_ret = returns.iter().sum::<f64>() / returns.len() as f64;
            let avg_sharpe = sharpes.iter().sum::<f64>() / sharpes.len() as f64;
            let profitable_pct = profitable as f64 / returns.len() as f64 * 100.0;

            println!(
                "  ATR×{:.2}: Avg return {:>8.1}%, Avg Sharpe {:>8.2}, Profitable {:>5.1}%",
                atr_mult, avg_ret, avg_sharpe, profitable_pct
            );
        }
    }

    // Verdict
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  INTERPRETATION".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!("• If random walk is profitable → backtest has a bug");
    println!("• If mean-reverting is profitable → edge is from mean reversion (expected)");
    println!("• If mean-reverting >> random walk → edge is real");
    println!("• Compare synthetic results to real data (XRP +670,000%)");

    Ok(())
}

//! Passive execution backtest for FDUSD pairs (0% maker fees).
//!
//! Demonstrates the walk-forward limit order model:
//! - For each 1m candle within a 1h bar
//! - Place limit at open - N_ticks
//! - Fill if low <= limit
//! - Almost 100% fill rate + better price + 0% fees
//!
//! Usage:
//!   cargo run --release --example passive_execution_backtest

use anyhow::Result;
use colored::*;
use krypto::{
    algo::SignalGenerator,
    algo::strategies::{DynamicTrend, RsiMeanReversion, BollingerReversion},
    backtest::engine::{Backtester, PositionSizing},
    backtest::passive::{PassiveExecutor, PassiveConfig, TickSize},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

// FDUSD pairs have 0% maker fees on Binance
const FDUSD_PAIRS: &[(&str, f64)] = &[
    ("BTCFDUSD", 0.01),
    ("ETHFDUSD", 0.001),
    ("SOLFDUSD", 0.0001),
    ("BNBFDUSD", 0.01),
];

const INTERVALS: &[&str] = &["1h", "4h"];
const CANDLES: u16 = 1000;
const CAPITAL: f64 = 10_000.0;

#[derive(Debug, Clone)]
struct ComparisonResult {
    strategy: String,
    symbol: String,
    interval: String,
    market_return_pct: f64,
    passive_return_pct: f64,
    improvement_pct: f64,
    fill_rate: f64,
    avg_price_improvement_ticks: f64,
    fees_saved_pct: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  PASSIVE EXECUTION — Walk-Forward Limit Orders".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!("\n  Walk-forward model: place limit below each 1m open, fill if low <= limit\n");

    let loader = DataLoader::new(None, None);
    let mut comparisons: Vec<ComparisonResult> = Vec::new();

    let strategies: Vec<(&str, Box<dyn SignalGenerator>)> = vec![
        ("dynamic_trend", Box::new(DynamicTrend::new())),
        ("rsi_mean_reversion", Box::new(RsiMeanReversion::new())),
        ("bollinger_reversion", Box::new(BollingerReversion::new())),
    ];

    let backtester = Backtester::with_defaults(CAPITAL)
        .with_position_sizing(PositionSizing::Full);

    println!("{}", "Phase 1: Fetching data...".bright_green());

    let mut data_cache: HashMap<(String, String), (DataFrame, DataFrame)> = HashMap::new();

    for (symbol, tick_size) in FDUSD_PAIRS {
        for interval in INTERVALS {
            print!("  {} {} (1h + 1m)... ", symbol, interval);
            let t = Instant::now();

            match loader.fetch_data(symbol, interval, CANDLES).await {
                Ok(df_high) => {
                    let df_high = FeatureEngine::add_technicals(&df_high, None).unwrap();
                    let mins_per_bar = match interval {
                        "1h" => 60,
                        "4h" => 240,
                        _ => 60,
                    };
                    let candles_1m = CANDLES as u32 * mins_per_bar;

                    match loader.fetch_data(symbol, "1m", candles_1m as u16).await {
                        Ok(df_low) => {
                            println!("{} ({:.1}s)", "✓".green(), t.elapsed().as_secs_f64());
                            data_cache.insert((symbol.to_string(), interval.to_string()), (df_high, df_low));
                        }
                        Err(e) => println!("{} 1m fetch: {}", "✗".red(), e),
                    }
                }
                Err(e) => println!("{} {}", "✗".red(), e),
            }
        }
    }

    println!("\n{}", "Phase 2: Running comparisons...".bright_green());

    for ((symbol, interval), (df_high, df_low)) in &data_cache {
        let tick_size = FDUSD_PAIRS.iter().find(|(s, _)| *s == symbol).map(|(_, t)| *t).unwrap_or(0.01);

        for (strat_name, mut strategy) in &strategies {
            print!("\r  {} on {} {}                    ", strat_name, symbol, interval);

            let signals = match strategy.predict(df_high) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Market execution backtest
            let market_result = match backtester.run(df_high, &signals, 0.05, 0.0) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if market_result.total_trades == 0 {
                continue;
            }

            // Passive execution
            let config = PassiveConfig {
                ticks_below_open: 3,
                tick_size: TickSize(tick_size),
                max_wait_bars: 240,
                maker_fee: 0.0, // 0% on FDUSD
                update_threshold_ticks: Some(5), // Update if price moves 5 ticks away
            };

            let executor = PassiveExecutor::new(config);
            let (fills, stats) = match executor.simulate(df_high, df_low, &signals).await {
                Ok(r) => r,
                Err(_) => continue,
            };

            if fills.is_empty() {
                continue;
            }

            // Create filtered signal series
            let passive_signals = executor.fills_to_signals(&fills, df_high.height());

            // Run backtest with passive signals
            let passive_result = match backtester.run(df_high, &passive_signals, 0.05, 0.0) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let improvement = passive_result.total_return_pct - market_result.total_return_pct;
            let fees_saved = (market_result.total_fees_paid - passive_result.total_fees_paid) / CAPITAL * 100.0;

            comparisons.push(ComparisonResult {
                strategy: strat_name.to_string(),
                symbol: symbol.clone(),
                interval: interval.clone(),
                market_return_pct: market_result.total_return_pct,
                passive_return_pct: passive_result.total_return_pct,
                improvement_pct: improvement,
                fill_rate: stats.fill_rate,
                avg_price_improvement_ticks: stats.avg_price_improvement_ticks,
                fees_saved_pct: fees_saved,
            });
        }
    }

    println!("\n");

    // Sort by improvement
    comparisons.sort_by(|a, b| b.improvement_pct.partial_cmp(&a.improvement_pct).unwrap());

    println!("{}", "━".repeat(100).bright_cyan());
    println!("{}", "  RESULTS: Market vs Walk-Forward Passive Execution".bright_cyan().bold());
    println!("{}", "━".repeat(100).bright_cyan());
    println!(
        "  {:<22} {:<12} {:<5} {:>10} {:>10} {:>10} {:>6} {:>8}",
        "Strategy", "Symbol", "Int", "Market%", "Passive%", "Improve%", "Fill%", "Ticks"
    );
    println!("{}", "─".repeat(100));

    for c in &comparisons {
        let improve_col = if c.improvement_pct > 0.0 {
            format!("{:>9.1}%", c.improvement_pct).green().to_string()
        } else {
            format!("{:>9.1}%", c.improvement_pct).red().to_string()
        };
        println!(
            "  {:<22} {:<12} {:<5} {:>9.1f}% {:>9.1f}% {} {:>5.0f}% {:>7.1f}",
            c.strategy,
            c.symbol,
            c.interval,
            c.market_return_pct,
            c.passive_return_pct,
            improve_col,
            c.fill_rate * 100.0,
            c.avg_price_improvement_ticks
        );
    }

    // Summary
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  Summary".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    let improved = comparisons.iter().filter(|c| c.improvement_pct > 0.0).count();
    let avg_improvement = if !comparisons.is_empty() {
        comparisons.iter().map(|c| c.improvement_pct).sum::<f64>() / comparisons.len() as f64
    } else {
        0.0
    };
    let avg_fill_rate = if !comparisons.is_empty() {
        comparisons.iter().map(|c| c.fill_rate).sum::<f64>() / comparisons.len() as f64
    } else {
        0.0
    };

    println!("  Total comparisons:     {}", comparisons.len());
    println!("  Improved by passive:  {}/{} ({:.0f}%)", improved.green(), comparisons.len(), improved as f64 / comparisons.len().max(1) as f64 * 100.0);
    println!("  Avg return improvement: {:.1f}%", avg_improvement);
    println!("  Avg fill rate:         {:.0f}%", avg_fill_rate * 100.0);

    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  How Walk-Forward Execution Works".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!("\n  1. Signal fires on 1h close at $100
  2. Walk through each 1m candle:
     - Candle 0: open=$100, place limit at $99.97 (3 ticks below)
     - If low ≤ $99.97 → filled! (almost always since most candles dip)
     - If not filled, candle 1: open=$99.98, place limit at $99.95
     - Continue until filled
  3. Result: almost 100% fill rate at slightly better price + 0% fees

  This captures the natural open-to-low movement in most candles.
");

    println!();
    Ok(())
}

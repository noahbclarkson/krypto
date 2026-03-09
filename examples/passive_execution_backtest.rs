//! Passive execution backtest for FDUSD pairs (0% maker fees).
//!
//! Compares market execution vs passive limit order execution.
//! Shows fill rates, price improvements, and fee savings.
//!
//! Usage:
//!   cargo run --release --example passive_execution_backtest

use anyhow::Result;
use colored::*;
use krypto::{
    algo::SignalGenerator,
    algo::strategies::{DynamicTrend, RsiMeanReversion, BollingerReversion, MacdTrend},
    backtest::engine::{Backtester, BacktestResult, PositionSizing},
    backtest::passive::{PassiveExecutor, PassiveConfig, PassiveFillStats},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

// FDUSD pairs have 0% maker fees on Binance
const FDUSD_PAIRS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD"];

// Intervals where passive execution works well (30m+)
const INTERVALS: &[&str] = &["1h", "4h"];

const CANDLES: u16 = 2000;
const CAPITAL: f64 = 10_000.0;
const TRAILING_STOP: f64 = 0.05;
const TAKE_PROFIT: f64 = 0.0;

#[derive(Debug, Clone)]
struct ComparisonResult {
    strategy: String,
    symbol: String,
    interval: String,
    market_return_pct: f64,
    passive_return_pct: f64,
    improvement_pct: f64,
    fill_rate: f64,
    avg_price_improvement_bps: f64,
    fees_saved: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  PASSIVE EXECUTION BACKTEST (FDUSD 0% Maker Fee)".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!("\n  Comparing market vs passive limit order execution on FDUSD pairs.\n");

    let loader = DataLoader::new(None, None);
    let mut comparisons: Vec<ComparisonResult> = Vec::new();

    // Strategies to test
    let strategies: Vec<(&str, Box<dyn SignalGenerator>)> = vec![
        ("dynamic_trend", Box::new(DynamicTrend::new())),
        ("rsi_mean_reversion", Box::new(RsiMeanReversion::new())),
        ("bollinger_reversion", Box::new(BollingerReversion::new())),
        ("macd_trend", Box::new(MacdTrend::new())),
    ];

    let backtester = Backtester::with_defaults(CAPITAL)
        .with_position_sizing(PositionSizing::Full);

    let passive_config = PassiveConfig {
        tick_offset_bps: 5.0,           // 5 bps below market
        max_wait_bars: 1,               // Fill within current bar
        force_market_on_timeout: false, // Skip if no fill
        maker_fee: 0.0,                 // FDUSD = 0% maker
        taker_fee: 0.001,               // 0.1% taker
    };
    let executor = PassiveExecutor::new(passive_config);

    println!("{}", "Phase 1: Fetching data...".bright_green());

    // Cache all data first
    let mut data_cache: HashMap<(String, String), (DataFrame, DataFrame)> = HashMap::new();

    for symbol in FDUSD_PAIRS {
        for interval in INTERVALS {
            print!("  {} {} (1h + 1m)... ", symbol, interval);
            let t = Instant::now();

            // Fetch higher timeframe
            match loader.fetch_data(symbol, interval, CANDLES).await {
                Ok(df_high) => {
                    let df_high = FeatureEngine::add_technicals(&df_high, None).unwrap();

                    // Calculate how many 1m candles we need
                    let mins_per_bar = match interval {
                        "1h" => 60,
                        "4h" => 240,
                        _ => 60,
                    };
                    let candles_1m = CANDLES as u32 * mins_per_bar;

                    // Fetch 1m data
                    match loader.fetch_data(symbol, "1m", candles_1m as u16).await {
                        Ok(df_low) => {
                            println!("{} ({:.1}s)", "✓".green(), t.elapsed().as_secs_f64());
                            data_cache.insert(
                                (symbol.to_string(), interval.to_string()),
                                (df_high, df_low),
                            );
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
        for (strat_name, mut strategy) in &strategies {
            print!("\r  {} on {} {}                    ", strat_name, symbol, interval);

            // Generate signals
            let signals = match strategy.predict(df_high) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Run market execution backtest
            let market_result = match backtester.run(df_high, &signals, TRAILING_STOP, TAKE_PROFIT) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if market_result.total_trades == 0 {
                continue;
            }

            // Simulate passive execution
            let (fills, stats) = match executor.simulate(df_high, df_low, &signals).await {
                Ok(r) => r,
                Err(_) => continue,
            };

            if fills.is_empty() {
                continue;
            }

            // Create filtered signal series with only filled signals
            let passive_signals = executor.fills_to_signals(&fills, df_high.height());

            // Run backtest with passive signals
            let passive_result = match backtester.run(df_high, &passive_signals, TRAILING_STOP, TAKE_PROFIT) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let improvement = passive_result.total_return_pct - market_result.total_return_pct;

            comparisons.push(ComparisonResult {
                strategy: strat_name.to_string(),
                symbol: symbol.clone(),
                interval: interval.clone(),
                market_return_pct: market_result.total_return_pct,
                passive_return_pct: passive_result.total_return_pct,
                improvement_pct: improvement,
                fill_rate: stats.fill_rate,
                avg_price_improvement_bps: stats.avg_price_improvement_bps,
                fees_saved: stats.total_fees_saved,
            });
        }
    }

    println!("\n");

    // Sort by improvement
    comparisons.sort_by(|a, b| b.improvement_pct.partial_cmp(&a.improvement_pct).unwrap());

    println!("{}", "━".repeat(100).bright_cyan());
    println!("{}", "  RESULTS: Market vs Passive Execution".bright_cyan().bold());
    println!("{}", "━".repeat(100).bright_cyan());
    println!(
        "  {:<22} {:<12} {:<5} {:>10} {:>10} {:>10} {:>6} {:>8}",
        "Strategy", "Symbol", "Int", "Market%", "Passive%", "Improve%", "Fill%", "Bps"
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
            c.avg_price_improvement_bps
        );
    }

    // Summary
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  Summary".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    let improved_count = comparisons.iter().filter(|c| c.improvement_pct > 0.0).count();
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
    let avg_price_improvement = if !comparisons.is_empty() {
        comparisons.iter().map(|c| c.avg_price_improvement_bps).sum::<f64>() / comparisons.len() as f64
    } else {
        0.0
    };

    println!("  Total comparisons:     {}", comparisons.len());
    println!("  Improved by passive:   {}/{} ({:.0f}%)",
        improved_count.green(),
        comparisons.len(),
        improved_count as f64 / comparisons.len().max(1) as f64 * 100.0
    );
    println!("  Avg return improvement: {:.1}%", avg_improvement);
    println!("  Avg fill rate:          {:.0f}%", avg_fill_rate * 100.0);
    println!("  Avg price improvement:  {:.1f} bps", avg_price_improvement);
    println!("  Fee savings:            0.1% per trade (maker vs taker)");

    // Explanation
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  How Passive Execution Works".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!("
  1. Signal fires on 1h candle close at $100
  2. Instead of market buy at $100 (0.1% fee), place limit at $99.95 (5 bps below)
  3. Scan 1m candles within that hour:
     - If price dips to $99.95 → filled at limit (0% fee, better price!)
     - If price never dips → skip signal
  4. For shorts: place limit above market

  Benefits:
  - Better entry price (5 bps average improvement)
  - Zero fees on FDUSD pairs
  - Forces patience — only trade when market comes to you
");

    println!();
    Ok(())
}

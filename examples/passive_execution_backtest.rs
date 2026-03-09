//! Passive execution backtest for FDUSD pairs (0% maker fees).
//!
//! Walk-forward model: for each 1m candle, place limit N ticks below open.
//! Fill if low <= limit. Near 100% fill rate + 0% fees.
//!
//! Usage:
//!   cargo run --release --example passive_execution_backtest

use anyhow::Result;
use colored::*;
use krypto::{
    algo::SignalGenerator,
    algo::strategies::{DynamicTrend, BollingerReversion, RsiMeanReversion},
    backtest::engine::Backtester,
    backtest::passive::{PassiveExecutor, PassiveConfig, TickSize},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::time::Instant;

const FDUSD_PAIRS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD"];
const INTERVALS: &[&str] = &["1h", "4h"];
const CANDLES: u16 = 1500;
const CAPITAL: f64 = 10_000.0;

#[derive(Debug, Clone)]
struct RunResult {
    strategy: String,
    symbol: String,
    interval: String,
    market_ret: f64,
    passive_ret: f64,
    improve: f64,
    fill_rate: f64,
    ticks: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  PASSIVE EXECUTION — Walk-Forward Limit Orders".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    let loader = DataLoader::new(None, None);
    let backtester = Backtester::with_defaults(CAPITAL);
    let mut results: Vec<RunResult> = Vec::new();

    // Pre-fetch tick sizes from API (globally cached — one call per process)
    println!("\n{}", "Phase 0: Tick sizes from Binance API (cached)...".bright_green());
    for symbol in FDUSD_PAIRS {
        let tick = TickSize::fetch(symbol).await?;
        println!("  {} = {}", symbol, tick.value());
    }

    println!("\n{}", "Phase 1: Fetching OHLCV + 1m data...".bright_green());

    for symbol in FDUSD_PAIRS {
        for interval in INTERVALS {
            print!("  {} {} ... ", symbol, interval);
            let t = Instant::now();

            let df_high = match loader.fetch_data(symbol, interval, CANDLES).await {
                Ok(df) => match FeatureEngine::add_technicals(&df, None) {
                    Ok(df) => df,
                    Err(e) => { println!("{} technicals: {}", "✗".red(), e); continue; }
                },
                Err(e) => { println!("{} {}", "✗".red(), e); continue; }
            };

            let mins: u32 = if *interval == "1h" { 60 } else { 240 };
            let df_low = match loader.fetch_data(symbol, "1m", (CANDLES as u32 * mins) as u16).await {
                Ok(df) => df,
                Err(e) => { println!("{} 1m: {}", "✗".red(), e); continue; }
            };

            println!("{} ({:.1}s)", "✓".green(), t.elapsed().as_secs_f64());

            let tick = TickSize::fetch(symbol).await?.value(); // From cache

            let strategies: Vec<(&str, Box<dyn SignalGenerator>)> = vec![
                ("dynamic_trend",     Box::new(DynamicTrend::new())),
                ("bollinger",         Box::new(BollingerReversion::new())),
                ("rsi",               Box::new(RsiMeanReversion::new())),
            ];

            for (name, mut strat) in strategies {
                let signals = match strat.predict(&df_high) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let market = match backtester.run(&df_high, &signals, 0.05, 0.0) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if market.total_trades == 0 { continue; }

                let config = PassiveConfig {
                    ticks_below_open: 3,
                    tick_size: TickSize::from_value(tick),
                    max_wait_bars: 240,
                    maker_fee: 0.0,
                    update_threshold_ticks: Some(5),
                };
                let executor = PassiveExecutor::new(config);

                let (passive_sigs, stats) = match executor.process_signals(&df_high, &df_low, &signals).await {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                let passive = match backtester.run(&df_high, &passive_sigs, 0.05, 0.0) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                results.push(RunResult {
                    strategy: name.to_string(),
                    symbol: symbol.to_string(),
                    interval: interval.to_string(),
                    market_ret: market.total_return_pct,
                    passive_ret: passive.total_return_pct,
                    improve: passive.total_return_pct - market.total_return_pct,
                    fill_rate: stats.fill_rate,
                    ticks: stats.avg_price_improvement_ticks,
                });
            }
        }
    }

    results.sort_by(|a, b| b.improve.partial_cmp(&a.improve).unwrap());

    println!("\n{}", "━".repeat(100).bright_cyan());
    println!("{}", "  Market vs Passive Execution".bright_cyan().bold());
    println!("{}", "━".repeat(100).bright_cyan());
    println!("  {:<18} {:<12} {:<5} {:>9} {:>9} {:>9} {:>6} {:>6}",
        "Strategy", "Symbol", "Int", "Market%", "Passive%", "Improve%", "Fill%", "Ticks");
    println!("{}", "─".repeat(100));

    for r in &results {
        let imp = if r.improve > 0.0 {
            format!("{:>8.1}%", r.improve).green().to_string()
        } else {
            format!("{:>8.1}%", r.improve).red().to_string()
        };
        println!("  {:<18} {:<12} {:<5} {:>8.1}% {:>8.1}% {} {:>5.0}% {:>6.1}",
            r.strategy, r.symbol, r.interval, r.market_ret, r.passive_ret, imp,
            r.fill_rate * 100.0, r.ticks);
    }

    let improved = results.iter().filter(|r| r.improve > 0.0).count();
    let avg_imp = results.iter().map(|r| r.improve).sum::<f64>() / results.len().max(1) as f64;
    let avg_fill = results.iter().map(|r| r.fill_rate).sum::<f64>() / results.len().max(1) as f64;

    println!("\n  Improved: {}/{} | Avg improvement: {:.1}% | Avg fill rate: {:.0}%",
        improved, results.len(), avg_imp, avg_fill * 100.0);

    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  Caching Strategy".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!("
  Current:
    - OHLCV data  → parquet files in data/cache/ (persist across runs)
    - Tick sizes  → OnceLock<RwLock<HashMap>> (one API call, all 688 symbols)
    - 1m data     → parquet files alongside 1h (60x more data)

  Bottlenecks:
    - 1m parquet files are large (1500 1h bars = 90,000 1m bars)
    - Strategy features recomputed on each run (not cached)
    - No incremental update — full fetch if candle count changes

  Potential improvements:
    - Cache computed signals per strategy+dataset (parquet)
    - Incremental fetch: only append new candles, not full refresh
    - Precompute all technicals once, store enriched parquet
");

    Ok(())
}

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
use std::time::Instant;

const FDUSD_PAIRS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD"];
const INTERVALS: &[&str] = &["1h", "4h"];
// Candle counts chosen so 1m data (candles * 60 or 240) fits in u16 (max 65535)
const CANDLES_1H: u16 = 500;   // 500 * 60 = 30,000 1m candles
const CANDLES_4H: u16 = 200;   // 200 * 240 = 48,000 1m candles
const CAPITAL: f64 = 10_000.0;

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  PASSIVE EXECUTION — Walk-Forward Limit Orders".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    let loader = DataLoader::new(None, None);
    let backtester = Backtester::with_defaults(CAPITAL);
    let mut results: Vec<(&str, &str, &str, f64, f64, f64, f64, f64)> = Vec::new();

    // Pre-fetch tick sizes from API (globally cached)
    println!("\n{}", "Tick sizes from Binance API (cached)...".bright_green());
    for symbol in FDUSD_PAIRS {
        let tick = TickSize::fetch(symbol).await?;
        println!("  {} = {}", symbol, tick.value());
    }

    println!("\n{}", "Fetching OHLCV + 1m data...".bright_green());

    for symbol in FDUSD_PAIRS {
        for interval in INTERVALS {
            let (candles_h, mins_per_bar) = if *interval == "1h" {
                (CANDLES_1H, 60u32)
            } else {
                (CANDLES_4H, 240u32)
            };
            let candles_1m = (candles_h as u32 * mins_per_bar) as u16;

            print!("  {} {} ({} {} bars, {} 1m bars)... ",
                symbol, interval, candles_h, interval, candles_1m);
            let t = Instant::now();

            let df_high = match loader.fetch_data(symbol, interval, candles_h).await {
                Ok(df) => match FeatureEngine::add_technicals(&df, None) {
                    Ok(df) => df,
                    Err(e) => { println!("{} technicals: {}", "✗".red(), e); continue; }
                },
                Err(e) => { println!("{} {}", "✗".red(), e); continue; }
            };

            let df_low = match loader.fetch_data(symbol, "1m", candles_1m).await {
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

                // Market execution backtest
                let market = match backtester.run(&df_high, &signals, 0.05, 0.0) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if market.total_trades == 0 { continue; }

                // Passive execution
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

                // Passive backtest
                let passive = match backtester.run(&df_high, &passive_sigs, 0.05, 0.0) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                results.push((
                    name,
                    symbol,
                    interval,
                    market.total_return_pct,
                    passive.total_return_pct,
                    passive.total_return_pct - market.total_return_pct,
                    stats.fill_rate,
                    stats.avg_price_improvement_ticks,
                ));
            }
        }
    }

    results.sort_by(|a, b| b.5.partial_cmp(&a.5).unwrap());

    println!("\n{}", "━".repeat(100).bright_cyan());
    println!("{}", "  Market vs Passive Execution".bright_cyan().bold());
    println!("{}", "━".repeat(100).bright_cyan());
    println!("  {:<18} {:<12} {:<5} {:>9} {:>9} {:>9} {:>6} {:>6}",
        "Strategy", "Symbol", "Int", "Market%", "Passive%", "Improve%", "Fill%", "Ticks");
    println!("{}", "─".repeat(100));

    for (name, sym, int, mkt, pas, imp, fill, ticks) in &results {
        let imp_str = if *imp > 0.0 {
            format!("{:>8.1}%", imp).green().to_string()
        } else {
            format!("{:>8.1}%", imp).red().to_string()
        };
        println!("  {:<18} {:<12} {:<5} {:>8.1}% {:>8.1}% {} {:>5.0}% {:>6.1}",
            name, sym, int, mkt, pas, imp_str, fill * 100.0, ticks);
    }

    let improved = results.iter().filter(|r| r.5 > 0.0).count();
    let avg_imp = results.iter().map(|r| r.5).sum::<f64>() / results.len().max(1) as f64;
    let avg_fill = results.iter().map(|r| r.6).sum::<f64>() / results.len().max(1) as f64;

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
    - 1m parquet files are large (500 1h bars = 30,000 1m bars)
    - Strategy features recomputed on each run (not cached)
    - No incremental update — full fetch if candle count changes

  Potential improvements:
    - Cache computed signals per strategy+dataset (parquet)
    - Incremental fetch: only append new candles, not full refresh
    - Precompute all technicals once, store enriched parquet
");

    Ok(())
}

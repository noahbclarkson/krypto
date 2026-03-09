//! Passive execution backtest for FDUSD pairs (0% maker fees).
//!
//! Measures entry price improvement from passive limit orders vs market execution.
//!
//! Usage:
//!   cargo run --release --example passive_execution_backtest

use anyhow::Result;
use colored::*;
use krypto::{
    algo::SignalGenerator,
    algo::strategies::{DynamicTrend, BollingerReversion, RsiMeanReversion},
    backtest::passive::{PassiveExecutor, PassiveConfig, TickSize},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use std::time::Instant;

const FDUSD_PAIRS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD"];
const INTERVALS: &[&str] = &["1h", "4h"];
const CANDLES_1H: u16 = 500;
const CANDLES_4H: u16 = 200;

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  PASSIVE EXECUTION — Entry Price Edge".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    let loader = DataLoader::new(None, None);
    let mut results: Vec<(&str, &str, &str, f64, f64, f64, usize)> = Vec::new();

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

            print!("  {} {} ({} bars)... ", symbol, interval, candles_h);
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

            let tick = TickSize::fetch(symbol).await?.value();

            let strategies: Vec<(&str, Box<dyn SignalGenerator>)> = vec![
                ("dynamic_trend", Box::new(DynamicTrend::new())),
                ("bollinger", Box::new(BollingerReversion::new())),
                ("rsi", Box::new(RsiMeanReversion::new())),
            ];

            for (name, mut strat) in strategies {
                let signals = match strat.predict(&df_high) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let config = PassiveConfig {
                    ticks_below_open: 3,
                    tick_size: TickSize::from_value(tick),
                    max_wait_bars: 240,
                    maker_fee: 0.0,
                    update_threshold_ticks: Some(5),
                };
                let executor = PassiveExecutor::new(config);

                let (fills, stats) = match executor.simulate(&df_high, &df_low, &signals).await {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                if fills.is_empty() {
                    continue;
                }

                // Compute average price improvement per fill
                let mut total_improvement_bps = 0.0;
                for fill in &fills {
                    // Price improvement: for longs, fill_price < market_price is good
                    // For shorts, fill_price > market_price is good
                    let improvement = (fill.market_price - fill.fill_price) * fill.direction;
                    let improvement_bps = improvement / fill.market_price * 10000.0; // basis points
                    total_improvement_bps += improvement_bps;
                }
                let avg_improvement_bps = total_improvement_bps / fills.len() as f64;

                // Annualized impact estimate (rough)
                // If we trade N times per period, each trade saves avg_improvement_bps
                // Plus 5 bps fee savings (0.05% taker -> 0% maker)
                let fee_savings_bps = 5.0; // 0.05%
                let total_edge_bps = avg_improvement_bps + fee_savings_bps;

                results.push((
                    name,
                    symbol,
                    interval,
                    avg_improvement_bps,
                    fee_savings_bps,
                    total_edge_bps,
                    fills.len(),
                ));
            }
        }
    }

    results.sort_by(|a, b| b.5.partial_cmp(&a.5).unwrap());

    println!("\n{}", "━".repeat(100).bright_cyan());
    println!("{}", "  Passive Execution Edge (per trade)".bright_cyan().bold());
    println!("{}", "━".repeat(100).bright_cyan());
    println!("  {:<18} {:<12} {:<5} {:>10} {:>10} {:>10} {:>8}",
        "Strategy", "Symbol", "Int", "Price bps", "Fee bps", "Total bps", "Trades");
    println!("{}", "─".repeat(100));

    for (name, sym, int, price, fee, total, trades) in &results {
        let total_str = if *total > 0.0 {
            format!("{:>9.1}", total).green().to_string()
        } else {
            format!("{:>9.1}", total).red().to_string()
        };
        println!("  {:<18} {:<12} {:<5} {:>9.1} {:>9.1} {} {:>7}",
            name, sym, int, price, fee, total_str, trades);
    }

    let avg_price = results.iter().map(|r| r.3).sum::<f64>() / results.len().max(1) as f64;
    let avg_total = results.iter().map(|r| r.5).sum::<f64>() / results.len().max(1) as f64;
    let total_trades: usize = results.iter().map(|r| r.6).sum();

    println!("\n  Avg price improvement: {:.1} bps | Avg total edge: {:.1} bps | Total trades: {}",
        avg_price, avg_total, total_trades);

    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  Edge Breakdown".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!("
  Per-trade edge from passive execution:
  
  1. Price improvement: Entry at (1m open - 3 ticks) vs 1h close
     - Captures intrabar dips
     - Varies by volatility (higher vol = larger dips = more edge)
     
  2. Fee savings: 0% maker (FDUSD pairs) vs ~0.05% taker
     - Fixed 5 bps per trade
     - Compounds over many trades
     
  Total edge = price_improvement + fee_savings
  
  With 100% fill rate, every signal captures this edge.
  Impact on returns = edge_per_trade × num_trades
");

    Ok(())
}

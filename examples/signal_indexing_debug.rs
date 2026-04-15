//! Debug signal indexing in paper bot vs backtest.
//!
//! Traces exactly which signal values are used at each bar.

use anyhow::Result;
use chrono::{DateTime, Utc};
use krypto::{
    algo::StrategyRegistry,
    backtest::engine::{Backtester, PositionSizing},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
    paper::{Bar, PaperBot, SignalGeneratorAdapter, Strategy},
};

const SYMBOL: &str = "ETHFDUSD";
const INTERVAL: &str = "1d";
const CANDLES: u32 = 150;
const INITIAL_CAPITAL: f64 = 2000.0;
const FEE: f64 = 0.0;
const ATR_MULT: f64 = 0.30;

fn df_to_bars(df: &polars::prelude::DataFrame) -> Result<Vec<Bar>> {
    use polars::prelude::*;

    let time_col = df.column("time")?.datetime()?;
    let open_col = df.column("open")?.f64()?;
    let high_col = df.column("high")?.f64()?;
    let low_col = df.column("low")?.f64()?;
    let close_col = df.column("close")?.f64()?;
    let volume_col = df.column("volume")?.f64()?;

    let mut bars = Vec::with_capacity(df.height());

    for i in 0..df.height() {
        let time_ms = time_col.get(i).unwrap_or(0);
        let secs = time_ms / 1000;
        let nsecs = ((time_ms % 1000) * 1_000_000) as u32;

        let time = DateTime::from_timestamp(secs, nsecs).unwrap_or_else(Utc::now);

        bars.push(Bar::new(
            time,
            open_col.get(i).unwrap_or(0.0),
            high_col.get(i).unwrap_or(0.0),
            low_col.get(i).unwrap_or(0.0),
            close_col.get(i).unwrap_or(0.0),
            volume_col.get(i).unwrap_or(0.0),
        ));
    }

    Ok(bars)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80));
    println!("  SIGNAL INDEXING DEBUG");
    println!("{}", "━".repeat(80));

    // Load data
    let loader = DataLoader::new(None, None);
    let raw = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;
    let bars = df_to_bars(&df)?;

    println!("\nLoaded {} bars", bars.len());

    // Generate signals
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();
    let signals = strategy.predict(&df)?;
    let signals_f64 = signals.f64()?;

    // Show which bars have signals in backtest
    println!("\n{:^80}", "BACKTEST SIGNAL USAGE");
    println!("{}", "-".repeat(80));
    println!("At bar i, backtest uses signals[i-1]");
    println!();

    for i in 1..bars.len().min(150) {
        let sig = signals_f64.get(i - 1).unwrap_or(0.0);
        if sig != 0.0 {
            println!(
                "Bar {:3} (global): uses signals[{:3}] = {:>4.1} → {:?}",
                i,
                i - 1,
                sig,
                if sig > 0.0 { "LONG" } else { "SHORT" }
            );
        }
    }

    // Now trace what the paper bot sees
    println!("\n{:^80}", "PAPER BOT SIGNAL INDEXING");
    println!("{}", "-".repeat(80));
    println!("Paper bot uses signals.len().saturating_sub(2)");
    println!();

    // Create adapter with verbose logging
    let strategy2 = registry.create("bollinger_reversion").unwrap();
    let mut adapter = SignalGeneratorAdapter::new(strategy2, 100, ATR_MULT).with_verbose();

    let mut bot = PaperBot::new(Box::new(adapter), INITIAL_CAPITAL).with_fee(FEE);

    for (global_idx, bar) in bars.iter().enumerate() {
        bot.on_bar(bar);

        // Stop after first 30 bars for debugging
        if global_idx >= 30 {
            break;
        }
    }

    println!("\n{:^80}", "COMPARISON");
    println!("{}", "-".repeat(80));
    println!();
    println!("If backtest uses signals[i-1] and paper bot uses signals[n-2],");
    println!("they should match when i == n (global bar index == current bar count).");
    println!();
    println!("Example: At global bar 21:");
    println!("  - Backtest: uses signals[20]");
    println!("  - Paper bot: has 22 bars (0-21), signals.len()=22, uses signals[20]");
    println!("  → MATCH");
    println!();
    println!("But warmup period means paper bot doesn't trade until bar 20+");

    Ok(())
}

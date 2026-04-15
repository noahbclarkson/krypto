//! Extended signal tracing for paper bot - check if signals are detected at bar 48+.

use anyhow::Result;
use chrono::{DateTime, Utc};
use krypto::{
    algo::StrategyRegistry,
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
    println!("  EXTENDED PAPER BOT SIGNAL TRACE (bars 40-60)");
    println!("{}", "━".repeat(80));

    // Load data
    let loader = DataLoader::new(None, None);
    let raw = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;
    let bars = df_to_bars(&df)?;

    println!("\nLoaded {} bars", bars.len());

    // Create adapter with verbose logging
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();
    let mut adapter = SignalGeneratorAdapter::new(strategy, 100, ATR_MULT).with_verbose();

    let mut bot = PaperBot::new(Box::new(adapter), INITIAL_CAPITAL).with_fee(FEE);

    println!(
        "\n{:<6} | {:<10} | {:<12} | {:<10} | {:<10}",
        "Bar", "Close", "Signal", "Position", "Equity"
    );
    println!("{}", "-".repeat(80));

    for (global_idx, bar) in bars.iter().enumerate() {
        let equity_before = bot.equity();
        bot.on_bar(bar);
        let equity_after = bot.equity();

        // Only print bars 40-60 to see the first signals
        if global_idx >= 40 && global_idx <= 60 {
            println!(
                "{:<6} | {:>10.2} | eq_before={:.2}, eq_after={:.2}",
                global_idx, bar.close, equity_before, equity_after
            );
        }
    }

    let summary = bot.summary();

    println!("\n{}", "━".repeat(80));
    println!("  FINAL SUMMARY");
    println!("{}", "━".repeat(80));
    println!("  Return:  {:>8.1}%", summary.total_return_pct);
    println!("  Trades:  {:>8}", summary.total_trades);
    println!("  WinRate: {:>8.1}%", summary.win_rate);

    Ok(())
}

//! Check why paper bot doesn't enter second LONG at bar 115.

use anyhow::Result;
use chrono::{DateTime, Utc};
use krypto::{
    algo::StrategyRegistry,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
    paper::{Bar, PaperBot, Position, SignalGeneratorAdapter, Strategy},
};

const SYMBOL: &str = "ETHFDUSD";
const INTERVAL: &str = "1d";
const CANDLES: u32 = 500;
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
    println!("  WHY NO SECOND LONG AT BAR 115?");
    println!("{}", "━".repeat(80));

    // Load data
    let loader = DataLoader::new(None, None);
    let raw = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;
    let bars = df_to_bars(&df)?;

    // Generate signals
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();
    let signals = strategy.predict(&df)?;
    let signals_f64 = signals.f64()?;

    // Check signals at bars 113-116
    println!("\n{:^80}", "FULL SIGNALS");
    println!("{}", "-".repeat(80));
    for i in 113..=116 {
        let sig = signals_f64.get(i).unwrap_or(0.0);
        println!("  signals[{}] = {:.1}", i, sig);
    }

    // Run paper bot and check what signal it sees at bar 115
    println!("\n{:^80}", "PAPER BOT SIGNALS (adapter's rolling window)");
    println!("{}", "-".repeat(80));

    let strategy2 = registry.create("bollinger_reversion").unwrap();
    let adapter = SignalGeneratorAdapter::new(strategy2, 100, ATR_MULT).with_verbose();
    let mut bot = PaperBot::new(Box::new(adapter), INITIAL_CAPITAL).with_fee(FEE);

    for (idx, bar) in bars.iter().enumerate() {
        let equity_before = bot.equity();
        let position_before = *bot.position();
        bot.on_bar(bar);
        let equity_after = bot.equity();
        let position_after = *bot.position();

        if idx >= 113 && idx <= 116 {
            let pos_str_before = match position_before {
                Position::Flat => "FLAT",
                Position::Long { .. } => "LONG",
                Position::Short { .. } => "SHORT",
            };
            let pos_str_after = match position_after {
                Position::Flat => "FLAT",
                Position::Long { .. } => "LONG",
                Position::Short { .. } => "SHORT",
            };

            println!("\n  Bar {:3}: close={:.2}", idx, bar.close);
            println!("    Position: {} -> {}", pos_str_before, pos_str_after);
            println!("    Equity:   {:.2} -> {:.2}", equity_before, equity_after);
        }
    }

    // Check backtest behavior
    println!("\n{:^80}", "BACKTEST BEHAVIOR");
    println!("{}", "-".repeat(80));
    println!("  Bar 114: Uses signals[113] = 1.0 → Enter LONG");
    println!("  Bar 115: Uses signals[114] = 1.0 → Exit LONG, Enter new LONG");
    println!("  Bar 116: Uses signals[115] = 0.0 → Exit LONG (or hold if signal exits)");

    Ok(())
}

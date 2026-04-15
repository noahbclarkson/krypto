//! Debug paper bot position tracking.

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

struct DebugBot {
    inner: PaperBot,
}

impl DebugBot {
    fn new(inner: PaperBot) -> Self {
        Self { inner }
    }

    fn on_bar(&mut self, bar: &Bar) -> (f64, Position) {
        self.inner.on_bar(bar);
        (self.inner.equity(), *self.inner.position())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80));
    println!("  PAPER BOT POSITION TRACKING (bars 105-120)");
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

    // Create paper bot
    let strategy2 = registry.create("bollinger_reversion").unwrap();
    let adapter = SignalGeneratorAdapter::new(strategy2, 100, ATR_MULT);
    let mut bot = DebugBot::new(PaperBot::new(Box::new(adapter), INITIAL_CAPITAL).with_fee(FEE));

    println!(
        "\n{:<6} | {:<10} | {:<12} | {:<12} | {:<10}",
        "Bar", "Close", "Signal", "Position", "Equity"
    );
    println!("{}", "-".repeat(80));

    for (idx, bar) in bars.iter().enumerate() {
        let (equity, position) = bot.on_bar(bar);

        if idx >= 105 && idx <= 120 {
            let sig = signals_f64.get(idx).unwrap_or(0.0);
            let pos_str = match position {
                Position::Flat => "FLAT".to_string(),
                Position::Long { entry_price, .. } => format!("LONG @{:.2}", entry_price),
                Position::Short { entry_price, .. } => format!("SHORT @{:.2}", entry_price),
            };
            println!(
                "{:<6} | {:>10.2} | {:>12.1} | {:<12} | {:>10.2}",
                idx, bar.close, sig, pos_str, equity
            );
        }
    }

    Ok(())
}

//! Check if paper bot trades match adapter signals.

use anyhow::Result;
use chrono::{DateTime, Utc};
use krypto::{
    algo::StrategyRegistry,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
    paper::{Bar, PaperBot, Position, SignalGeneratorAdapter, Strategy, Trade},
};

const SYMBOL: &str = "ETHFDUSD";
const INTERVAL: &str = "1d";
const CANDLES: u32 = 200;
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
    println!("  ADAPTER TRADES vs PAPER BOT EQUITY CHANGES");
    println!("{}", "━".repeat(80));

    // Load data
    let loader = DataLoader::new(None, None);
    let raw = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;
    let bars = df_to_bars(&df)?;

    // Create custom adapter that logs trades
    #[derive(Debug)]
    struct LoggingAdapter {
        inner: SignalGeneratorAdapter,
    }

    impl Strategy for LoggingAdapter {
        fn name(&self) -> &str {
            self.inner.name()
        }

        fn on_bar(&mut self, bar: &Bar, position: f64, history: &[Bar]) -> Option<Trade> {
            let result = self.inner.on_bar(bar, position, history);
            if result.is_some() {
                eprintln!(
                    "[ADAPTER] Bar close={:.2}, position={:.1}, trade={:?}",
                    bar.close, position, result
                );
            }
            result
        }

        fn reset(&mut self) {
            self.inner.reset();
        }
    }

    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();
    let adapter = SignalGeneratorAdapter::new(strategy, 100, ATR_MULT);
    let logging_adapter = LoggingAdapter { inner: adapter };

    let mut bot = PaperBot::new(Box::new(logging_adapter), INITIAL_CAPITAL).with_fee(FEE);

    println!("\nProcessing bars 100-160...");

    for (idx, bar) in bars.iter().enumerate() {
        let equity_before = bot.equity();
        bot.on_bar(bar);
        let equity_after = bot.equity();

        if idx >= 100 && idx <= 160 && equity_before != equity_after {
            println!(
                "[PAPER BOT] Bar {:3} | ${:.2} -> ${:.2} ({:+.1}%)",
                idx,
                equity_before,
                equity_after,
                (equity_after - equity_before) / equity_before * 100.0
            );
        }
    }

    Ok(())
}

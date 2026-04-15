//! Debug consecutive trades at bars 114-115.

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
    println!("  CONSECUTIVE TRADE DEBUG (bars 110-120)");
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
    println!("\n{:^80}", "SIGNAL VALUES");
    println!("{}", "-".repeat(80));
    for i in 113..=116 {
        let sig = signals_f64.get(i).unwrap_or(0.0);
        println!("  signals[{}] = {:.1}", i, sig);
    }

    // Run backtest
    let trailing_sl = 0.012;
    let backtester =
        Backtester::new(INITIAL_CAPITAL, FEE, 0.0).with_position_sizing(PositionSizing::Full);
    let backtest_result = backtester.run(&df, &signals, trailing_sl, 0.0)?;

    println!("\n{:^80}", "BACKTEST TRADES (bars 110-120)");
    println!("{}", "-".repeat(80));
    for trade in &backtest_result.trades {
        if trade.entry_bar >= 110 && trade.entry_bar <= 120 {
            let dir = if trade.direction > 0.0 {
                "LONG"
            } else {
                "SHORT"
            };
            println!(
                "  {} Bar {:3}->{:<3} @{:.2} -> @{:.2} ({:+.1}%)",
                dir,
                trade.entry_bar,
                trade.exit_bar,
                trade.entry_price,
                trade.exit_price,
                trade.pnl_pct * 100.0
            );
        }
    }

    // Run paper bot with verbose logging
    println!("\n{:^80}", "PAPER BOT PROCESSING (bars 110-120)");
    println!("{}", "-".repeat(80));

    let strategy2 = registry.create("bollinger_reversion").unwrap();
    let adapter = SignalGeneratorAdapter::new(strategy2, 100, ATR_MULT).with_verbose();
    let mut bot = PaperBot::new(Box::new(adapter), INITIAL_CAPITAL).with_fee(FEE);

    for (idx, bar) in bars.iter().enumerate() {
        let equity_before = bot.equity();
        bot.on_bar(bar);
        let equity_after = bot.equity();

        if idx >= 110 && idx <= 120 {
            let sig = signals_f64.get(idx).unwrap_or(0.0);
            println!(
                "  Bar {:3} | close={:.2} | signal={:.1} | eq={:.2} -> {:.2}",
                idx, bar.close, sig, equity_before, equity_after
            );
        }
    }

    Ok(())
}

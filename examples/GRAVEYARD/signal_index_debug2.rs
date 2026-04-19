//! Debug signal indexing with current code (signals.len() - 2).

use anyhow::Result;
use chrono::{DateTime, Utc};
use krypto::{
    algo::StrategyRegistry,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
    paper::{Bar, PaperBot, SignalGeneratorAdapter, Strategy},
};
use polars::prelude::*;

const SYMBOL: &str = "ETHFDUSD";
const INTERVAL: &str = "1d";
const CANDLES: u32 = 500;
const INITIAL_CAPITAL: f64 = 2000.0;
const FEE: f64 = 0.0;
const ATR_MULT: f64 = 0.30;

fn df_to_bars(df: &DataFrame) -> Result<Vec<Bar>> {
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

fn bars_to_df(bars: &[Bar]) -> DataFrame {
    let times: Vec<i64> = bars.iter().map(|b| b.time.timestamp_millis()).collect();
    let opens: Vec<f64> = bars.iter().map(|b| b.open).collect();
    let highs: Vec<f64> = bars.iter().map(|b| b.high).collect();
    let lows: Vec<f64> = bars.iter().map(|b| b.low).collect();
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let volumes: Vec<f64> = bars.iter().map(|b| b.volume).collect();

    df! [
        "time" => times,
        "open" => opens,
        "high" => highs,
        "low" => lows,
        "close" => closes,
        "volume" => volumes,
    ]
    .unwrap()
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80));
    println!("  SIGNAL INDEXING DEBUG (bars 113-117)");
    println!("{}", "━".repeat(80));

    // Load data
    let loader = DataLoader::new(None, None);
    let raw = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;
    let bars = df_to_bars(&raw)?;

    // Generate signals on full data
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();
    let signals_full = strategy.predict(&df)?;
    let signals_full_f64 = signals_full.f64()?;

    println!("\n{:^80}", "FULL SIGNALS (backtest uses signals[i-1])");
    println!("{}", "-".repeat(80));
    for i in 113..=117 {
        let sig = signals_full_f64.get(i).unwrap_or(0.0);
        let prev_sig = if i > 0 {
            signals_full_f64.get(i - 1).unwrap_or(0.0)
        } else {
            0.0
        };
        println!(
            "  Bar {:3}: signals[{:3}] = {:.1}, signals[{:3}] = {:.1}",
            i,
            i,
            sig,
            i - 1,
            prev_sig
        );
    }

    // Simulate what the adapter sees
    println!("\n{:^80}", "ADAPTER SIGNAL INDEXING (rolling window)");
    println!("{}", "-".repeat(80));

    let lookback: usize = 100;
    for global_idx in 113usize..=117 {
        // Build rolling window
        let window_start = global_idx.saturating_sub(lookback);
        let window_bars: Vec<Bar> = bars[window_start..=global_idx].to_vec();
        let window_df = bars_to_df(&window_bars);
        let window_df_with_features = FeatureEngine::add_technicals(&window_df, None)?;

        // Generate signals
        let strategy2 = registry.create("bollinger_reversion").unwrap();
        let signals_window = strategy2.predict(&window_df_with_features)?;
        let signals_window_f64 = signals_window.f64()?;

        // Get signal at index (signals.len() - 2)
        let signal_idx = signals_window.len().saturating_sub(2);
        let signal = signals_window_f64.get(signal_idx).unwrap_or(0.0);

        println!(
            "  Bar {:3} (global): window.len() = {:3}, signals.len() = {:3}, signal_idx = {:3}, signal = {:.1}",
            global_idx, window_bars.len(), signals_window.len(), signal_idx, signal
        );
    }

    // Expected backtest signals
    println!("\n{:^80}", "EXPECTED SIGNALS (backtest)");
    println!("{}", "-".repeat(80));
    for i in 114..=117 {
        let backtest_sig = signals_full_f64.get(i - 1).unwrap_or(0.0);
        println!(
            "  Backtest at bar {:3}: uses signals[{:3}] = {:.1}",
            i,
            i - 1,
            backtest_sig
        );
    }

    Ok(())
}

//! Compare signal generation: full history vs rolling window.
//!
//! Tests whether the paper bot's rolling window produces the same signals
//! as the backtest's full history.

use anyhow::Result;
use chrono::{DateTime, Utc};
use krypto::{
    algo::StrategyRegistry, data::loader::DataLoader, features::indicators::FeatureEngine,
};
use polars::prelude::*;

const SYMBOL: &str = "ETHFDUSD";
const INTERVAL: &str = "1d";
const CANDLES: u32 = 150;

fn df_to_bars(df: &DataFrame) -> Result<Vec<(i64, f64, f64, f64, f64, f64)>> {
    let time_col = df.column("time")?.datetime()?;
    let open_col = df.column("open")?.f64()?;
    let high_col = df.column("high")?.f64()?;
    let low_col = df.column("low")?.f64()?;
    let close_col = df.column("close")?.f64()?;
    let volume_col = df.column("volume")?.f64()?;

    let mut bars = Vec::with_capacity(df.height());

    for i in 0..df.height() {
        bars.push((
            time_col.get(i).unwrap_or(0),
            open_col.get(i).unwrap_or(0.0),
            high_col.get(i).unwrap_or(0.0),
            low_col.get(i).unwrap_or(0.0),
            close_col.get(i).unwrap_or(0.0),
            volume_col.get(i).unwrap_or(0.0),
        ));
    }

    Ok(bars)
}

fn bars_to_df(bars: &[(i64, f64, f64, f64, f64, f64)]) -> DataFrame {
    let times: Vec<i64> = bars.iter().map(|b| b.0).collect();
    let opens: Vec<f64> = bars.iter().map(|b| b.1).collect();
    let highs: Vec<f64> = bars.iter().map(|b| b.2).collect();
    let lows: Vec<f64> = bars.iter().map(|b| b.3).collect();
    let closes: Vec<f64> = bars.iter().map(|b| b.4).collect();
    let volumes: Vec<f64> = bars.iter().map(|b| b.5).collect();

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
    println!("  ROLLING WINDOW vs FULL HISTORY SIGNAL COMPARISON");
    println!("{}", "━".repeat(80));

    // Load full data
    let loader = DataLoader::new(None, None);
    let raw = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df_full = FeatureEngine::add_technicals(&raw, None)?;
    let bars = df_to_bars(&raw)?;

    println!("\nLoaded {} bars", bars.len());

    // Generate signals on full history
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();
    let signals_full = strategy.predict(&df_full)?;
    let signals_full_f64 = signals_full.f64()?;

    println!("\n{:^80}", "SIGNAL COMPARISON (bars 20-50)");
    println!("{}", "-".repeat(80));
    println!(
        "{:<6} | {:<12} | {:<12} | {:<12}",
        "Bar", "Full[i-1]", "Rolling[n-2]", "Match?"
    );
    println!("{}", "-".repeat(80));

    // Compare signals at each bar
    let mut mismatches = 0;
    let lookback = 100;

    for i in 20..50.min(bars.len()) {
        // Full history signal (what backtest uses)
        let full_sig = if i > 0 {
            signals_full_f64.get(i - 1).unwrap_or(0.0)
        } else {
            0.0
        };

        // Rolling window signal (what paper bot sees)
        let window_start = i.saturating_sub(lookback);
        let window_bars: Vec<_> = bars[window_start..=i].to_vec();
        let window_df = bars_to_df(&window_bars);
        let window_df_with_features = FeatureEngine::add_technicals(&window_df, None)?;

        let strategy2 = registry.create("bollinger_reversion").unwrap();
        let signals_window = strategy2.predict(&window_df_with_features)?;
        let signals_window_f64 = signals_window.f64()?;

        let window_sig = signals_window_f64
            .get(signals_window.len().saturating_sub(2))
            .unwrap_or(0.0);

        let match_status = if (full_sig - window_sig).abs() < 0.01 {
            "✓"
        } else {
            mismatches += 1;
            "✗"
        };

        if full_sig != 0.0 || window_sig != 0.0 {
            println!(
                "{:<6} | {:>12.1} | {:>12.1} | {:<12}",
                i, full_sig, window_sig, match_status
            );
        }
    }

    println!("{}", "-".repeat(80));
    println!("Mismatches: {}", mismatches);

    // Check signal values at specific bars
    println!("\n{:^80}", "SIGNAL VALUES AT KEY BARS");
    println!("{}", "-".repeat(80));

    for i in [20, 47, 48, 71, 72, 100, 101] {
        if i < bars.len() {
            let sig = signals_full_f64.get(i).unwrap_or(0.0);
            let prev_sig = if i > 0 {
                signals_full_f64.get(i - 1).unwrap_or(0.0)
            } else {
                0.0
            };
            println!(
                "Bar {:3}: signals[{:3}] = {:>4.1}, signals[{:3}] = {:>4.1}",
                i,
                i,
                sig,
                i - 1,
                prev_sig
            );
        }
    }

    Ok(())
}

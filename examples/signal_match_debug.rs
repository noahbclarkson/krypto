//! Verify that rolling window signals match full signals at the same indices.

use anyhow::Result;
use chrono::{DateTime, Utc};
use krypto::{
    algo::StrategyRegistry, data::loader::DataLoader, features::indicators::FeatureEngine,
};
use polars::prelude::*;

const SYMBOL: &str = "ETHFDUSD";
const INTERVAL: &str = "1d";
const CANDLES: u32 = 500;

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
    println!("  ROLLING WINDOW vs FULL SIGNALS AT SAME GLOBAL INDICES");
    println!("{}", "━".repeat(80));

    // Load data
    let loader = DataLoader::new(None, None);
    let raw = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df_full = FeatureEngine::add_technicals(&raw, None)?;
    let bars = df_to_bars(&raw)?;

    // Generate full signals
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();
    let signals_full = strategy.predict(&df_full)?;
    let signals_full_f64 = signals_full.f64()?;

    println!("\nComparing signals at global indices 112-116:");
    println!(
        "{:<10} | {:<15} | {:<15} | {:<10}",
        "Global Idx", "Full Signal", "Window Signal", "Match?"
    );
    println!("{}", "-".repeat(60));

    let lookback: usize = 100;

    for global_idx in 112usize..=116 {
        // Build rolling window
        let window_start = global_idx.saturating_sub(lookback);
        let window_bars: Vec<_> = bars[window_start..=global_idx].to_vec();
        let window_df = bars_to_df(&window_bars);
        let window_df_with_features = FeatureEngine::add_technicals(&window_df, None)?;

        // Generate signals on window
        let strategy2 = registry.create("bollinger_reversion").unwrap();
        let signals_window = strategy2.predict(&window_df_with_features)?;
        let signals_window_f64 = signals_window.f64()?;

        // The last signal in the window corresponds to global_idx
        let window_last_idx = signals_window.len() - 1;
        let window_signal = signals_window_f64.get(window_last_idx).unwrap_or(0.0);

        // Full signal at same global index
        let full_signal = signals_full_f64.get(global_idx).unwrap_or(0.0);

        let match_status = if (window_signal - full_signal).abs() < 0.01 {
            "✓"
        } else {
            "✗"
        };

        println!(
            "{:<10} | {:>15.1} | {:>15.1} | {:<10}",
            global_idx, full_signal, window_signal, match_status
        );
    }

    // Now check what the adapter actually uses
    println!("\n{:^80}", "WHAT ADAPTER USEES (signals.len() - 2)");
    println!("{}", "-".repeat(80));
    println!(
        "{:<10} | {:<15} | {:<15} | {:<10}",
        "Global Idx", "Adapter Uses", "Backtest Uses", "Match?"
    );
    println!("{}", "-".repeat(60));

    for global_idx in 112usize..=116 {
        // Build rolling window
        let window_start = global_idx.saturating_sub(lookback);
        let window_bars: Vec<_> = bars[window_start..=global_idx].to_vec();
        let window_df = bars_to_df(&window_bars);
        let window_df_with_features = FeatureEngine::add_technicals(&window_df, None)?;

        // Generate signals on window
        let strategy2 = registry.create("bollinger_reversion").unwrap();
        let signals_window = strategy2.predict(&window_df_with_features)?;
        let signals_window_f64 = signals_window.f64()?;

        // Adapter uses signals.len() - 2
        let adapter_idx = signals_window.len().saturating_sub(2);
        let adapter_signal = signals_window_f64.get(adapter_idx).unwrap_or(0.0);

        // Backtest uses signals[global_idx - 1]
        let backtest_signal = if global_idx > 0 {
            signals_full_f64.get(global_idx - 1).unwrap_or(0.0)
        } else {
            0.0
        };

        let match_status = if (adapter_signal - backtest_signal).abs() < 0.01 {
            "✓"
        } else {
            "✗"
        };

        println!(
            "{:<10} | {:>15.1} | {:>15.1} | {:<10}",
            global_idx, adapter_signal, backtest_signal, match_status
        );
    }

    Ok(())
}

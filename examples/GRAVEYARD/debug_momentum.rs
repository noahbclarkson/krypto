//! Debug momentum signal

use anyhow::Result;
use krypto::{
    backtest::engine::Backtester, data::loader::DataLoader, features::indicators::FeatureEngine,
};
use polars::prelude::*;

const CANDLES: u32 = 2000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;

fn compute_atr_stop(df: &DataFrame, atr_mult: f64) -> f64 {
    let atr = df
        .column("atr")
        .ok()
        .and_then(|s| {
            s.f64()
                .ok()
                .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
        })
        .unwrap_or(0.0);
    let close = df
        .column("close")
        .ok()
        .and_then(|s| {
            s.f64()
                .ok()
                .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
        })
        .unwrap_or(1.0);
    if close > 0.0 {
        (atr * atr_mult / close).clamp(0.005, 0.30)
    } else {
        0.05
    }
}

fn generate_momentum_signals(df: &DataFrame, lookback: usize) -> Vec<f64> {
    let close = df.column("close").unwrap().f64().unwrap();

    let mut signals = vec![0.0; df.height()];

    for i in lookback..df.height() {
        let c = close.get(i).unwrap_or(0.0);
        let c_prev = close.get(i - lookback).unwrap_or(0.0);

        if c_prev > 0.0 {
            let ret = (c / c_prev - 1.0) * 100.0;

            // Mean reversion on momentum: buy on DIPS, sell on SPIKES
            if ret < -10.0 {
                signals[i] = 1.0; // Big drop = buy (mean reversion)
            } else if ret > 10.0 {
                signals[i] = -1.0; // Big spike = sell (mean reversion)
            }
        }
    }

    signals
}

#[tokio::main]
async fn main() -> Result<()> {
    let loader = DataLoader::new(None, None);

    let symbol = "DOGEUSDT";
    println!("Testing {}...", symbol);

    let raw = loader.fetch_data(symbol, "1d", CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;

    let signals = generate_momentum_signals(&df, 5);
    let n_signals = signals.iter().filter(|&&s| s != 0.0).count();
    let n_long = signals.iter().filter(|&&s| s > 0.0).count();
    let n_short = signals.iter().filter(|&&s| s < 0.0).count();

    println!("Total bars: {}", df.height());
    println!(
        "Total signals: {} ({:.1}%)",
        n_signals,
        n_signals as f64 / df.height() as f64 * 100.0
    );
    println!("  Long: {}", n_long);
    println!("  Short: {}", n_short);

    let trailing_sl = compute_atr_stop(&df, 0.3);
    println!("\nATR×0.3 stop: {:.2}%", trailing_sl * 100.0);

    let backtester = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
    let signals_series = Series::new("signal".into(), signals);
    let result = backtester.run(&df, &signals_series, trailing_sl, 0.0)?;

    println!("\nBacktest result:");
    println!("  Total return: {:.1}%", result.total_return_pct);
    println!("  Sharpe: {:.1}", result.annualised_sharpe);
    println!("  Max DD: {:.1}%", result.max_drawdown_pct);
    println!("  Trades: {}", result.total_trades);
    println!("  Win rate: {:.1}%", result.win_rate * 100.0);

    // Check for extreme values
    if result.annualised_sharpe > 1000.0 {
        println!("\n⚠️  EXTREME SHARPE — likely bug or edge case");
        println!("  This suggests near-zero variance in returns");
    }

    Ok(())
}

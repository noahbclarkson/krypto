//! Debug momentum signal — detailed trade analysis

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

            if ret < -10.0 {
                signals[i] = 1.0;
            } else if ret > 10.0 {
                signals[i] = -1.0;
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

    let trailing_sl = compute_atr_stop(&df, 0.3);
    println!("ATR×0.3 stop: {:.2}%", trailing_sl * 100.0);

    let backtester = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
    let signals_series = Series::new("signal".into(), signals);
    let result = backtester.run(&df, &signals_series, trailing_sl, 0.0)?;

    println!("\nBacktest result:");
    println!("  Total return: {:.1}%", result.total_return_pct);
    println!("  Sharpe: {:.1}", result.annualised_sharpe);
    println!("  Max DD: {:.1}%", result.max_drawdown_pct);
    println!("  Trades: {}", result.total_trades);
    println!("  Win rate: {:.1}%", result.win_rate); // Already × 100

    // Analyze trade distribution
    println!("\nTrade analysis:");

    let wins: Vec<_> = result
        .trades
        .iter()
        .filter(|t| t.pnl_amount > 0.0)
        .collect();
    let losses: Vec<_> = result
        .trades
        .iter()
        .filter(|t| t.pnl_amount <= 0.0)
        .collect();

    let avg_win_pct = if !wins.is_empty() {
        wins.iter().map(|t| t.pnl_pct * 100.0).sum::<f64>() / wins.len() as f64
    } else {
        0.0
    };

    let avg_loss_pct = if !losses.is_empty() {
        losses.iter().map(|t| t.pnl_pct * 100.0).sum::<f64>() / losses.len() as f64
    } else {
        0.0
    };

    let max_win_pct = wins
        .iter()
        .map(|t| t.pnl_pct * 100.0)
        .fold(0.0_f64, |a, b| a.max(b));
    let max_loss_pct = losses
        .iter()
        .map(|t| t.pnl_pct * 100.0)
        .fold(0.0_f64, |a, b| a.min(b));

    println!(
        "  Wins: {} (avg {:.2}%, max {:.2}%)",
        wins.len(),
        avg_win_pct,
        max_win_pct
    );
    println!(
        "  Losses: {} (avg {:.2}%, max {:.2}%)",
        losses.len(),
        avg_loss_pct,
        max_loss_pct
    );

    // Check for extreme outliers
    let extreme_wins: Vec<_> = wins.iter().filter(|t| t.pnl_pct > 0.10).collect();
    let extreme_losses: Vec<_> = losses.iter().filter(|t| t.pnl_pct < -0.10).collect();

    if !extreme_wins.is_empty() {
        println!("\n  ⚠️ EXTREME WINS (>10%): {}", extreme_wins.len());
        for t in extreme_wins.iter().take(5) {
            println!(
                "    Bar {}→{}: {:.1}%",
                t.entry_bar,
                t.exit_bar,
                t.pnl_pct * 100.0
            );
        }
    }

    if !extreme_losses.is_empty() {
        println!("\n  ⚠️ EXTREME LOSSES (<-10%): {}", extreme_losses.len());
        for t in extreme_losses.iter().take(5) {
            println!(
                "    Bar {}→{}: {:.1}%",
                t.entry_bar,
                t.exit_bar,
                t.pnl_pct * 100.0
            );
        }
    }

    // Calculate expected value
    let ev_per_trade = (wins.len() as f64 / result.total_trades as f64) * avg_win_pct / 100.0
        + (losses.len() as f64 / result.total_trades as f64) * avg_loss_pct / 100.0;
    println!("\n  Expected value per trade: {:.2}%", ev_per_trade * 100.0);

    // Simulate compounding
    let mut equity = CAPITAL;
    for t in &result.trades {
        equity *= 1.0 + t.pnl_pct;
    }
    println!("  Simulated final equity: ${:.0}", equity);
    println!(
        "  Simulated return: {:.0}%",
        (equity / CAPITAL - 1.0) * 100.0
    );

    Ok(())
}

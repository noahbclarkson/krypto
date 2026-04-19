//! Time-Based Exit Strategy Test
//!
//! After discovering that trailing stops with daily OHLC are optimistic (high/low
//! sequence is a coin flip), test if time-based exits work.
//!
//! No trailing stops = no intra-bar uncertainty. Just hold for N bars and exit.

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];
const CANDLES: u32 = 3000; // ~8 years at 1d
const TAKER_FEE: f64 = 0.001; // 0.1% per side

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== TIME-BASED EXIT STRATEGY TEST ===\n");
    println!("Testing BollingerReversion with time-based exits (N bars).\n");
    println!("This removes trailing stop intra-bar uncertainty entirely.\n");

    let loader = DataLoader::new(None, None);

    // Load data once
    let mut data_cache: std::collections::HashMap<String, DataFrame> =
        std::collections::HashMap::new();

    for symbol in SYMBOLS {
        print!("Loading {}... ", symbol);
        match loader.fetch_data(symbol, "1d", CANDLES).await {
            Ok(raw) => {
                let df = FeatureEngine::add_technicals(&raw, None)?;
                println!("{} bars", df.height());
                data_cache.insert(symbol.to_string(), df);
            }
            Err(e) => {
                println!("SKIP ({})", e);
            }
        }
    }

    // Test different hold periods
    let hold_periods = [1, 2, 3, 5, 7, 10, 14];

    for hold_bars in hold_periods {
        println!("\n{}", "=".repeat(80));
        println!("HOLD PERIOD: {} bar(s)", hold_bars);
        println!("{}", "=".repeat(80));

        let mut total_return = 0.0;
        let mut total_trades = 0;
        let mut profitable_count = 0;

        for symbol in SYMBOLS {
            if let Some(df) = data_cache.get(&symbol.to_string()) {
                let result = run_time_based_backtest(df, hold_bars, "bollinger")?;

                total_return += result.total_return_pct;
                total_trades += result.trades;
                if result.total_return_pct > 0.0 {
                    profitable_count += 1;
                }

                println!(
                    "{:12} | Return: {:>8.1}% | Trades: {:>4} | WinRate: {:>5.1}% | AvgWin: {:>6.2}% | AvgLoss: {:>6.2}%",
                    symbol,
                    result.total_return_pct,
                    result.trades,
                    result.win_rate * 100.0,
                    result.avg_win_pct,  // Already in percent
                    result.avg_loss_pct  // Already in percent
                );
            }
        }

        println!("{}", "-".repeat(80));
        println!(
            "PORTFOLIO   | Return: {:>8.1}% | Trades: {:>4} | Profitable: {}/{}",
            total_return,
            total_trades,
            profitable_count,
            SYMBOLS.len()
        );
    }

    // Now test with different signal types
    println!("\n\n{}", "=".repeat(80));
    println!("SIGNAL COMPARISON (hold = 3 bars)");
    println!("{}", "=".repeat(80));

    let hold_bars = 3;
    for signal_type in &["bollinger", "rsi", "random"] {
        println!("\n--- Signal: {} ---", signal_type);

        let mut total_return = 0.0;
        let mut total_trades = 0;

        for symbol in SYMBOLS {
            if let Some(df) = data_cache.get(&symbol.to_string()) {
                let result = run_time_based_backtest(df, hold_bars, signal_type)?;

                total_return += result.total_return_pct;
                total_trades += result.trades;

                println!(
                    "{:12} | Return: {:>8.1}% | Trades: {:>4} | WinRate: {:>5.1}%",
                    symbol,
                    result.total_return_pct,
                    result.trades,
                    result.win_rate * 100.0
                );
            }
        }

        println!(
            "TOTAL       | Return: {:>8.1}% | Trades: {:>4}",
            total_return, total_trades
        );
    }

    // Final summary: best configuration
    println!("\n\n{}", "=".repeat(80));
    println!("SUMMARY: Time-based exits with Bollinger signals");
    println!("{}", "=".repeat(80));
    println!("Hold=1:  Fast mean-reversion capture");
    println!("Hold=2-3: Moderate hold, balances reversion and momentum");
    println!("Hold=5+:  Longer-term reversion, may miss optimal exit");

    Ok(())
}

struct TimeBasedResult {
    total_return_pct: f64,
    trades: usize,
    win_rate: f64,
    avg_win_pct: f64,
    avg_loss_pct: f64,
}

fn run_time_based_backtest(
    df: &DataFrame,
    hold_bars: usize,
    signal_type: &str,
) -> Result<TimeBasedResult> {
    let close = df.column("close")?.f64()?;
    let n = close.len();

    // Generate signals based on type
    let signals = match signal_type {
        "bollinger" => generate_bollinger_signals(df)?,
        "rsi" => generate_rsi_signals(df)?,
        "random" => generate_random_signals(n, 42),
        _ => panic!("Unknown signal type: {}", signal_type),
    };

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut i = 20; // Start after indicator warmup

    while i < n.saturating_sub(hold_bars + 1) {
        let signal = signals[i];

        if signal != 0 && i + 1 + hold_bars < n {
            let entry_price = match close.get(i + 1) {
                Some(p) => p,
                None => {
                    i += 1;
                    continue;
                }
            };
            let exit_idx = i + 1 + hold_bars;
            let exit_price = match close.get(exit_idx) {
                Some(p) => p,
                None => {
                    i += 1;
                    continue;
                }
            };

            // Calculate return
            let return_pct = if signal > 0 {
                // Long
                (exit_price / entry_price - 1.0) * 100.0
            } else {
                // Short
                (entry_price / exit_price - 1.0) * 100.0
            };

            // Debug first few trades
            if trade_returns.len() < 3 && signal_type == "bollinger" {
                println!(
                    "    Trade {}: {} {:.2} -> {:.2} = {:.2}%",
                    trade_returns.len(),
                    if signal > 0 { "LONG" } else { "SHORT" },
                    entry_price,
                    exit_price,
                    return_pct
                );
            }

            // Deduct fees (0.2% round-trip = 0.1% entry + 0.1% exit)
            let return_after_fees = return_pct - (TAKER_FEE * 100.0 * 2.0);

            trade_returns.push(return_after_fees);

            // Skip ahead to avoid overlapping trades
            i = exit_idx;
        } else {
            i += 1;
        }
    }

    let total_return_pct: f64 = trade_returns.iter().sum();
    let trades = trade_returns.len();
    let wins: Vec<f64> = trade_returns
        .iter()
        .filter(|&&r| r > 0.0)
        .cloned()
        .collect();
    let losses: Vec<f64> = trade_returns
        .iter()
        .filter(|&&r| r <= 0.0)
        .cloned()
        .collect();

    let win_rate = if trades == 0 {
        0.0
    } else {
        wins.len() as f64 / trades as f64
    };

    let avg_win_pct = if wins.is_empty() {
        0.0
    } else {
        wins.iter().sum::<f64>() / wins.len() as f64
    };

    let avg_loss_pct = if losses.is_empty() {
        0.0
    } else {
        losses.iter().sum::<f64>() / losses.len() as f64
    };

    Ok(TimeBasedResult {
        total_return_pct,
        trades,
        win_rate,
        avg_win_pct,
        avg_loss_pct,
    })
}

fn generate_bollinger_signals(df: &DataFrame) -> Result<Vec<i8>> {
    let close = df.column("close")?.f64()?;
    let period = 20;
    let std_dev = 2.0;
    let n = close.len();

    let mut signals = vec![0i8; n];

    for i in period..n {
        // Calculate SMA and std for window
        let window: Vec<f64> = (i.saturating_sub(period - 1)..=i)
            .map(|j| close.get(j).unwrap())
            .collect();

        let sma = window.iter().sum::<f64>() / period as f64;
        let variance: f64 = window.iter().map(|x| (x - sma).powi(2)).sum::<f64>() / period as f64;
        let std = variance.sqrt();

        let upper = sma + std_dev * std;
        let lower = sma - std_dev * std;

        let price = close.get(i).unwrap();

        // Signal for NEXT bar (no look-ahead)
        if price > upper {
            signals[i] = -1; // Short on breakout above upper band
        } else if price < lower {
            signals[i] = 1; // Long on breakout below lower band
        }
    }

    Ok(signals)
}

fn generate_rsi_signals(df: &DataFrame) -> Result<Vec<i8>> {
    let close = df.column("close")?.f64()?;
    let period = 14;
    let oversold = 30.0;
    let overbought = 70.0;
    let n = close.len();

    let mut signals = vec![0i8; n];
    let mut gains: Vec<f64> = Vec::new();
    let mut losses: Vec<f64> = Vec::new();

    for i in 1..n {
        let change = close.get(i).unwrap() - close.get(i - 1).unwrap();
        if change > 0.0 {
            gains.push(change);
            losses.push(0.0);
        } else {
            gains.push(0.0);
            losses.push(-change);
        }
    }

    for i in period..n {
        let start_idx = i.saturating_sub(period);
        let end_idx = i.saturating_sub(1);

        if end_idx >= gains.len() || start_idx > end_idx {
            continue;
        }

        let avg_gain: f64 = gains[start_idx..=end_idx].iter().sum::<f64>() / period as f64;
        let avg_loss: f64 = losses[start_idx..=end_idx].iter().sum::<f64>() / period as f64;

        let rs = if avg_loss == 0.0 {
            100.0
        } else {
            avg_gain / avg_loss
        };
        let rsi = 100.0 - (100.0 / (1.0 + rs));

        // Signal for NEXT bar
        if rsi < oversold {
            signals[i] = 1; // Long when oversold
        } else if rsi > overbought {
            signals[i] = -1; // Short when overbought
        }
    }

    Ok(signals)
}

fn generate_random_signals(len: usize, seed: u64) -> Vec<i8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let mut signals = vec![0i8; len];
    let mut hasher = DefaultHasher::new();

    for i in 20..len {
        // Simple deterministic "random" based on index + seed
        hasher.write_u64(seed);
        hasher.write_u64(i as u64);
        let hash = hasher.finish();
        hasher = DefaultHasher::new();

        // 10% probability of signal (5% long, 5% short)
        let prob = hash % 100;
        if prob < 5 {
            signals[i] = 1; // Long
        } else if prob < 10 {
            signals[i] = -1; // Short
        }
    }

    signals
}

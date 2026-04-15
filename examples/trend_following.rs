//! Trend Following Strategy Test
//!
//! Previous sessions showed crypto daily bars are mean-reverting, but let's test
//! trend-following approaches which might work with time-based exits (no trailing stops).
//!
//! Hypothesis: If crypto has momentum on longer timeframes, trend-following with
//! time-based exits might capture a real edge.

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.001;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== TREND FOLLOWING STRATEGY TEST ===\n");
    println!("Testing trend-following approaches with time-based exits.\n");
    println!("Hypothesis: Momentum might work where mean-reversion failed.\n");

    let loader = DataLoader::new(None, None);

    // Load data
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

    // Test different trend-following approaches
    let strategies = [
        ("momentum_breakout", "Momentum Breakout (price > 20d high)"),
        ("ma_cross", "MA Cross (price > 50d MA)"),
        ("turtle", "Turtle (20d high/low breakout)"),
        ("macd", "MACD (signal line cross)"),
    ];

    let hold_periods = [5, 10, 14, 21]; // Longer holds for trend following

    for (strategy_key, strategy_name) in &strategies {
        println!("\n{}", "=".repeat(80));
        println!("STRATEGY: {}", strategy_name);
        println!("{}", "=".repeat(80));

        for &hold_bars in &hold_periods {
            println!("\n--- Hold: {} bars ---", hold_bars);

            let mut total_return = 0.0;
            let mut total_trades = 0;
            let mut profitable_count = 0;

            for symbol in SYMBOLS {
                if let Some(df) = data_cache.get(&symbol.to_string()) {
                    let result = run_trend_backtest(df, hold_bars, strategy_key)?;

                    total_return += result.total_return_pct;
                    total_trades += result.trades;
                    if result.total_return_pct > 0.0 {
                        profitable_count += 1;
                    }

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
                "  TOTAL     | Return: {:>8.1}% | Trades: {:>4} | Profitable: {}/{}",
                total_return,
                total_trades,
                profitable_count,
                SYMBOLS.len()
            );
        }
    }

    // Summary
    println!("\n\n{}", "=".repeat(80));
    println!("SUMMARY");
    println!("{}", "=".repeat(80));
    println!("If trend following works, we should see positive returns with longer holds.");
    println!("If not, the crypto daily edge is entirely from trailing stops (now invalid).");

    Ok(())
}

struct TrendResult {
    total_return_pct: f64,
    trades: usize,
    win_rate: f64,
}

fn run_trend_backtest(df: &DataFrame, hold_bars: usize, strategy: &str) -> Result<TrendResult> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();

    // Generate signals
    let signals = match strategy {
        "momentum_breakout" => generate_momentum_breakout(df)?,
        "ma_cross" => generate_ma_cross(df)?,
        "turtle" => generate_turtle(df)?,
        "macd" => generate_macd(df)?,
        _ => panic!("Unknown strategy: {}", strategy),
    };

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut i = 50; // Start after indicator warmup

    while i < n.saturating_sub(hold_bars + 1) {
        let signal = signals[i];

        if signal != 0 && i + 1 + hold_bars < n {
            let entry_price = match close.get(i + 1) {
                Some(p) if p > 0.0 => p,
                _ => {
                    i += 1;
                    continue;
                }
            };
            let exit_idx = i + 1 + hold_bars;
            let exit_price = match close.get(exit_idx) {
                Some(p) if p > 0.0 => p,
                _ => {
                    i += 1;
                    continue;
                }
            };

            // Calculate return
            let return_pct = if signal > 0 {
                (exit_price / entry_price - 1.0) * 100.0
            } else {
                (entry_price / exit_price - 1.0) * 100.0
            };

            // Deduct fees
            let return_after_fees = return_pct - (TAKER_FEE * 100.0 * 2.0);
            trade_returns.push(return_after_fees);

            i = exit_idx;
        } else {
            i += 1;
        }
    }

    let total_return_pct: f64 = trade_returns.iter().sum();
    let trades = trade_returns.len();
    let wins = trade_returns.iter().filter(|&&r| r > 0.0).count();

    Ok(TrendResult {
        total_return_pct,
        trades,
        win_rate: if trades == 0 {
            0.0
        } else {
            wins as f64 / trades as f64
        },
    })
}

// Price breaks above 20-day high -> go long
fn generate_momentum_breakout(df: &DataFrame) -> Result<Vec<i8>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let n = close.len();
    let period = 20;

    let mut signals = vec![0i8; n];

    for i in period..n {
        // Find highest high of last 20 days
        let highest: f64 = (i.saturating_sub(period)..i)
            .map(|j| high.get(j).unwrap_or(0.0))
            .fold(0.0, f64::max);

        let price = close.get(i).unwrap_or(0.0);

        if price > highest && price > 0.0 {
            signals[i] = 1; // Long on breakout
        }
    }

    Ok(signals)
}

// Price crosses above 50-day MA -> go long
fn generate_ma_cross(df: &DataFrame) -> Result<Vec<i8>> {
    let close = df.column("close")?.f64()?;
    let n = close.len();
    let period = 50;

    let mut signals = vec![0i8; n];

    for i in period..n {
        let ma: f64 = (i.saturating_sub(period)..i)
            .map(|j| close.get(j).unwrap_or(0.0))
            .sum::<f64>()
            / period as f64;

        let prev_ma: f64 = (i.saturating_sub(period + 1)..i.saturating_sub(1))
            .map(|j| close.get(j).unwrap_or(0.0))
            .sum::<f64>()
            / period as f64;

        let price = close.get(i).unwrap_or(0.0);
        let prev_price = close.get(i.saturating_sub(1)).unwrap_or(0.0);

        // Cross up
        if prev_price <= prev_ma && price > ma && price > 0.0 {
            signals[i] = 1;
        }
    }

    Ok(signals)
}

// Turtle trading: 20-day high breakout (long) or 20-day low breakout (short)
fn generate_turtle(df: &DataFrame) -> Result<Vec<i8>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();
    let period = 20;

    let mut signals = vec![0i8; n];

    for i in period..n {
        let highest: f64 = (i.saturating_sub(period)..i)
            .map(|j| high.get(j).unwrap_or(0.0))
            .fold(0.0, f64::max);

        let lowest: f64 = (i.saturating_sub(period)..i)
            .map(|j| low.get(j).unwrap_or(f64::MAX))
            .fold(f64::MAX, f64::min);

        let price = close.get(i).unwrap_or(0.0);

        if price >= highest && price > 0.0 {
            signals[i] = 1; // Long on new high
        } else if price <= lowest && price > 0.0 {
            signals[i] = -1; // Short on new low
        }
    }

    Ok(signals)
}

// MACD: buy when MACD crosses above signal line
fn generate_macd(df: &DataFrame) -> Result<Vec<i8>> {
    let close = df.column("close")?.f64()?;
    let n = close.len();

    // Calculate MACD (12, 26, 9)
    let ema12 = calculate_ema(&close, 12);
    let ema26 = calculate_ema(&close, 26);

    let mut macd_line: Vec<f64> = Vec::with_capacity(n);
    for i in 0..n {
        let m12 = *ema12.get(i).unwrap_or(&0.0);
        let m26 = *ema26.get(i).unwrap_or(&0.0);
        macd_line.push(m12 - m26);
    }

    let signal_line = calculate_ema_vec(&macd_line, 9);

    let mut signals = vec![0i8; n];

    for i in 26..n {
        let macd = macd_line.get(i).copied().unwrap_or(0.0);
        let prev_macd = macd_line.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        let signal = signal_line.get(i).copied().unwrap_or(0.0);
        let prev_signal = signal_line.get(i.saturating_sub(1)).copied().unwrap_or(0.0);

        // Cross up
        if prev_macd <= prev_signal && macd > signal {
            signals[i] = 1;
        }
    }

    Ok(signals)
}

fn calculate_ema(close: &ChunkedArray<Float64Type>, period: usize) -> Vec<f64> {
    let n = close.len();
    let multiplier = 2.0 / (period as f64 + 1.0);
    let mut ema = Vec::with_capacity(n);

    // First EMA is SMA
    let mut sum = 0.0;
    for i in 0..n.min(period) {
        let val = close.get(i).unwrap_or(0.0);
        sum += val;
        ema.push(0.0);
    }

    if n >= period {
        ema[period - 1] = sum / period as f64;

        for i in period..n {
            let val = close.get(i).unwrap_or(0.0);
            let prev_ema = ema[i - 1];
            ema.push((val - prev_ema) * multiplier + prev_ema);
        }
    }

    ema
}

fn calculate_ema_vec(data: &[f64], period: usize) -> Vec<f64> {
    let n = data.len();
    let multiplier = 2.0 / (period as f64 + 1.0);
    let mut ema = vec![0.0; n];

    if n < period {
        return ema;
    }

    // First EMA is SMA
    let sum: f64 = data[..period].iter().sum();
    ema[period - 1] = sum / period as f64;

    for i in period..n {
        let prev_ema = ema[i - 1];
        ema[i] = (data[i] - prev_ema) * multiplier + prev_ema;
    }

    ema
}

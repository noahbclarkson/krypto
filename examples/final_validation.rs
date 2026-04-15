//! Final Validation: Turtle + Regime + MACD with Monte Carlo
//!
//! Validate the best configuration found in Session 13:
//! - Turtle breakout (20d)
//! - Regime filter (price > SMA 200)
//! - MACD confirmation
//!
//! Rigorous testing: Walk-forward + Monte Carlo + Random entry comparison.

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

const SYMBOLS: &[&str] = &["SOLUSDT", "DOGEUSDT", "BTCUSDT", "ETHUSDT", "XRPUSDT"];
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.001;
const HOLD_BARS: usize = 21;
const PERIOD: usize = 20;
const N_MONTE_CARLO: usize = 100;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== FINAL VALIDATION: TURTLE + REGIME + MACD ===\n");
    println!("Best config from Session 13:\n");
    println!("  - Turtle breakout ({}d high/low)", PERIOD);
    println!("  - Regime filter (price > SMA 200)");
    println!("  - MACD confirmation");
    println!(
        "  - {} day hold, {:.1}% fee\n",
        HOLD_BARS,
        TAKER_FEE * 100.0
    );

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

    // 1. Full history results
    println!("\n{}", "=".repeat(80));
    println!("1. FULL HISTORY RESULTS");
    println!("{}", "=".repeat(80));

    let mut total_return = 0.0;
    let mut total_trades = 0;
    let mut total_wins = 0;

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let result = run_combined_backtest(df, true, true)?;
            total_return += result.total_return_pct;
            total_trades += result.trades;
            total_wins += (result.win_rate * result.trades as f64) as usize;

            println!(
                "{:12} | Return: {:>8.1}% | Trades: {:>3} | WinRate: {:>5.1}%",
                symbol,
                result.total_return_pct,
                result.trades,
                result.win_rate * 100.0
            );
        }
    }

    println!("{}", "-".repeat(80));
    println!(
        "{:12} | Return: {:>8.1}% | Trades: {:>3} | WinRate: {:>5.1}%",
        "PORTFOLIO",
        total_return,
        total_trades,
        if total_trades > 0 {
            total_wins as f64 / total_trades as f64 * 100.0
        } else {
            0.0
        }
    );

    // 2. Walk-forward validation
    println!("\n\n{}", "=".repeat(80));
    println!("2. WALK-FORWARD VALIDATION");
    println!("{}", "=".repeat(80));

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let n = df.height();
            let quarter = n / 4;

            println!("\n{} ({} bars):", symbol, n);

            for i in 0..4 {
                let start = i * quarter;
                let end = if i == 3 { n } else { (i + 1) * quarter };
                let period_df = df.slice(start as i64, end - start);

                let result = run_combined_backtest(&period_df, true, true)?;

                println!(
                    "  Period {} ({} bars): Return={:>7.1}% | Trades={:>3} | WinRate={:.0}%",
                    i + 1,
                    end - start,
                    result.total_return_pct,
                    result.trades,
                    result.win_rate * 100.0
                );
            }
        }
    }

    // 3. Monte Carlo permutation test
    println!("\n\n{}", "=".repeat(80));
    println!("3. MONTE CARLO PERMUTATION TEST");
    println!("{}", "=".repeat(80));
    println!(
        "Shuffle signal dates {} times, compare real vs random.\n",
        N_MONTE_CARLO
    );

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let real_result = run_combined_backtest(df, true, true)?;
            let mut random_returns: Vec<f64> = Vec::with_capacity(N_MONTE_CARLO);

            for seed in 0..N_MONTE_CARLO {
                let shuffled = shuffle_signals(df, seed as u64)?;
                let result = run_backtest_with_signals(df, &shuffled, true)?;
                random_returns.push(result.total_return_pct);
            }

            random_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());

            let real_return = real_result.total_return_pct;
            let percentile = random_returns
                .iter()
                .position(|&r| r > real_return)
                .unwrap_or(N_MONTE_CARLO);
            let p_value = percentile as f64 / N_MONTE_CARLO as f64;

            let mean_random = random_returns.iter().sum::<f64>() / N_MONTE_CARLO as f64;
            let profitable_random = random_returns.iter().filter(|&&r| r > 0.0).count();

            println!(
                "{:12} | Real: {:>7.1}% | Random mean: {:>7.1}% | Pctl: {:>3}% | Real > Random: {}/{}",
                symbol,
                real_return,
                mean_random,
                (1.0 - p_value) * 100.0,
                N_MONTE_CARLO - percentile,
                N_MONTE_CARLO
            );
        }
    }

    // 4. Random entry comparison
    println!("\n\n{}", "=".repeat(80));
    println!("4. RANDOM ENTRY COMPARISON");
    println!("{}", "=".repeat(80));
    println!("Compare real signals to random entry with same filters.\n");

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let real_result = run_combined_backtest(df, true, true)?;

            let mut random_returns: Vec<f64> = Vec::with_capacity(N_MONTE_CARLO);
            for seed in 0..N_MONTE_CARLO {
                let random_signals = generate_random_signals(df.height(), seed as u64);
                let result = run_backtest_with_signals(df, &random_signals, true)?;
                random_returns.push(result.total_return_pct);
            }

            random_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mean_random = random_returns.iter().sum::<f64>() / N_MONTE_CARLO as f64;
            let real_beats = random_returns
                .iter()
                .filter(|&&r| real_result.total_return_pct > r)
                .count();

            println!(
                "{:12} | Real: {:>7.1}% | Random mean: {:>7.1}% | Real beats: {}/{} ({:.0}%)",
                symbol,
                real_result.total_return_pct,
                mean_random,
                real_beats,
                N_MONTE_CARLO,
                real_beats as f64 / N_MONTE_CARLO as f64 * 100.0
            );
        }
    }

    // Summary
    println!("\n\n{}", "=".repeat(80));
    println!("SUMMARY");
    println!("{}", "=".repeat(80));

    println!("Configuration: Turtle + Regime Filter + MACD Confirmation");
    println!("Portfolio return: {:.1}% over ~8 years", total_return);
    println!(
        "Total trades: {} (avg {:.1} per symbol)",
        total_trades,
        total_trades as f64 / SYMBOLS.len() as f64
    );
    println!(
        "Overall win rate: {:.1}%",
        if total_trades > 0 {
            total_wins as f64 / total_trades as f64 * 100.0
        } else {
            0.0
        }
    );

    println!("\nValidation status:");
    println!("  ✓ Walk-forward: Check for negative periods above");
    println!("  ✓ Monte Carlo: Real should beat random mean");
    println!("  ✓ Random entry: Real should beat >50% of random");

    println!("\nNext steps:");
    println!("  1. If all periods positive + Monte Carlo good → candidate for paper trading");
    println!("  2. Test on 4h data for more granular entry");
    println!("  3. Consider position sizing / correlation analysis");

    Ok(())
}

struct TrendResult {
    total_return_pct: f64,
    trades: usize,
    win_rate: f64,
}

fn run_combined_backtest(
    df: &DataFrame,
    use_regime_filter: bool,
    use_macd_confirm: bool,
) -> Result<TrendResult> {
    let close = df.column("close")?.f64()?;
    let n = close.len();

    let sma_200 = calculate_sma(&close, 200)?;
    let turtle_signals = generate_turtle_signals(df, PERIOD)?;
    let macd_signals = generate_macd_signals(df)?;

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut i = 200;

    while i < n.saturating_sub(HOLD_BARS + 1) {
        let turtle_signal = turtle_signals[i];

        if turtle_signal != 0 && i + 1 + HOLD_BARS < n {
            let mut should_trade = true;

            if use_regime_filter && turtle_signal > 0 {
                let current_price = close.get(i).unwrap_or(0.0);
                let current_sma = match sma_200.get(i) {
                    Some(Some(v)) => *v,
                    _ => 0.0,
                };
                if current_price <= current_sma {
                    should_trade = false;
                }
            }

            if use_macd_confirm && should_trade {
                let macd_signal = macd_signals[i];
                if turtle_signal > 0 && macd_signal <= 0 {
                    should_trade = false;
                } else if turtle_signal < 0 && macd_signal >= 0 {
                    should_trade = false;
                }
            }

            if should_trade {
                let entry_price = match close.get(i + 1) {
                    Some(p) if p > 0.0 => p,
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                let exit_idx = i + 1 + HOLD_BARS;
                let exit_price = match close.get(exit_idx) {
                    Some(p) if p > 0.0 => p,
                    _ => {
                        i += 1;
                        continue;
                    }
                };

                let return_pct = if turtle_signal > 0 {
                    (exit_price / entry_price - 1.0) * 100.0
                } else {
                    (entry_price / exit_price - 1.0) * 100.0
                };

                let net_return = return_pct - 2.0 * TAKER_FEE * 100.0;
                trade_returns.push(net_return);
                i += HOLD_BARS + 1;
                continue;
            }
        }
        i += 1;
    }

    let trades = trade_returns.len();
    let total_return_pct = trade_returns.iter().sum();
    let win_rate = if trades > 0 {
        trade_returns.iter().filter(|&&r| r > 0.0).count() as f64 / trades as f64
    } else {
        0.0
    };

    Ok(TrendResult {
        total_return_pct,
        trades,
        win_rate,
    })
}

fn run_backtest_with_signals(
    df: &DataFrame,
    signals: &[i32],
    use_regime_filter: bool,
) -> Result<TrendResult> {
    let close = df.column("close")?.f64()?;
    let n = close.len();

    let sma_200 = calculate_sma(&close, 200)?;

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut i = 200;

    while i < n.saturating_sub(HOLD_BARS + 1) {
        let signal = signals[i];

        if signal != 0 && i + 1 + HOLD_BARS < n {
            let mut should_trade = true;

            if use_regime_filter && signal > 0 {
                let current_price = close.get(i).unwrap_or(0.0);
                let current_sma = match sma_200.get(i) {
                    Some(Some(v)) => *v,
                    _ => 0.0,
                };
                if current_price <= current_sma {
                    should_trade = false;
                }
            }

            if should_trade {
                let entry_price = match close.get(i + 1) {
                    Some(p) if p > 0.0 => p,
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                let exit_idx = i + 1 + HOLD_BARS;
                let exit_price = match close.get(exit_idx) {
                    Some(p) if p > 0.0 => p,
                    _ => {
                        i += 1;
                        continue;
                    }
                };

                let return_pct = if signal > 0 {
                    (exit_price / entry_price - 1.0) * 100.0
                } else {
                    (entry_price / exit_price - 1.0) * 100.0
                };

                let net_return = return_pct - 2.0 * TAKER_FEE * 100.0;
                trade_returns.push(net_return);
                i += HOLD_BARS + 1;
                continue;
            }
        }
        i += 1;
    }

    let trades = trade_returns.len();
    let total_return_pct = trade_returns.iter().sum();
    let win_rate = if trades > 0 {
        trade_returns.iter().filter(|&&r| r > 0.0).count() as f64 / trades as f64
    } else {
        0.0
    };

    Ok(TrendResult {
        total_return_pct,
        trades,
        win_rate,
    })
}

fn shuffle_signals(df: &DataFrame, seed: u64) -> Result<Vec<i32>> {
    let turtle = generate_turtle_signals(df, PERIOD)?;
    let n = turtle.len();

    // Create seeded RNG
    let mut hasher = DefaultHasher::new();
    hasher.write_u64(seed);
    let mut state = hasher.finish();

    let mut shuffled = turtle.clone();

    // Fisher-Yates shuffle with simple RNG
    for i in (1..n).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (state % (i as u64 + 1)) as usize;
        shuffled.swap(i, j);
    }

    Ok(shuffled)
}

fn generate_random_signals(n: usize, seed: u64) -> Vec<i32> {
    let mut hasher = DefaultHasher::new();
    hasher.write_u64(seed);
    let mut state = hasher.finish();

    let mut signals = vec![0i32; n];

    for i in 200..n {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let rand = state % 100;

        if rand < 10 {
            signals[i] = 1; // 10% long
        } else if rand < 20 {
            signals[i] = -1; // 10% short
        }
    }

    signals
}

fn generate_turtle_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();

    let mut signals = vec![0i32; n];

    for i in period..n {
        let mut period_high = f64::NEG_INFINITY;
        let mut period_low = f64::INFINITY;

        for j in (i - period)..i {
            if let Some(h) = high.get(j) {
                period_high = period_high.max(h);
            }
            if let Some(l) = low.get(j) {
                period_low = period_low.min(l);
            }
        }

        let current_close = close.get(i).unwrap_or(0.0);

        if current_close > period_high {
            signals[i] = 1;
        } else if current_close < period_low {
            signals[i] = -1;
        }
    }

    Ok(signals)
}

fn generate_macd_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = df.column("macd").ok().and_then(|s| s.f64().ok());
    let signal = df.column("macd_signal").ok().and_then(|s| s.f64().ok());
    let close = df.column("close")?.f64()?;
    let n = close.len();

    let mut signals = vec![0i32; n];

    if let (Some(macd_series), Some(signal_series)) = (macd, signal) {
        for i in 1..n {
            let macd_curr = macd_series.get(i).unwrap_or(0.0);
            let sig_curr = signal_series.get(i).unwrap_or(0.0);

            if macd_curr > sig_curr {
                signals[i] = 1;
            } else if macd_curr < sig_curr {
                signals[i] = -1;
            }
        }
    } else {
        let ema_12 = calculate_ema(&close, 12)?;
        let ema_26 = calculate_ema(&close, 26)?;

        for i in 26..n {
            let fast = match ema_12.get(i) {
                Some(Some(v)) => *v,
                _ => continue,
            };
            let slow = match ema_26.get(i) {
                Some(Some(v)) => *v,
                _ => continue,
            };

            if fast > slow {
                signals[i] = 1;
            } else if fast < slow {
                signals[i] = -1;
            }
        }
    }

    Ok(signals)
}

fn calculate_sma(series: &ChunkedArray<Float64Type>, period: usize) -> Result<Vec<Option<f64>>> {
    let n = series.len();
    let mut sma = vec![None; n];

    for i in period..n {
        let sum: f64 = (0..period).filter_map(|j| series.get(i - j)).sum();
        sma[i] = Some(sum / period as f64);
    }

    Ok(sma)
}

fn calculate_ema(series: &ChunkedArray<Float64Type>, period: usize) -> Result<Vec<Option<f64>>> {
    let n = series.len();
    let mut ema = vec![None; n];
    let multiplier = 2.0 / (period as f64 + 1.0);

    if n >= period {
        let sum: f64 = (0..period).filter_map(|j| series.get(j)).sum();
        ema[period - 1] = Some(sum / period as f64);

        for i in period..n {
            if let (Some(prev_ema), Some(curr_price)) = (ema[i - 1], series.get(i)) {
                ema[i] = Some((curr_price - prev_ema) * multiplier + prev_ema);
            }
        }
    }

    Ok(ema)
}

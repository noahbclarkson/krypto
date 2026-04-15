//! Combined Signal Test: Turtle + MACD Confirmation
//!
//! PLAN.md Session 13: Regime filter helps (+215.6% improvement).
//! Now test if combining multiple trend signals improves edge further.
//!
//! Hypothesis: Only trade Turtle breakouts when MACD confirms trend direction.

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;

const SYMBOLS: &[&str] = &["SOLUSDT", "DOGEUSDT", "BTCUSDT", "ETHUSDT", "XRPUSDT"];
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.001;
const HOLD_BARS: usize = 21;
const PERIOD: usize = 20;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== COMBINED SIGNAL TEST: TURTLE + MACD ===\n");
    println!("Hypothesis: Only trade Turtle breakouts when MACD confirms direction.\n");

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

    // Compare strategies
    println!("\n{}", "=".repeat(100));
    println!("COMPARISON: Turtle vs Regime Filter vs MACD Confirmation vs Combined");
    println!("{}", "=".repeat(100));
    println!(
        "{:12} | {:^17} | {:^17} | {:^17} | {:^17}",
        "", "TURTLE ONLY", "+ REGIME FILTER", "+ MACD CONF", "+ REGIME + MACD"
    );
    println!(
        "{:12} | {:>7} {:>4} {:>4} | {:>7} {:>4} {:>4} | {:>7} {:>4} {:>4} | {:>7} {:>4} {:>4}",
        "", "Ret%", "Trd", "Win", "Ret%", "Trd", "Win", "Ret%", "Trd", "Win", "Ret%", "Trd", "Win"
    );
    println!("{}", "-".repeat(100));

    let mut totals = vec![0.0f64; 4];
    let mut trade_totals = vec![0usize; 4];

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let r0 = run_combined_backtest(df, false, false)?; // Turtle only
            let r1 = run_combined_backtest(df, true, false)?; // + Regime
            let r2 = run_combined_backtest(df, false, true)?; // + MACD
            let r3 = run_combined_backtest(df, true, true)?; // + Both

            for (i, r) in [&r0, &r1, &r2, &r3].iter().enumerate() {
                totals[i] += r.total_return_pct;
                trade_totals[i] += r.trades;
            }

            println!(
                "{:12} | {:>7.1} {:>4} {:>3.0}% | {:>7.1} {:>4} {:>3.0}% | {:>7.1} {:>4} {:>3.0}% | {:>7.1} {:>4} {:>3.0}%",
                symbol,
                r0.total_return_pct, r0.trades, r0.win_rate * 100.0,
                r1.total_return_pct, r1.trades, r1.win_rate * 100.0,
                r2.total_return_pct, r2.trades, r2.win_rate * 100.0,
                r3.total_return_pct, r3.trades, r3.win_rate * 100.0
            );
        }
    }

    println!("{}", "-".repeat(100));
    println!(
        "{:12} | {:>7.1} {:>4} {:>4} | {:>7.1} {:>4} {:>4} | {:>7.1} {:>4} {:>4} | {:>7.1} {:>4} {:>4}",
        "TOTAL",
        totals[0], trade_totals[0], "",
        totals[1], trade_totals[1], "",
        totals[2], trade_totals[2], "",
        totals[3], trade_totals[3], ""
    );

    // Show improvement deltas
    println!("\n{}", "=".repeat(100));
    println!("IMPROVEMENT DELTAS");
    println!("{}", "=".repeat(100));
    println!(
        "{:12} | {:^26} | {:^26} | {:^26}",
        "", "Regime Filter", "MACD Confirmation", "Both Filters"
    );
    println!(
        "{:12} | {:>10} {:>7} {:>7} | {:>10} {:>7} {:>7} | {:>10} {:>7} {:>7}",
        "", "RetΔ", "TrdΔ%", "Better?", "RetΔ", "TrdΔ%", "Better?", "RetΔ", "TrdΔ%", "Better?"
    );
    println!("{}", "-".repeat(100));

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let r0 = run_combined_backtest(df, false, false)?;
            let r1 = run_combined_backtest(df, true, false)?;
            let r2 = run_combined_backtest(df, false, true)?;
            let r3 = run_combined_backtest(df, true, true)?;

            let ret0 = r0.total_return_pct;
            let trd0 = r0.trades as f64;

            let (d1_ret, d1_trd, b1) = (
                r1.total_return_pct - ret0,
                (1.0 - r1.trades as f64 / trd0) * 100.0,
                r1.total_return_pct > ret0,
            );
            let (d2_ret, d2_trd, b2) = (
                r2.total_return_pct - ret0,
                (1.0 - r2.trades as f64 / trd0) * 100.0,
                r2.total_return_pct > ret0,
            );
            let (d3_ret, d3_trd, b3) = (
                r3.total_return_pct - ret0,
                (1.0 - r3.trades as f64 / trd0) * 100.0,
                r3.total_return_pct > ret0,
            );

            println!(
                "{:12} | {:>+10.1} {:>+6.0}% {:>7} | {:>+10.1} {:>+6.0}% {:>7} | {:>+10.1} {:>+6.0}% {:>7}",
                symbol,
                d1_ret, d1_trd, if b1 { "✓" } else { "✗" },
                d2_ret, d2_trd, if b2 { "✓" } else { "✗" },
                d3_ret, d3_trd, if b3 { "✓" } else { "✗" }
            );
        }
    }

    // Summary
    println!("\n\n{}", "=".repeat(100));
    println!("SUMMARY");
    println!("{}", "=".repeat(100));

    let best_idx = totals
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(0);

    let names = [
        "Turtle only",
        "+ Regime filter",
        "+ MACD confirmation",
        "+ Both filters",
    ];
    let improvements: Vec<f64> = totals.iter().map(|&t| t - totals[0]).collect();

    println!("Portfolio returns:");
    for (i, (((name, total), trades), imp)) in names
        .iter()
        .zip(totals.iter())
        .zip(trade_totals.iter())
        .zip(improvements.iter())
        .enumerate()
    {
        let marker = if i == best_idx { "← BEST" } else { "" };
        println!(
            "  {:25} | Return: {:>8.1}% | Trades: {:>4} | Δ: {:>+8.1}% {}",
            name, total, trades, imp, marker
        );
    }

    println!(
        "\nRecommendation: Use {} for live testing.",
        names[best_idx]
    );

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

    // Calculate SMA 200 for regime filter
    let sma_200 = calculate_sma(&close, 200)?;

    // Generate Turtle signals
    let turtle_signals = generate_turtle_signals(df, PERIOD)?;

    // Generate MACD signals
    let macd_signals = generate_macd_signals(df)?;

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut i = 200; // Start after SMA 200 warmup

    while i < n.saturating_sub(HOLD_BARS + 1) {
        let turtle_signal = turtle_signals[i];

        if turtle_signal != 0 && i + 1 + HOLD_BARS < n {
            let mut should_trade = true;

            // Regime filter: only trade longs when price > SMA 200
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

            // MACD confirmation: only trade if MACD agrees with direction
            if use_macd_confirm && should_trade {
                let macd_signal = macd_signals[i];
                if turtle_signal > 0 && macd_signal <= 0 {
                    should_trade = false; // Long but MACD not bullish
                } else if turtle_signal < 0 && macd_signal >= 0 {
                    should_trade = false; // Short but MACD not bearish
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

                // Calculate return
                let return_pct = if turtle_signal > 0 {
                    (exit_price / entry_price - 1.0) * 100.0
                } else {
                    (entry_price / exit_price - 1.0) * 100.0
                };

                // Deduct fees
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
    // Use pre-computed MACD if available, otherwise calculate
    let macd = df.column("macd").ok().and_then(|s| s.f64().ok());
    let signal = df.column("macd_signal").ok().and_then(|s| s.f64().ok());

    let close = df.column("close")?.f64()?;
    let n = close.len();

    let mut signals = vec![0i32; n];

    if let (Some(macd_series), Some(signal_series)) = (macd, signal) {
        for i in 1..n {
            let macd_curr = macd_series.get(i).unwrap_or(0.0);
            let macd_prev = macd_series.get(i - 1).unwrap_or(0.0);
            let sig_curr = signal_series.get(i).unwrap_or(0.0);
            let sig_prev = signal_series.get(i - 1).unwrap_or(0.0);

            // MACD bullish when MACD > signal
            if macd_curr > sig_curr {
                signals[i] = 1;
            } else if macd_curr < sig_curr {
                signals[i] = -1;
            }
        }
    } else {
        // Fallback: simple EMA crossover
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

    // Start with SMA for first value
    if n >= period {
        let sum: f64 = (0..period).filter_map(|j| series.get(j)).sum();
        ema[period - 1] = Some(sum / period as f64);

        // Then EMA
        for i in period..n {
            if let (Some(prev_ema), Some(curr_price)) = (ema[i - 1], series.get(i)) {
                ema[i] = Some((curr_price - prev_ema) * multiplier + prev_ema);
            }
        }
    }

    Ok(ema)
}

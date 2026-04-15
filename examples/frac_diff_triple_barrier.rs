//! Fractional differencing + triple barrier exploration.
//!
//! Tests whether:
//! 1. Fractionally differenced prices (d=0.4-0.6) preserve memory better than first differences
//! 2. Triple barrier labels can identify regime-specific entry points
//!
//! Hypothesis: FracDiff preserves long-term memory while achieving stationarity,
//! which could improve mean-reversion signal quality.
//!
//! Usage:
//!   cargo run --profile sweep --example frac_diff_triple_barrier

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::engine::Backtester,
    data::loader::DataLoader,
    features::frac_diff::frac_diff_ffd,
    features::indicators::FeatureEngine,
    labeling::triple_barrier::{apply_triple_barrier, BarrierEvent},
};
use polars::prelude::*;

const CAPITAL: f64 = 2_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30;
const INTERVAL: &str = "1d";
const CANDLES: u32 = 925;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

// Triple barrier params
const T_HORIZON: usize = 20; // Max bars to hold
const PT_SL: (f64, f64) = (2.0, 1.0); // Profit take = 2×ATR, Stop = 1×ATR

fn compute_atr_stop(df: &DataFrame, atr_mult: f64) -> f64 {
    let mid = df.height() / 2;
    let atr = df
        .column("atr")
        .ok()
        .and_then(|s| s.f64().ok().and_then(|ca| ca.get(mid)))
        .unwrap_or(0.0);
    let close = df
        .column("close")
        .ok()
        .and_then(|s| s.f64().ok().and_then(|ca| ca.get(mid)))
        .unwrap_or(1.0);
    if close > 0.0 {
        (atr * atr_mult / close).clamp(0.005, 0.30)
    } else {
        0.05
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  FRACTIONAL DIFFERENTIATION + TRIPLE BARRIER"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());
    println!();
    println!("  Testing: Can FracDiff preserve memory for better signals?");
    println!("  - d=0.0: Original series (non-stationary)");
    println!("  - d=1.0: First difference (stationary, no memory)");
    println!("  - d=0.4-0.6: Fractional (stationary, preserves memory)");
    println!();

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    // Test different d values for fractional differencing
    let d_values: Vec<f64> = vec![0.0, 0.3, 0.4, 0.5, 0.6, 0.7, 1.0];

    println!("{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  Part 1: FracDiff Signal Quality Test"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    for symbol in SYMBOLS {
        print!("\n  {}:\n", symbol.bright_white().bold());

        let raw = match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
            Ok(d) => d,
            Err(_) => continue,
        };
        let df = FeatureEngine::add_technicals(&raw, None)?;
        let close = df.column("close")?.f64()?;
        let close_series = close.clone().into_series();

        let stop = compute_atr_stop(&df, ATR_MULT);
        let strategy = registry.create("bollinger_reversion").unwrap();
        let baseline_signals = strategy.predict(&df)?;

        // Baseline BollingerReversion
        let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
        let baseline_result = bt.run(&df, &baseline_signals, stop, 0.0)?;

        println!(
            "    Baseline Bollinger: {:.1}% (Sharpe {:.2})",
            baseline_result.total_return_pct, baseline_result.sharpe_ratio
        );

        // Test different d values
        for d in &d_values {
            if *d == 0.0 {
                continue; // Skip d=0 (original, already tested)
            }

            // Apply fractional differencing to close prices
            let frac_diff_series = frac_diff_ffd(&close_series, *d, 100)?;
            let frac_diff = frac_diff_series.f64()?;

            // Create simple mean-reversion signals on frac-diff series
            // If frac_diff crosses below -threshold, go long (expect reversion)
            // If frac_diff crosses above +threshold, go short
            let threshold = 0.02; // 2% deviation
            let mut signals = vec![0.0f64; frac_diff.len()];

            for i in 1..frac_diff.len() {
                let prev = frac_diff.get(i - 1).unwrap_or(0.0);
                let curr = frac_diff.get(i).unwrap_or(0.0);

                if curr < -threshold && prev >= -threshold {
                    signals[i] = 1.0; // Long signal
                } else if curr > threshold && prev <= threshold {
                    signals[i] = -1.0; // Short signal
                }
            }

            let signal_series = Series::new("signal".into(), signals.clone());

            // Only test if we have enough signals
            let sig_count = signals.iter().filter(|&&s| s.abs() > 0.01).count();
            if sig_count < 10 {
                println!("    d={:.1}: SKIP (only {} signals)", d, sig_count);
                continue;
            }

            let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
            let result = match bt.run(&df, &signal_series, stop, 0.0) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let ret_str = if result.total_return_pct > 0.0 {
                format!("{:+.1}%", result.total_return_pct).green()
            } else {
                format!("{:+.1}%", result.total_return_pct).red()
            };

            println!(
                "    d={:.1}: {} (Sharpe {:.2}, {} trades)",
                d, ret_str, result.sharpe_ratio, result.total_trades
            );
        }
    }

    // Part 2: Triple Barrier Labeling
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  Part 2: Triple Barrier Label Analysis"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    for symbol in SYMBOLS {
        print!("\n  {}:\n", symbol.bright_white().bold());

        let raw = match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
            Ok(d) => d,
            Err(_) => continue,
        };
        let df = FeatureEngine::add_technicals(&raw, None)?;

        // Get ATR for volatility-scaled barriers
        let atr = df.column("atr")?.f64()?.clone().into_series();

        // Apply triple barrier
        let labels = apply_triple_barrier(&df, &atr, T_HORIZON, PT_SL)?;

        // Count outcomes
        let mut hit_upper = 0;
        let mut hit_lower = 0;
        let mut timeout = 0;

        for &label in &labels {
            match label {
                x if x == BarrierEvent::HitUpper as i8 => hit_upper += 1,
                x if x == BarrierEvent::HitLower as i8 => hit_lower += 1,
                _ => timeout += 1,
            }
        }

        let total = labels.len();
        let upper_pct = hit_upper as f64 / total as f64 * 100.0;
        let lower_pct = hit_lower as f64 / total as f64 * 100.0;
        let timeout_pct = timeout as f64 / total as f64 * 100.0;

        println!("    Total bars: {}", total);
        println!("    Hit Upper (TP):  {:5} ({:.1}%)", hit_upper, upper_pct);
        println!("    Hit Lower (SL):  {:5} ({:.1}%)", hit_lower, lower_pct);
        println!("    Timeout:         {:5} ({:.1}%)", timeout, timeout_pct);

        // Check if labels have predictive power for Bollinger signals
        let strategy = registry.create("bollinger_reversion").unwrap();
        let signals = strategy.predict(&df)?;

        let signal_arr = signals.f64()?;
        let mut signal_upper = 0;
        let mut signal_lower = 0;
        let mut signal_timeout = 0;

        for i in 0..labels.len().min(signal_arr.len()) {
            let sig = signal_arr.get(i).unwrap_or(0.0);
            if sig.abs() > 0.01 {
                match labels[i] {
                    x if x == BarrierEvent::HitUpper as i8 => signal_upper += 1,
                    x if x == BarrierEvent::HitLower as i8 => signal_lower += 1,
                    _ => signal_timeout += 1,
                }
            }
        }

        let signal_total = signal_upper + signal_lower + signal_timeout;
        if signal_total > 0 {
            println!("    Bollinger signal outcomes:");
            println!(
                "      TP hit:   {:4} ({:.1}%)",
                signal_upper,
                signal_upper as f64 / signal_total as f64 * 100.0
            );
            println!(
                "      SL hit:   {:4} ({:.1}%)",
                signal_lower,
                signal_lower as f64 / signal_total as f64 * 100.0
            );
            println!(
                "      Timeout:  {:4} ({:.1}%)",
                signal_timeout,
                signal_timeout as f64 / signal_total as f64 * 100.0
            );
        }
    }

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  SUMMARY".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();
    println!("  FracDiff: Tests whether fractional differencing preserves");
    println!("  memory better than first differences for signal generation.");
    println!();
    println!("  Triple Barrier: Labels each bar by which barrier it hits first.");
    println!("  Useful for: filtering signals by predicted outcome, ML training.");
    println!();

    Ok(())
}

//! Volume filter research for BollingerReversion 1d.
//!
//! Hypothesis: Bollinger breakouts on LOW volume are false signals (momentum continuation),
//! while breakouts on HIGH volume are stronger mean-reversion signals.
//!
//! Tests:
//! 1. No filter (baseline)
//! 2. Volume > 20-day SMA (above average volume)
//! 3. Volume < 20-day SMA (below average volume)
//! 4. Volume > 1.5× SMA (high volume)
//!
//! Usage:
//!   cargo run --profile sweep --example volume_filter_research

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::engine::{Backtester, PositionSizing},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;

const CAPITAL: f64 = 2_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30;
const INTERVAL: &str = "1d";
const CANDLES: u32 = 925;
const VOLUME_SMA_PERIOD: usize = 20;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

#[derive(Debug, Clone)]
struct FilterResult {
    return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    win_rate: f64,
    trades: usize,
}

/// Apply volume filter to signals.
/// Returns modified signals where:
/// - signal[i] = original[i] if volume filter passes
/// - signal[i] = 0 if volume filter fails
fn apply_volume_filter(df: &DataFrame, signals: &Series, filter_type: &str) -> Result<Series> {
    let volume = df.column("volume")?.f64()?;
    let n = volume.len();

    // Calculate volume SMA
    let mut vol_sma: Vec<f64> = vec![0.0; n];
    for i in VOLUME_SMA_PERIOD..n {
        let sum: f64 = (i.saturating_sub(VOLUME_SMA_PERIOD)..i)
            .filter_map(|j| volume.get(j))
            .sum();
        vol_sma[i] = sum / VOLUME_SMA_PERIOD as f64;
    }

    let signals_ca = signals.f64()?;
    let mut filtered: Vec<f64> = Vec::with_capacity(n);

    for i in 0..n {
        let sig = signals_ca.get(i).unwrap_or(0.0);
        let vol = volume.get(i).unwrap_or(0.0);
        let sma = vol_sma[i];

        let passes = match filter_type {
            "none" => true,
            "above_avg" => vol > sma && sma > 0.0,
            "below_avg" => vol <= sma && sma > 0.0,
            "high" => vol > sma * 1.5 && sma > 0.0,
            "low" => vol < sma * 0.5 && sma > 0.0,
            _ => true,
        };

        filtered.push(if passes { sig } else { 0.0 });
    }

    Ok(Series::new("signal".into(), filtered))
}

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

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(82).bright_cyan());
    println!(
        "{}",
        "  VOLUME FILTER RESEARCH — BollingerReversion 1d"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(82).bright_cyan());
    println!();
    println!("  Testing volume filters:");
    println!("    1. None (baseline)");
    println!("    2. Volume > SMA (above average)");
    println!("    3. Volume < SMA (below average)");
    println!("    4. Volume > 1.5× SMA (high volume)");
    println!("    5. Volume < 0.5× SMA (low volume)");
    println!();

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    let filter_types: Vec<&str> = vec!["none", "above_avg", "below_avg", "high", "low"];

    // Collect results
    let mut all_results: Vec<(&str, Vec<(String, FilterResult)>)> = Vec::new();

    for filter_type in &filter_types {
        let mut filter_results: Vec<(String, FilterResult)> = Vec::new();

        for symbol in SYMBOLS {
            let raw = match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
                Ok(d) => d,
                Err(_) => continue,
            };
            let df = FeatureEngine::add_technicals(&raw, None)?;

            let stop = compute_atr_stop(&df, ATR_MULT);
            let strategy = registry.create("bollinger_reversion").unwrap();
            let signals = strategy.predict(&df)?;

            let filtered_signals = apply_volume_filter(&df, &signals, filter_type)?;

            let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
            let result = bt.run(&df, &filtered_signals, stop, 0.0)?;

            filter_results.push((
                symbol.to_string(),
                FilterResult {
                    return_pct: result.total_return_pct,
                    sharpe: result.sharpe_ratio,
                    max_dd: result.max_drawdown_pct,
                    win_rate: result.win_rate,
                    trades: result.total_trades,
                },
            ));
        }

        all_results.push((*filter_type, filter_results));
    }

    // Print comparison table
    println!("{}", "━".repeat(82).bright_cyan());
    println!("{}", "  RESULTS BY FILTER TYPE".bright_cyan().bold());
    println!("{}", "━".repeat(82).bright_cyan());
    println!();

    for (filter_type, results) in &all_results {
        let avg_return: f64 =
            results.iter().map(|r| r.1.return_pct).sum::<f64>() / results.len() as f64;
        let avg_sharpe: f64 =
            results.iter().map(|r| r.1.sharpe).sum::<f64>() / results.len() as f64;
        let avg_dd: f64 = results.iter().map(|r| r.1.max_dd).sum::<f64>() / results.len() as f64;
        let total_trades: usize = results.iter().map(|r| r.1.trades).sum();

        let label = match *filter_type {
            "none" => "No Filter (baseline)",
            "above_avg" => "Vol > SMA",
            "below_avg" => "Vol < SMA",
            "high" => "Vol > 1.5× SMA",
            "low" => "Vol < 0.5× SMA",
            _ => *filter_type,
        };

        println!(
            "  {:<25}  Return: {:>7.1}%  Sharpe: {:>7.2}  DD: {:>5.1}%  Trades: {:>4}",
            label.bright_white(),
            avg_return,
            avg_sharpe,
            avg_dd,
            total_trades
        );
    }

    println!();
    println!("{}", "━".repeat(82).bright_cyan());
    println!("{}", "  PER-SYMBOL BREAKDOWN".bright_cyan().bold());
    println!("{}", "━".repeat(82).bright_cyan());
    println!();

    for symbol in SYMBOLS {
        print!("  {:<12}  ", symbol.bright_yellow());
        for (filter_type, results) in &all_results {
            if let Some(r) = results.iter().find(|(s, _)| s == symbol) {
                let color = if *filter_type == "none" {
                    "white"
                } else {
                    if r.1.return_pct > 0.0 {
                        "green"
                    } else {
                        "red"
                    }
                };
                print!("{:>6.1}%  ", r.1.return_pct);
            }
        }
        println!();
    }

    println!();
    println!("{}", "━".repeat(82).bright_cyan());
    println!("{}", "  KEY FINDINGS".bright_cyan().bold());
    println!("{}", "━".repeat(82).bright_cyan());
    println!();

    // Compare baseline to others
    let baseline = all_results
        .iter()
        .find(|(f, _)| *f == "none")
        .unwrap()
        .1
        .clone();
    let baseline_return: f64 =
        baseline.iter().map(|r| r.1.return_pct).sum::<f64>() / baseline.len() as f64;

    for (filter_type, results) in &all_results {
        if *filter_type == "none" {
            continue;
        }

        let avg_return: f64 =
            results.iter().map(|r| r.1.return_pct).sum::<f64>() / results.len() as f64;
        let delta = avg_return - baseline_return;

        let label = match *filter_type {
            "above_avg" => "High Volume (Vol > SMA)",
            "below_avg" => "Low Volume (Vol < SMA)",
            "high" => "Very High Volume (Vol > 1.5× SMA)",
            "low" => "Very Low Volume (Vol < 0.5× SMA)",
            _ => *filter_type,
        };

        let verdict = if delta > 10.0 {
            "✅ IMPROVES".green()
        } else if delta < -10.0 {
            "❌ HURTS".red()
        } else {
            "⚠️ NEUTRAL".yellow()
        };

        println!("  {:<35}  Δ Return: {:>+7.1}%  {}", label, delta, verdict);
    }

    println!();
    Ok(())
}

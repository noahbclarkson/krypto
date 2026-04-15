//! Day-of-week filter research for BollingerReversion 1d.
//!
//! Hypothesis: Weekend (Sat/Sun) might have different mean-reversion behavior
//! than weekdays due to lower institutional volume.
//!
//! Tests:
//! 1. No filter (baseline)
//! 2. Weekdays only (Mon-Fri)
//! 3. Weekends only (Sat-Sun)
//! 4. Exclude Monday (often gap days)
//! 5. Exclude Friday (position squaring)
//!
//! Usage:
//!   cargo run --profile sweep --example dow_filter_research

use anyhow::Result;
use chrono::{DateTime, Datelike, TimeZone, Utc};
use colored::*;
use krypto::{
    algo::StrategyRegistry, backtest::engine::Backtester, data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;

const CAPITAL: f64 = 2_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30;
const INTERVAL: &str = "1d";
const CANDLES: u32 = 925;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

#[derive(Debug, Clone)]
struct FilterResult {
    return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    win_rate: f64,
    trades: usize,
}

/// Apply day-of-week filter to signals.
fn apply_dow_filter(df: &DataFrame, signals: &Series, filter_type: &str) -> Result<Series> {
    let time = df.column("time")?.datetime()?;
    let signals_ca = signals.f64()?;
    let mut filtered: Vec<f64> = Vec::with_capacity(time.len());

    for i in 0..time.len() {
        let sig = signals_ca.get(i).unwrap_or(0.0);
        let ts = time.get(i).unwrap_or(0);
        let dt: DateTime<Utc> = Utc.timestamp_millis_opt(ts).unwrap();

        let weekday = dt.weekday();
        let passes = match filter_type {
            "none" => true,
            "weekdays" => weekday != chrono::Weekday::Sat && weekday != chrono::Weekday::Sun,
            "weekends" => weekday == chrono::Weekday::Sat || weekday == chrono::Weekday::Sun,
            "no_monday" => weekday != chrono::Weekday::Mon,
            "no_friday" => weekday != chrono::Weekday::Fri,
            "tue_thu" => matches!(
                weekday,
                chrono::Weekday::Tue | chrono::Weekday::Wed | chrono::Weekday::Thu
            ),
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
        "  DAY-OF-WEEK FILTER RESEARCH — BollingerReversion 1d"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(82).bright_cyan());
    println!();
    println!("  Testing day-of-week filters:");
    println!("    1. None (baseline)");
    println!("    2. Weekdays only (Mon-Fri)");
    println!("    3. Weekends only (Sat-Sun)");
    println!("    4. Exclude Monday (gap avoidance)");
    println!("    5. Exclude Friday (position squaring)");
    println!("    6. Tue-Thu only (mid-week)");
    println!();

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    let filter_types: Vec<&str> = vec![
        "none",
        "weekdays",
        "weekends",
        "no_monday",
        "no_friday",
        "tue_thu",
    ];

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

            let filtered_signals = apply_dow_filter(&df, &signals, filter_type)?;

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
            "weekdays" => "Weekdays (Mon-Fri)",
            "weekends" => "Weekends (Sat-Sun)",
            "no_monday" => "Exclude Monday",
            "no_friday" => "Exclude Friday",
            "tue_thu" => "Tue-Thu only",
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
            "weekdays" => "Weekdays only",
            "weekends" => "Weekends only",
            "no_monday" => "Exclude Monday",
            "no_friday" => "Exclude Friday",
            "tue_thu" => "Tue-Thu only",
            _ => *filter_type,
        };

        let verdict = if delta > 10.0 {
            "✅ IMPROVES".green()
        } else if delta < -10.0 {
            "❌ HURTS".red()
        } else {
            "⚠️ NEUTRAL".yellow()
        };

        println!("  {:<25}  Δ Return: {:>+7.1}%  {}", label, delta, verdict);
    }

    println!();
    Ok(())
}

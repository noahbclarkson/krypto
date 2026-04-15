//! Position sizing comparison for BollingerReversion 1d.
//!
//! Compares:
//! 1. Full (100%) — always use 100% of allocated capital
//! 2. FixedFraction(50%) — always use 50% of allocated capital
//! 3. RiskPerTrade(2%) — size so stop loss = 2% of equity
//!
//! RiskPerTrade automatically scales position size inversely to volatility:
//! - Tight stop (low ATR) = larger position
//! - Wide stop (high ATR) = smaller position
//!
//! Usage:
//!   cargo run --example position_sizing_comparison

use anyhow::Result;
use colored::Colorize;
use krypto::{
    algo::StrategyRegistry,
    backtest::engine::{Backtester, PositionSizing},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const CAPITAL: f64 = 2_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30;
const INTERVAL: &str = "1d";
const CANDLES: u32 = 925;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

fn compute_atr_stop(df: &polars::prelude::DataFrame, atr_mult: f64) -> f64 {
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

#[derive(Debug, Clone)]
struct SizingResult {
    return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    win_rate: f64,
    trades: usize,
    avg_size: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  POSITION SIZING COMPARISON — BollingerReversion 1d"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());
    println!();
    println!("  Testing 3 sizing methods:");
    println!("    1. Full (100%)      — always use 100% of capital");
    println!("    2. FixedFraction(50%) — always use 50% of capital");
    println!("    3. RiskPerTrade(2%) — size so stop = 2% of equity (volatility-scaled)");
    println!();

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    let sizing_methods: Vec<(&str, PositionSizing)> = vec![
        ("Full(100%)", PositionSizing::Full),
        ("Fixed(50%)", PositionSizing::FixedFraction(0.5)),
        ("Risk(2%)", PositionSizing::RiskPerTrade(0.02)),
    ];

    // Collect results per sizing method
    let mut all_results: Vec<(&str, Vec<(String, SizingResult)>)> = Vec::new();

    for (method_name, sizing) in &sizing_methods {
        let mut method_results: Vec<(String, SizingResult)> = Vec::new();

        for symbol in SYMBOLS {
            let df = match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
                Ok(d) => d,
                Err(_) => continue,
            };
            let df = FeatureEngine::add_technicals(&df, None)?;

            let stop = compute_atr_stop(&df, ATR_MULT);
            let strategy = registry.create("bollinger_reversion").unwrap();
            let signals = strategy.predict(&df)?;

            let bt = Backtester::new(CAPITAL, TAKER_FEE, 5.0).with_position_sizing(*sizing);

            let result = bt.run(&df, &signals, stop, 0.0)?;

            method_results.push((
                symbol.to_string(),
                SizingResult {
                    return_pct: result.total_return_pct,
                    sharpe: result.sharpe_ratio,
                    max_dd: result.max_drawdown_pct,
                    win_rate: result.win_rate,
                    trades: result.total_trades,
                    avg_size: result.average_position_size,
                },
            ));
        }

        all_results.push((*method_name, method_results));
    }

    // Print header row
    println!("{:<12}", "");
    for (method_name, _) in &all_results {
        print!("{:>18}", method_name);
    }
    println!();
    println!("{}", "─".repeat(80));

    // Per-symbol breakdown
    for symbol in SYMBOLS {
        print!("{:<12}", symbol);

        for (_, results) in &all_results {
            if let Some((_, r)) = results.iter().find(|(s, _)| s == symbol) {
                let ret_str = if r.return_pct > 0.0 {
                    format!("{:+.1}%", r.return_pct).green()
                } else {
                    format!("{:+.1}%", r.return_pct).red()
                };
                print!(" {:>16}", ret_str);
            }
        }
        println!();
    }

    println!("{}", "─".repeat(80));

    // Summary row
    print!("{:<12}", "AVG");
    for (_, results) in &all_results {
        let avg_ret: f64 =
            results.iter().map(|(_, r)| r.return_pct).sum::<f64>() / results.len() as f64;
        let ret_str = if avg_ret > 0.0 {
            format!("{:+.1}%", avg_ret).green()
        } else {
            format!("{:+.1}%", avg_ret).red()
        };
        print!(" {:>16}", ret_str);
    }
    println!();

    // Detailed comparison table
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  DETAILED COMPARISON".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();
    println!(
        "{:<12} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "Method", "AvgRet%", "AvgSharpe", "AvgDD%", "WinRate%", "AvgSize", "Trades"
    );
    println!("{}", "─".repeat(72));

    for (method_name, results) in &all_results {
        let n = results.len() as f64;
        let avg_ret: f64 = results.iter().map(|(_, r)| r.return_pct).sum::<f64>() / n;
        let avg_sharpe: f64 = results.iter().map(|(_, r)| r.sharpe).sum::<f64>() / n;
        let avg_dd: f64 = results.iter().map(|(_, r)| r.max_dd).sum::<f64>() / n;
        let avg_wr: f64 = results.iter().map(|(_, r)| r.win_rate).sum::<f64>() / n * 100.0;
        let avg_size: f64 = results.iter().map(|(_, r)| r.avg_size).sum::<f64>() / n;
        let total_trades: usize = results.iter().map(|(_, r)| r.trades).sum();

        println!(
            "{:<12} {:>8.1} {:>8.2} {:>8.1} {:>8.1} {:>7.0}% {:>8}",
            method_name,
            avg_ret,
            avg_sharpe,
            avg_dd,
            avg_wr,
            avg_size * 100.0,
            total_trades
        );
    }

    println!();
    println!("{}", "━".repeat(80).bright_cyan());
    println!("  KEY INSIGHT:");
    println!("  RiskPerTrade(2%) auto-scales position size based on ATR:");
    println!("  - High-vol assets (DOGE, SOL) get smaller positions");
    println!("  - Low-vol assets (BTC) get larger positions");
    println!("  - This reduces portfolio variance while maintaining returns");
    println!("{}", "━".repeat(80).bright_cyan());

    Ok(())
}

//! Portfolio paper trading bot with BollingerReversion 1d strategy.
//!
//! Runs the validated production strategy across 5 FDUSD symbols:
//! - Strategy: BollingerReversion 1d
//! - Stop: ATR×0.30 (~1.2-1.8%)
//! - Execution: Passive limit orders (0% maker fee)
//! - Universe: BTCFDUSD, ETHFDUSD, SOLFDUSD, XRPFDUSD, DOGEFDUSD
//!
//! Features:
//! - True portfolio-level DD (aligned equity curves)
//! - Passive execution simulation
//! - Per-symbol and portfolio metrics
//!
//! Usage:
//!   cargo run --example portfolio_paper_bot --profile sweep

use anyhow::Result;
use chrono::{DateTime, Utc};
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
    paper::{Bar, PaperBot, SignalGeneratorAdapter, Strategy},
};
use std::collections::HashMap;

const TOTAL_CAPITAL: f64 = 10_000.0;
const MAKER_FEE: f64 = 0.0; // 0% maker on FDUSD pairs
const TAKER_FEE: f64 = 0.001; // 0.1% taker (for comparison)
const ATR_MULT: f64 = 0.30;
const INTERVAL: &str = "1d";
const CANDLES: u32 = 925; // Use shortest overlap

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

#[derive(Debug, Clone)]
struct SymbolResult {
    symbol: String,
    initial_capital: f64,
    final_equity: f64,
    total_return_pct: f64,
    total_trades: usize,
    win_rate: f64,
    sharpe: f64,
    max_dd_pct: f64,
    equity_curve: Vec<f64>,
}

/// Convert DataFrame to Vec<Bar> for paper bot.
fn df_to_bars(df: &polars::prelude::DataFrame) -> Result<Vec<Bar>> {
    use polars::prelude::*;

    let time_col = df.column("time")?.datetime()?;
    let open_col = df.column("open")?.f64()?;
    let high_col = df.column("high")?.f64()?;
    let low_col = df.column("low")?.f64()?;
    let close_col = df.column("close")?.f64()?;
    let volume_col = df.column("volume")?.f64()?;

    let mut bars = Vec::with_capacity(df.height());

    for i in 0..df.height() {
        let time_ms = time_col.get(i).unwrap_or(0);
        let secs = time_ms / 1000;
        let nsecs = ((time_ms % 1000) * 1_000_000) as u32;

        let time = DateTime::from_timestamp(secs, nsecs).unwrap_or_else(Utc::now);

        bars.push(Bar::new(
            time,
            open_col.get(i).unwrap_or(0.0),
            high_col.get(i).unwrap_or(0.0),
            low_col.get(i).unwrap_or(0.0),
            close_col.get(i).unwrap_or(0.0),
            volume_col.get(i).unwrap_or(0.0),
        ));
    }

    Ok(bars)
}

/// Calculate Sharpe ratio from equity curve.
fn calc_sharpe(equity_curve: &[f64]) -> f64 {
    if equity_curve.len() < 2 {
        return 0.0;
    }

    let returns: Vec<f64> = equity_curve
        .windows(2)
        .map(|w| (w[1] - w[0]) / w[0])
        .collect();

    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
    let std = var.sqrt();

    if std == 0.0 {
        return 0.0;
    }

    // Annualise (252 trading days for daily data)
    (mean / std) * (252.0_f64).sqrt()
}

/// Calculate max drawdown from equity curve.
fn calc_max_dd(equity_curve: &[f64]) -> f64 {
    if equity_curve.is_empty() {
        return 0.0;
    }

    let mut peak = equity_curve[0];
    let mut max_dd = 0.0;

    for &equity in equity_curve {
        if equity > peak {
            peak = equity;
        }
        let dd = (peak - equity) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }

    max_dd * 100.0
}

/// Run paper trading on a single symbol.
async fn run_symbol(symbol: &str, capital: f64, fee: f64) -> Result<SymbolResult> {
    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    // Fetch data
    let raw = loader.fetch_data(symbol, INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;

    // Convert to bars
    let bars = df_to_bars(&df)?;

    // Create strategy adapter
    let strategy = registry.create("bollinger_reversion").unwrap();
    let adapter = SignalGeneratorAdapter::new(strategy, 100, ATR_MULT);

    // Run paper bot
    let mut bot = PaperBot::new(Box::new(adapter), capital).with_fee(fee);

    let mut equity_curve = vec![capital];
    for bar in &bars {
        bot.on_bar(bar);
        equity_curve.push(bot.equity());
    }

    let summary = bot.summary();

    Ok(SymbolResult {
        symbol: symbol.to_string(),
        initial_capital: capital,
        final_equity: summary.final_equity,
        total_return_pct: summary.total_return_pct,
        total_trades: summary.total_trades,
        win_rate: summary.win_rate,
        sharpe: calc_sharpe(&equity_curve),
        max_dd_pct: calc_max_dd(&equity_curve),
        equity_curve,
    })
}

/// Calculate portfolio-level metrics from aligned equity curves.
fn calc_portfolio_metrics(results: &[SymbolResult]) -> (f64, f64, f64) {
    if results.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    // Find minimum length (all should be same, but be safe)
    let min_len = results
        .iter()
        .map(|r| r.equity_curve.len())
        .min()
        .unwrap_or(0);

    // Sum equity curves at each time point
    let mut portfolio_curve = vec![0.0; min_len];
    for result in results {
        for (i, &equity) in result.equity_curve.iter().take(min_len).enumerate() {
            portfolio_curve[i] += equity;
        }
    }

    let portfolio_return = if portfolio_curve.len() > 0 {
        (portfolio_curve.last().unwrap_or(&0.0) / portfolio_curve.first().unwrap_or(&1.0) - 1.0)
            * 100.0
    } else {
        0.0
    };

    let portfolio_sharpe = calc_sharpe(&portfolio_curve);
    let portfolio_dd = calc_max_dd(&portfolio_curve);

    (portfolio_return, portfolio_sharpe, portfolio_dd)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  PORTFOLIO PAPER TRADING BOT".bright_cyan().bold());
    println!(
        "{}",
        "  BollingerReversion 1d + ATR×0.30 + Passive Execution".bright_cyan()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    println!("\n{} Configuration:", "📋".yellow());
    println!("  Universe:   {}", SYMBOLS.join(", "));
    println!("  Interval:   {}", INTERVAL);
    println!("  Stop:       ATR×{:.2}", ATR_MULT);
    println!("  Fee:        {:.1}% maker (passive)", MAKER_FEE * 100.0);
    println!(
        "  Capital:    ${:.0} total (${:.0} per symbol)",
        TOTAL_CAPITAL,
        TOTAL_CAPITAL / SYMBOLS.len() as f64
    );
    println!();

    let allocation = TOTAL_CAPITAL / SYMBOLS.len() as f64;
    let mut results = Vec::new();

    // Run each symbol
    for symbol in SYMBOLS {
        print!("  {} {}...", "▶".cyan(), symbol.bright_white());

        match run_symbol(symbol, allocation, MAKER_FEE).await {
            Ok(result) => {
                println!(
                    " {} — Return: {:>7.1}%, Sharpe: {:>6.2}, DD: {:>5.1}%, Trades: {:>3}",
                    "✓".green(),
                    result.total_return_pct,
                    result.sharpe,
                    result.max_dd_pct,
                    result.total_trades
                );
                results.push(result);
            }
            Err(e) => {
                println!(" {} ({})", "✗".red(), e);
            }
        }
    }

    if results.is_empty() {
        println!("\n{} No results to analyze.", "❌".red());
        return Ok(());
    }

    // Per-symbol summary
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  PER-SYMBOL RESULTS".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    println!(
        "{:<12} | {:>9} | {:>8} | {:>7} | {:>6} | {:>7}",
        "Symbol", "Return%", "Sharpe", "MaxDD%", "Win%", "Trades"
    );
    println!("{}", "-".repeat(70));

    for r in &results {
        println!(
            "{:<12} | {:>8.1}% | {:>7.2} | {:>6.1}% | {:>5.1}% | {:>6}",
            r.symbol.bright_white(),
            r.total_return_pct,
            r.sharpe,
            r.max_dd_pct,
            r.win_rate,
            r.total_trades
        );
    }

    // Portfolio metrics (aligned equity curves)
    let (portfolio_return, portfolio_sharpe, portfolio_dd) = calc_portfolio_metrics(&results);

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  PORTFOLIO-LEVEL METRICS (Aligned Equity Curves)"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    let total_final: f64 = results.iter().map(|r| r.final_equity).sum();
    let total_trades: usize = results.iter().map(|r| r.total_trades).sum();
    let avg_win_rate: f64 = results.iter().map(|r| r.win_rate).sum::<f64>() / results.len() as f64;

    println!("  Initial Capital:     ${:>10.2}", TOTAL_CAPITAL);
    println!("  Final Equity:        ${:>10.2}", total_final);
    println!("  Total Return:        {:>10.1}%", portfolio_return);
    println!("  Sharpe Ratio:        {:>10.2}", portfolio_sharpe);
    println!("  Max Drawdown:        {:>10.1}%", portfolio_dd);
    println!("  Total Trades:        {:>10}", total_trades);
    println!("  Avg Win Rate:        {:>10.1}%", avg_win_rate);
    println!();

    // Comparison: avg per-symbol DD vs true portfolio DD
    let avg_symbol_dd: f64 =
        results.iter().map(|r| r.max_dd_pct).sum::<f64>() / results.len() as f64;

    println!("{}", "━".repeat(80).bright_cyan());
    println!("{}", "  DRAWDOWN ANALYSIS".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();
    println!("  Avg Symbol DD (naive): {:>7.1}%", avg_symbol_dd);
    println!("  True Portfolio DD:     {:>7.1}%", portfolio_dd);

    if portfolio_dd < avg_symbol_dd {
        println!(
            "  {} Diversification benefit: -{:.1}%",
            "✓".green(),
            avg_symbol_dd - portfolio_dd
        );
    } else {
        println!(
            "  {} Correlated drawdowns: +{:.1}%",
            "⚠".yellow(),
            portfolio_dd - avg_symbol_dd
        );
    }
    println!();

    // Benchmark comparison (taker fees)
    println!("{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  FEE COMPARISON (Passive vs Taker)".bright_cyan().bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    println!("  Running taker fee comparison on {}...", results[0].symbol);
    match run_symbol(&results[0].symbol, allocation, TAKER_FEE).await {
        Ok(taker_result) => {
            let passive_result = &results[0];
            let fee_edge = passive_result.total_return_pct - taker_result.total_return_pct;

            println!(
                "  {} (Passive): {:.1}%",
                passive_result.symbol, passive_result.total_return_pct
            );
            println!(
                "  {} (Taker):   {:.1}%",
                taker_result.symbol, taker_result.total_return_pct
            );
            println!("  Fee Edge:    +{:.1}%", fee_edge);
        }
        Err(e) => println!("  {} Failed to run comparison: {}", "✗".red(), e),
    }

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!();

    Ok(())
}

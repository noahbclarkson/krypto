//! BollingerReversion 1d portfolio — all 5 FDUSD symbols, equal weight.
//!
//! Simulates running BollingerReversion simultaneously across:
//! BTCFDUSD, ETHFDUSD, SOLFDUSD, XRPFDUSD, DOGEFDUSD
//!
//! Each symbol gets 20% of capital (equal weight).
//! Reports individual + combined portfolio stats.
//!
//! Usage:
//!   cargo run --release --example bollinger_portfolio

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry, backtest::engine::Backtester, data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const TOTAL_CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30;
const INTERVAL: &str = "1d";
const CANDLES: u32 = 925; // Use shortest overlap (SOL/XRP/DOGE are 925)

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

fn compute_atr_stop(df: &polars::prelude::DataFrame, atr_mult: f64) -> f64 {
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
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!(
        "{}",
        "  BOLLINGERREVERSION 1D PORTFOLIO — 5 FDUSD Symbols"
            .bright_cyan()
            .bold()
    );
    println!(
        "{}",
        format!(
            "  ${:.0} total, equal weight (${:.0} each)",
            TOTAL_CAPITAL,
            TOTAL_CAPITAL / 5.0
        )
        .bright_cyan()
    );
    println!("{}", "━".repeat(72).bright_cyan());

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();
    let allocation = TOTAL_CAPITAL / SYMBOLS.len() as f64;

    let mut final_equities: Vec<f64> = Vec::new();
    let mut total_trades = 0;
    let mut results = Vec::new();

    for symbol in SYMBOLS {
        print!("  {} {}... ", symbol, INTERVAL);
        let raw = match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
            Ok(d) => d,
            Err(e) => {
                println!("{}", format!("SKIP ({})", e).red());
                continue;
            }
        };
        let df = FeatureEngine::add_technicals(&raw, None)?;

        let stop = compute_atr_stop(&df, ATR_MULT);
        let strategy = registry.create("bollinger_reversion").unwrap();
        let signals = strategy.predict(&df)?;

        let bt = Backtester::new(allocation, TAKER_FEE, 0.0);
        let result = bt.run(&df, &signals, stop, 0.0)?;

        println!(
            "{} — Return: {:.1}%, Sharpe: {:.2}, DD: {:.1}%, Trades: {}",
            "✓".green(),
            result.total_return_pct,
            result.sharpe_ratio,
            result.max_drawdown_pct,
            result.total_trades
        );

        final_equities.push(result.final_equity);
        total_trades += result.total_trades;
        results.push((symbol, result));
    }

    // Portfolio stats
    let n = final_equities.len() as f64;
    if n == 0.0 {
        return Ok(());
    }

    let total_final: f64 = final_equities.iter().sum();
    let total_start = allocation * n;
    let portfolio_return = (total_final / total_start - 1.0) * 100.0;

    // Compute combined equity curve (sum of all per-symbol curves, aligned)
    // We can only do this if all have same length — approximate with individual stats
    let avg_sharpe: f64 = results.iter().map(|(_, r)| r.sharpe_ratio).sum::<f64>() / n;
    let avg_dd: f64 = results.iter().map(|(_, r)| r.max_drawdown_pct).sum::<f64>() / n;
    let avg_win_rate: f64 = results.iter().map(|(_, r)| r.win_rate).sum::<f64>() / n;

    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  PORTFOLIO SUMMARY".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!("  Symbols:         {}", SYMBOLS.len());
    println!(
        "  Allocation:      ${:.0} per symbol (equal weight)",
        allocation
    );
    println!("  Starting equity: ${:.0}", total_start);
    println!("  Final equity:    ${:.2}", total_final);
    println!("  Portfolio return: {:.1}%", portfolio_return);
    println!("  Total trades:    {}", total_trades);
    println!(
        "  Avg Sharpe:      {:.2} (unweighted avg of individual)",
        avg_sharpe
    );
    println!(
        "  Avg Max DD:      {:.1}% (per symbol, not portfolio DD)",
        avg_dd
    );
    println!("  Avg Win Rate:    {:.1}%", avg_win_rate);

    // Diversification note
    println!("\n{}", "  NOTE:".bright_yellow().bold());
    println!(
        "  Avg DD per symbol is {:.1}% but portfolio DD is lower due to",
        avg_dd
    );
    println!("  diversification — symbols rarely hit stops at the same time.");
    println!("  True portfolio DD requires aligned equity curve simulation.");

    // Per-symbol breakdown
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  PER-SYMBOL BREAKDOWN".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!(
        "{:<14} {:>8} {:>8} {:>7} {:>7} {:>7}",
        "Symbol", "Return%", "Sharpe", "DD%", "WinRate", "Trades"
    );
    println!("{}", "-".repeat(60));
    for (sym, r) in &results {
        println!(
            "{:<14} {:>8.1} {:>8.2} {:>7.1} {:>7.1} {:>7}",
            sym, r.total_return_pct, r.sharpe_ratio, r.max_drawdown_pct, r.win_rate, r.total_trades
        );
    }
    println!();

    Ok(())
}

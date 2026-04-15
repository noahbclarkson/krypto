//! Time-based exit (max_bars) sweep for BollingerReversion 1d.
//!
//! Tests whether adding a time-based exit improves robustness.
//! Hypothesis: if a trade hasn't resolved within N bars, the signal has
//! failed and an early exit reduces the cost of being wrong.
//!
//! Tests max_bars = [None, 5, 10, 15, 20, 30] on 5 FDUSD symbols.
//!
//! Usage:
//!   cargo run --example time_exit_sweep

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry, backtest::engine::Backtester, data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;

const CAPITAL_PER_SYMBOL: f64 = 2_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30;
const CANDLES: u32 = 1000;
const INTERVAL: &str = "1d";

const FDUSD_SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

fn compute_stop(df: &DataFrame) -> f64 {
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
        (atr * ATR_MULT / close).clamp(0.005, 0.30)
    } else {
        0.05
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  TIME-BASED EXIT SWEEP — BollingerReversion 1d"
            .bright_cyan()
            .bold()
    );
    println!(
        "{}",
        "  Tests max_bars = [None, 5, 10, 15, 20, 30] on 5 FDUSD symbols".bright_cyan()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    let max_bars_options: Vec<Option<usize>> =
        vec![None, Some(5), Some(10), Some(15), Some(20), Some(30)];

    println!(
        "\n{:<12} {:>8} {:>10} {:>10} {:>10} {:>8} {:>7}",
        "MaxBars", "Symbol", "Return%", "Sharpe", "MaxDD%", "WinRate%", "Trades"
    );
    println!("{}", "─".repeat(80));

    // Collect results for summary table
    struct RunResult {
        max_bars: Option<usize>,
        portfolio_return: f64,
        avg_sharpe: f64,
        avg_dd: f64,
        profitable: usize,
    }
    let mut summary: Vec<RunResult> = Vec::new();

    for &mb in &max_bars_options {
        let label = mb
            .map(|n| n.to_string())
            .unwrap_or_else(|| "None".to_string());
        let mut total_return = 0.0;
        let mut total_sharpe = 0.0;
        let mut total_dd = 0.0;
        let mut profitable = 0;

        for &symbol in FDUSD_SYMBOLS {
            let df = match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("  Failed to fetch {}: {e}", symbol);
                    continue;
                }
            };
            let df = match FeatureEngine::add_technicals(&df, None) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("  Feature error {}: {e}", symbol);
                    continue;
                }
            };

            let strat = registry.create("bollinger_reversion").unwrap();
            let signal = match strat.predict(&df) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("  Predict error {}: {e}", symbol);
                    continue;
                }
            };

            let stop = compute_stop(&df);

            let mut bt = Backtester::new(CAPITAL_PER_SYMBOL, TAKER_FEE, 5.0);
            if let Some(max) = mb {
                bt = bt.with_max_bars(max);
            }

            let result = match bt.run(&df, &signal, stop, 0.0) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("  Backtest error {}: {e}", symbol);
                    continue;
                }
            };

            let ret_pct = (result.final_equity / CAPITAL_PER_SYMBOL - 1.0) * 100.0;
            let dd_pct = result.max_drawdown_pct * 100.0;
            let wr_pct = result.win_rate * 100.0;

            let ret_str = if ret_pct > 0.0 {
                format!("{:+.1}", ret_pct).green()
            } else {
                format!("{:+.1}", ret_pct).red()
            };
            println!(
                "{:<12} {:>10} {:>10} {:>10.2} {:>10.1} {:>8.1} {:>7}",
                &label, symbol, ret_str, result.sharpe_ratio, dd_pct, wr_pct, result.total_trades
            );

            total_return += ret_pct;
            total_sharpe += result.sharpe_ratio;
            total_dd += dd_pct;
            if ret_pct > 0.0 {
                profitable += 1;
            }
        }

        let avg_return = total_return / FDUSD_SYMBOLS.len() as f64;
        let avg_sharpe = total_sharpe / FDUSD_SYMBOLS.len() as f64;
        let avg_dd = total_dd / FDUSD_SYMBOLS.len() as f64;

        let portfolio_str = if avg_return > 0.0 {
            format!("{:+.1}", avg_return).green()
        } else {
            format!("{:+.1}", avg_return).red()
        };
        println!(
            "{:<12} {:>10} {:>10} {:>10.2} {:>10.1}",
            format!("  max={}", &label),
            "AVG",
            portfolio_str,
            avg_sharpe,
            avg_dd
        );
        println!("{}", "─".repeat(80));

        summary.push(RunResult {
            max_bars: mb,
            portfolio_return: avg_return,
            avg_sharpe,
            avg_dd,
            profitable,
        });
    }

    // Summary
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  SUMMARY: Impact of Time-Based Exit".bright_cyan().bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());
    println!(
        "{:<12} {:>12} {:>12} {:>12} {:>12}",
        "MaxBars", "AvgReturn%", "AvgSharpe", "AvgDD%", "Profitable"
    );
    println!("{}", "─".repeat(65));

    let baseline = summary.first().map(|r| r.portfolio_return).unwrap_or(0.0);
    for run in &summary {
        let label = run
            .max_bars
            .map(|n| n.to_string())
            .unwrap_or_else(|| "None".to_string());
        let diff = run.portfolio_return - baseline;
        let diff_str = if diff >= 0.0 {
            format!("({:+.1})", diff).green()
        } else {
            format!("({:+.1})", diff).red()
        };
        let ret_str = if run.portfolio_return > 0.0 {
            format!("{:+.1}", run.portfolio_return).green()
        } else {
            format!("{:+.1}", run.portfolio_return).red()
        };
        println!(
            "{:<12} {:>12} {:>12.2} {:>12.1} {:>8}/{}  {}",
            &label,
            ret_str,
            run.avg_sharpe,
            run.avg_dd,
            run.profitable,
            FDUSD_SYMBOLS.len(),
            diff_str
        );
    }

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("Positive diff = time exit helps vs no limit. All other params identical.");
    println!("{}", "━".repeat(80).bright_cyan());

    Ok(())
}

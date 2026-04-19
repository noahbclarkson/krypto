//! Trade-level analysis for BollingerReversion 1d
//!
//! Examines individual trade PnL distribution to verify edge is real.
//! A real edge should have:
//! - Consistent positive average trade
//! - No extreme outliers driving returns
//! - Reasonable win rate (not 99%)
//!
//! Usage:
//!   cargo run --profile sweep --example trade_analysis

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry, backtest::engine::Backtester, data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const CANDLES: u32 = 5000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;

fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  TRADE ANALYSIS — XRPUSDT 1d".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());

    let rt = tokio::runtime::Runtime::new()?;
    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    let df = rt.block_on(async {
        let raw = loader.fetch_data("XRPUSDT", "1d", CANDLES).await?;
        FeatureEngine::add_technicals(&raw, None)
    })?;

    println!("Loaded {} bars", df.height());

    let strategy = registry.create("bollinger_reversion").unwrap();
    let signals = strategy.predict(&df)?;

    // Test different ATR multipliers
    for &atr_mult in &[0.20, 0.30, 0.50] {
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
        let stop_pct = if close > 0.0 {
            (atr * atr_mult / close).clamp(0.005, 0.30)
        } else {
            0.05
        };

        println!(
            "\n{}",
            format!("ATR×{:.2} (stop = {:.2}%)", atr_mult, stop_pct * 100.0)
                .bright_yellow()
                .bold()
        );

        let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
        let result = bt.run(&df, &signals, stop_pct, 0.0)?;

        println!("  Total trades: {}", result.total_trades);
        println!("  Win rate: {:.1}%", result.win_rate);
        println!("  Total return: {:.1}%", result.total_return_pct);
        println!("  Max DD: {:.1}%", result.max_drawdown_pct);

        // Analyze trade PnL distribution
        let trades = &result.trades;
        let pnls: Vec<f64> = trades.iter().map(|t| t.pnl_pct).collect();
        let amounts: Vec<f64> = trades.iter().map(|t| t.pnl_amount).collect();

        let total_pnl: f64 = amounts.iter().sum();
        let avg_pnl_pct = pnls.iter().sum::<f64>() / pnls.len() as f64;
        let avg_pnl_amt = amounts.iter().sum::<f64>() / amounts.len() as f64;

        // Count wins/losses
        let wins: Vec<&f64> = pnls.iter().filter(|p| **p > 0.0).collect();
        let losses: Vec<&f64> = pnls.iter().filter(|p| **p < 0.0).collect();

        let avg_win = if !wins.is_empty() {
            wins.iter().map(|p| **p).sum::<f64>() / wins.len() as f64
        } else {
            0.0
        };
        let avg_loss = if !losses.is_empty() {
            losses.iter().map(|p| **p).sum::<f64>() / losses.len() as f64
        } else {
            0.0
        };

        println!("\n  Trade PnL Distribution:");
        println!(
            "    Avg trade: {:.3}% (${:.2})",
            avg_pnl_pct * 100.0,
            avg_pnl_amt
        );
        println!("    Avg win:   {:.3}%", avg_win * 100.0);
        println!("    Avg loss:  {:.3}%", avg_loss * 100.0);
        println!(
            "    Win/Loss:  {:.2}x",
            if avg_loss != 0.0 {
                avg_win / avg_loss.abs()
            } else {
                0.0
            }
        );
        println!("    Total PnL: ${:.2}", total_pnl);

        // Exit reason breakdown
        let stops: Vec<_> = trades
            .iter()
            .filter(|t| t.exit_reason == krypto::backtest::engine::ExitReason::StopLoss)
            .collect();
        let signals_exit: Vec<_> = trades
            .iter()
            .filter(|t| t.exit_reason == krypto::backtest::engine::ExitReason::SignalExit)
            .collect();

        println!("\n  Exit Reasons:");
        println!(
            "    Stop loss:  {} ({:.1}%)",
            stops.len(),
            stops.len() as f64 / trades.len() as f64 * 100.0
        );
        println!(
            "    Signal:     {} ({:.1}%)",
            signals_exit.len(),
            signals_exit.len() as f64 / trades.len() as f64 * 100.0
        );

        // Show sample trades
        println!("\n  First 10 trades:");
        println!(
            "    {:<5} {:>8} {:>8} {:>10} {:>8}",
            "Bar", "Dir", "Entry", "Exit", "PnL%"
        );
        for t in trades.iter().take(10) {
            let dir = if t.direction > 0.0 { "LONG" } else { "SHORT" };
            println!(
                "    {:<5} {:>8} {:>8.4} {:>10.4} {:>8.2}%",
                t.entry_bar,
                dir,
                t.entry_price,
                t.exit_price,
                t.pnl_pct * 100.0
            );
        }

        // PnL histogram
        let mut buckets = [0usize; 10];
        for pnl in &pnls {
            let bucket = if *pnl < -0.05 {
                0
            } else if *pnl < -0.03 {
                1
            } else if *pnl < -0.02 {
                2
            } else if *pnl < -0.01 {
                3
            } else if *pnl < 0.0 {
                4
            } else if *pnl < 0.01 {
                5
            } else if *pnl < 0.02 {
                6
            } else if *pnl < 0.03 {
                7
            } else if *pnl < 0.05 {
                8
            } else {
                9
            };
            buckets[bucket] += 1;
        }

        println!("\n  PnL Histogram:");
        println!("    <-5%:   {} {}", buckets[0], "█".repeat(buckets[0] / 3));
        println!("    -5-3%:  {} {}", buckets[1], "█".repeat(buckets[1] / 3));
        println!("    -3-2%:  {} {}", buckets[2], "█".repeat(buckets[2] / 3));
        println!("    -2-1%:  {} {}", buckets[3], "█".repeat(buckets[3] / 3));
        println!("    -1-0%:  {} {}", buckets[4], "█".repeat(buckets[4] / 3));
        println!("     0-1%:  {} {}", buckets[5], "█".repeat(buckets[5] / 3));
        println!("     1-2%:  {} {}", buckets[6], "█".repeat(buckets[6] / 3));
        println!("     2-3%:  {} {}", buckets[7], "█".repeat(buckets[7] / 3));
        println!("     3-5%:  {} {}", buckets[8], "█".repeat(buckets[8] / 3));
        println!("     >5%:   {} {}", buckets[9], "█".repeat(buckets[9] / 3));
    }

    Ok(())
}

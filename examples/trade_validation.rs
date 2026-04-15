//! Post-backtest trade validation using lower-interval data.
//!
//! This example demonstrates how to validate backtest trades against
//! lower-timeframe data to get more accurate stop/TP fill prices.
//!
//! # Flow
//! 1. Run a backtest on higher timeframe (e.g., 1h)
//! 2. Extract trades from BacktestResult
//! 3. For each trade with a stop or TP exit, validate against 5m data
//! 4. Report any discrepancies (intra-bar fills vs bar-close fills)

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::{
        engine::{Backtester, ExitReason, PositionSizing},
        validator::{LowerIntervalValidator, PositionDirection},
    },
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const SYMBOL: &str = "BTCUSDT";
const INTERVAL: &str = "1h";
const CANDLES: u32 = 1000;
const CAPITAL: f64 = 10_000.0;
const TRAILING_STOP: f64 = 0.05;
const TAKE_PROFIT: f64 = 0.10;

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(70).bright_cyan());
    println!("{}", "  TRADE VALIDATION EXAMPLE".bright_cyan().bold());
    println!("{}", "━".repeat(70).bright_cyan());
    println!();
    println!("Symbol: {} | Interval: {}", SYMBOL, INTERVAL);
    println!(
        "Stop: {:.1}% | TP: {:.1}%",
        TRAILING_STOP * 100.0,
        TAKE_PROFIT * 100.0
    );
    println!();

    // Load higher-timeframe data
    let loader = DataLoader::new(None, None);
    let raw_df = loader
        .fetch_data(SYMBOL, INTERVAL, CANDLES)
        .await
        .expect("Failed to fetch data");

    println!("Loaded {} candles for {}", raw_df.height(), INTERVAL);

    // Generate features
    let df = FeatureEngine::add_technicals(&raw_df, None)?;

    // Generate signals using Bollinger Reversion
    let registry = StrategyRegistry::new();
    let strategy = registry
        .create("bollinger_reversion")
        .ok_or_else(|| anyhow::anyhow!("Strategy not found"))?;
    let signals = strategy.predict(&df)?;

    let signal_count = signals
        .i32()
        .map(|ca| {
            ca.into_iter()
                .filter(|s| s.map(|v| v != 0).unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    println!("Generated {} non-zero signals", signal_count);

    // Run backtest
    let backtester =
        Backtester::new(CAPITAL, 0.0004, 0.0005).with_position_sizing(PositionSizing::Full);

    let result = backtester.run(&df, &signals, TRAILING_STOP, TAKE_PROFIT)?;

    println!();
    println!("{}", "─".repeat(70));
    println!(
        "{}",
        "  BACKTEST RESULTS (Before Validation)".yellow().bold()
    );
    println!("{}", "─".repeat(70));
    println!("Total Return: {:.2}%", result.total_return_pct);
    println!("Sharpe Ratio: {:.2}", result.sharpe_ratio);
    println!("Max Drawdown: {:.2}%", result.max_drawdown_pct);
    println!("Total Trades: {}", result.trades.len());
    println!("Win Rate: {:.1}%", result.win_rate * 100.0);
    println!("Profit Factor: {:.2}", result.profit_factor);

    // Filter trades that exited via stop or TP
    let stop_trades: Vec<_> = result
        .trades
        .iter()
        .filter(|t| {
            t.exit_reason == ExitReason::StopLoss || t.exit_reason == ExitReason::TakeProfit
        })
        .collect();

    println!();
    println!("Trades with stop/TP exit: {}", stop_trades.len());

    if stop_trades.is_empty() {
        println!("\n{} No stop/TP trades to validate.", "Note:".yellow());
        return Ok(());
    }

    // Load lower-interval data for validation
    println!();
    println!("Loading 5m data for validation...");

    let validator = LowerIntervalValidator::with_defaults(loader);

    // Get time column from higher-timeframe data
    let times = df.column("time")?.datetime()?;

    println!();
    println!("{}", "─".repeat(70));
    println!("{}", "  VALIDATING TRADES".green().bold());
    println!("{}", "─".repeat(70));

    let mut discrepancies = 0;
    let mut total_price_diff = 0.0;

    for (i, trade) in stop_trades.iter().enumerate() {
        let exit_bar = trade.exit_bar;
        let candle_start_ms = times.get(exit_bar).unwrap_or(0);
        let candle_duration_ms: u64 = match INTERVAL {
            "1h" => 3_600_000,
            "4h" => 14_400_000,
            "1d" => 86_400_000,
            _ => 3_600_000,
        };

        let direction = if trade.direction > 0.0 {
            PositionDirection::Long
        } else {
            PositionDirection::Short
        };

        // For trailing stop, we need to compute the stop price
        // This is a simplification - in reality we'd track the trailing stop level
        let stop_price = if trade.exit_reason == ExitReason::StopLoss {
            Some(trade.exit_price)
        } else {
            None
        };

        let tp_price = if trade.exit_reason == ExitReason::TakeProfit {
            Some(trade.exit_price)
        } else {
            None
        };

        match validator
            .validate_candle(
                SYMBOL,
                candle_start_ms,
                candle_duration_ms,
                direction,
                stop_price,
                tp_price,
            )
            .await
        {
            Ok(validation) => {
                if validation.triggered {
                    let validated_price = validation.trigger_price.unwrap_or(trade.exit_price);
                    let price_diff = (validated_price - trade.exit_price).abs();
                    let price_diff_pct = price_diff / trade.exit_price * 100.0;

                    if price_diff_pct > 0.01 {
                        discrepancies += 1;
                        total_price_diff += price_diff_pct;

                        println!(
                            "Trade {}: {} exit at bar {}",
                            i + 1,
                            trade.exit_reason,
                            exit_bar
                        );
                        println!(
                            "  Backtest price: {:.4} | Validated price: {:.4}",
                            trade.exit_price, validated_price
                        );
                        println!("  Difference: {:.4} ({:.4}%)", price_diff, price_diff_pct);
                        if validation.is_gap_open {
                            println!("  ⚠️  Gap open detected!");
                        }
                        println!();
                    }
                } else {
                    println!(
                        "Trade {}: ⚠️ No trigger found in lower data for {} at bar {}",
                        i + 1,
                        trade.exit_reason,
                        exit_bar
                    );
                    discrepancies += 1;
                }
            }
            Err(e) => {
                println!("Trade {}: Validation error: {}", i + 1, e);
            }
        }
    }

    println!();
    println!("{}", "─".repeat(70));
    println!("{}", "  VALIDATION SUMMARY".bright_magenta().bold());
    println!("{}", "─".repeat(70));
    println!("Trades validated: {}", stop_trades.len());
    println!("Discrepancies found: {}", discrepancies);

    if discrepancies > 0 {
        println!(
            "Avg price difference: {:.4}%",
            total_price_diff / discrepancies as f64
        );
        println!();
        println!(
            "{}: Lower-interval validation found price discrepancies.",
            "Note".yellow()
        );
        println!("This means the backtest may be using optimistic fill prices.");
    } else {
        println!();
        println!("{}: All stop/TP fills validated correctly!", "✓".green());
    }

    Ok(())
}

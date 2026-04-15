//! Hall of Fame validator - runs top research candidates and reports results.
//!
//! Validates the strategies documented in HALL_OF_FAME.md:
//! 1. OBV Trend (BTCUSDT 4h) - only profitable strategy in 108-combination sweep
//! 2. Bollinger Reversion (XRP/DOGE 1d) - tight ATR stops
//! 3. Passive execution on FDUSD pairs - 0% maker fees
//!
//! This provides a quick validation of our best research findings.

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::{
        engine::Backtester,
        passive::{PassiveConfig, PassiveExecutor, TickSize},
    },
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const CAPITAL: f64 = 10_000.0;

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(76).bright_cyan());
    println!("{}", "  HALL OF FAME VALIDATOR".bright_cyan().bold());
    println!("{}", "━".repeat(76).bright_cyan());
    println!();

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    // ════════════════════════════════════════════════════════════════════
    // 1. OBV TREND (BTCUSDT 4h)
    // ════════════════════════════════════════════════════════════════════
    println!("{}", "─".repeat(76));
    println!("{}", "  1. OBV TREND — BTCUSDT 4h".yellow().bold());
    println!("{}", "─".repeat(76));
    println!("Hypothesis: OBV leads price. EMA cross on OBV captures institutional flow.");
    println!();

    validate_obv_trend(&loader, &registry).await?;

    // ════════════════════════════════════════════════════════════════════
    // 2. BOLLINGER REVERSION (XRP/DOGE 1d with tight stops)
    // ════════════════════════════════════════════════════════════════════
    println!();
    println!("{}", "─".repeat(76));
    println!(
        "{}",
        "  2. BOLLINGER REVERSION — XRP/DOGE 1d (0.5× ATR Stop)"
            .yellow()
            .bold()
    );
    println!("{}", "─".repeat(76));
    println!("Hypothesis: Strong mean-reversion on daily bars, tight stops cut losers fast.");
    println!();

    validate_bollinger_reversion(&loader, &registry).await?;

    // ════════════════════════════════════════════════════════════════════
    // 3. PASSIVE EXECUTION (FDUSD pairs)
    // ════════════════════════════════════════════════════════════════════
    println!();
    println!("{}", "─".repeat(76));
    println!(
        "{}",
        "  3. PASSIVE EXECUTION — BTCFDUSD 1h (0% Maker Fees)"
            .yellow()
            .bold()
    );
    println!("{}", "─".repeat(76));
    println!("Hypothesis: Passive limit orders + 0% maker fees = massive edge.");
    println!();

    validate_passive_execution(&loader, &registry).await?;

    println!();
    println!("{}", "━".repeat(76).bright_cyan());
    println!("{}", "  VALIDATION COMPLETE".bright_cyan().bold());
    println!("{}", "━".repeat(76).bright_cyan());

    Ok(())
}

async fn validate_obv_trend(loader: &DataLoader, registry: &StrategyRegistry) -> Result<()> {
    let df = loader.fetch_data("BTCUSDT", "4h", 5000).await?;
    let df = FeatureEngine::add_technicals(&df, None)?;

    let strategy = registry
        .create("obv_trend")
        .ok_or_else(|| anyhow::anyhow!("Strategy not found"))?;
    let signals = strategy.predict(&df)?;

    let signal_count = signals
        .f64()?
        .into_no_null_iter()
        .filter(|&s| s.abs() > 0.01)
        .count();

    println!("Data: {} candles | Signals: {}", df.height(), signal_count);
    println!();

    // Test with different stop/TP combinations
    let configs = [
        (0.15, 0.15, "15% SL / 15% TP (documented)"),
        (0.10, 0.10, "10% SL / 10% TP"),
        (0.20, 0.20, "20% SL / 20% TP"),
    ];

    println!(
        "{:<25} {:>12} {:>10} {:>12} {:>10}",
        "Config", "Return%", "Sharpe", "MaxDD%", "Trades"
    );
    println!("{}", "-".repeat(76));

    for (sl, tp, label) in &configs {
        let bt = Backtester::new(CAPITAL, 0.0004, 0.0005); // Taker fees
        let result = bt.run(&df, &signals, *sl, *tp)?;

        let line = format!(
            "{:<25} {:>11.1}% {:>10.2} {:>11.1}% {:>10}",
            label,
            result.total_return_pct,
            result.sharpe_ratio,
            result.max_drawdown_pct,
            result.total_trades
        );

        if result.total_return_pct > 0.0 {
            println!("{}", line.green());
        } else {
            println!("{}", line.red());
        }
    }

    Ok(())
}

async fn validate_bollinger_reversion(
    loader: &DataLoader,
    registry: &StrategyRegistry,
) -> Result<()> {
    let symbols = ["XRPUSDT", "DOGEUSDT"];

    for symbol in &symbols {
        println!("\n{}", format!("  {} 1d", symbol).white().bold());

        let df = match loader.fetch_data(symbol, "1d", 3000).await {
            Ok(d) => FeatureEngine::add_technicals(&d, None)?,
            Err(e) => {
                println!("    ✗ Failed to fetch: {}", e);
                continue;
            }
        };

        let strategy = registry
            .create("bollinger_reversion")
            .ok_or_else(|| anyhow::anyhow!("Strategy not found"))?;
        let signals = strategy.predict(&df)?;

        let signal_count = signals
            .f64()?
            .into_no_null_iter()
            .filter(|&s| s.abs() > 0.01)
            .count();

        println!(
            "    Data: {} candles | Signals: {}",
            df.height(),
            signal_count
        );

        // Test with tight vs wide stops
        let configs = [
            (0.05, 0.0, "0.5× ATR (tight)"),
            (0.10, 0.0, "1.0× ATR (wide)"),
        ];

        println!(
            "    {:<20} {:>12} {:>10} {:>12} {:>10}",
            "Config", "Return%", "Sharpe", "MaxDD%", "Trades"
        );
        println!("    {}", "-".repeat(64));

        for (sl, tp, label) in &configs {
            let bt = Backtester::new(CAPITAL, 0.0004, 0.0005);
            let result = bt.run(&df, &signals, *sl, *tp)?;

            let line = format!(
                "    {:<20} {:>11.1}% {:>10.2} {:>11.1}% {:>10}",
                label,
                result.total_return_pct,
                result.sharpe_ratio,
                result.max_drawdown_pct,
                result.total_trades
            );

            if result.total_return_pct > 0.0 {
                println!("{}", line.green());
            } else {
                println!("{}", line.red());
            }
        }
    }

    Ok(())
}

async fn validate_passive_execution(
    loader: &DataLoader,
    registry: &StrategyRegistry,
) -> Result<()> {
    // Fetch both 1h signal data and 5m fill data
    let df_high = loader.fetch_data("BTCFDUSD", "1h", 1000).await?;
    let df_high = FeatureEngine::add_technicals(&df_high, None)?;

    let df_low = loader.fetch_data("BTCFDUSD", "5m", 12000).await?; // 1000h * 12 bars/h

    let strategy = registry
        .create("rsi_mean_reversion")
        .ok_or_else(|| anyhow::anyhow!("Strategy not found"))?;
    let signals = strategy.predict(&df_high)?;

    let signal_count = signals
        .f64()?
        .into_no_null_iter()
        .filter(|&s| s.abs() > 0.01)
        .count();

    println!(
        "Data: {} candles (1h) | {} candles (5m) | Signals: {}",
        df_high.height(),
        df_low.height(),
        signal_count
    );
    println!();

    // Get tick size
    let tick = TickSize::fetch("BTCFDUSD").await?.value();

    // Passive config
    let passive_cfg = PassiveConfig {
        ticks_below_open: 3,
        tick_size: TickSize::from_value(tick),
        max_wait_bars: 12, // 12 5m bars = 1h
        maker_fee: 0.0,    // FDUSD = 0% maker
        update_threshold_ticks: None,
        anchor_to_signal: true,
    };

    // Run passive executor
    let executor = PassiveExecutor::new(passive_cfg.clone());
    let (passive_signals, fill_stats) = executor
        .process_signals(&df_high, &df_low, &signals)
        .await?;

    println!("Passive Execution Stats:");
    println!("  Fill Rate: {:.1}%", fill_stats.fill_rate * 100.0);
    println!("  Avg Bars to Fill: {:.1}", fill_stats.avg_bars_to_fill);
    println!(
        "  Price Improvement: {:.2} ticks",
        fill_stats.avg_price_improvement_ticks
    );
    println!("  Total Fees Saved: ${:.2}", fill_stats.total_fees_saved);
    println!();

    // Compare: taker vs maker
    println!(
        "{:<25} {:>12} {:>10} {:>12} {:>10}",
        "Execution", "Return%", "Sharpe", "MaxDD%", "Trades"
    );
    println!("{}", "-".repeat(76));

    // Taker (0.1% fee)
    let taker_bt = Backtester::new(CAPITAL, 0.001, 5.0);
    let taker_result = taker_bt.run(&df_high, &signals, 0.15, 0.15)?;

    let line = format!(
        "{:<25} {:>11.1}% {:>10.2} {:>11.1}% {:>10}",
        "Taker (0.1% fee)",
        taker_result.total_return_pct,
        taker_result.sharpe_ratio,
        taker_result.max_drawdown_pct,
        taker_result.total_trades
    );

    if taker_result.total_return_pct > 0.0 {
        println!("{}", line.green());
    } else {
        println!("{}", line.red());
    }

    // Maker (0% fee, passive)
    let maker_bt = Backtester::new(CAPITAL, 0.0, 0.0);
    let maker_result = maker_bt.run(&df_high, &passive_signals, 0.15, 0.15)?;

    let line = format!(
        "{:<25} {:>11.1}% {:>10.2} {:>11.1}% {:>10}",
        "Maker (0% fee)",
        maker_result.total_return_pct,
        maker_result.sharpe_ratio,
        maker_result.max_drawdown_pct,
        maker_result.total_trades
    );

    if maker_result.total_return_pct > 0.0 {
        println!("{}", line.green());
    } else {
        println!("{}", line.red());
    }

    let edge = maker_result.total_return_pct - taker_result.total_return_pct;
    println!();
    println!("Edge from passive execution: {:.1}%", edge);

    Ok(())
}

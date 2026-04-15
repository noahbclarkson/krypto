//! Trade-by-trade validation of paper bot vs backtest engine.
//!
//! Identifies discrepancies between:
//! - Backtest engine (src/backtest/engine.rs)
//! - Paper bot with SignalGeneratorAdapter (src/paper/)
//!
//! Usage:
//!   cargo run --example paper_vs_backtest_validation --profile sweep

use anyhow::Result;
use chrono::{DateTime, Utc};
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::engine::{Backtester, PositionSizing},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
    paper::{Bar, PaperBot, SignalGeneratorAdapter, Strategy},
};

const SYMBOL: &str = "ETHFDUSD";
const INTERVAL: &str = "1d";
const CANDLES: u32 = 500;
const INITIAL_CAPITAL: f64 = 2000.0;
const FEE: f64 = 0.0; // 0% maker
const ATR_MULT: f64 = 0.30;

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

#[derive(Debug, Clone)]
struct TradeComparison {
    bar_idx: usize,
    backtest_action: String,
    paper_action: String,
    match_status: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  PAPER BOT vs BACKTEST VALIDATION".bright_cyan().bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());
    println!(
        "\n{} Symbol: {}, Interval: {}",
        "📊".yellow(),
        SYMBOL,
        INTERVAL
    );

    // Load data
    let loader = DataLoader::new(None, None);
    let raw = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;
    let bars = df_to_bars(&df)?;

    println!("  Bars loaded: {}", bars.len());

    // Generate signals (same for both)
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();
    let signals = strategy.predict(&df)?;

    println!("  Signals generated: {}", signals.len());

    // ── RUN BACKTEST ─────────────────────────────────────────────────────
    let backtester =
        Backtester::new(INITIAL_CAPITAL, FEE, 0.0).with_position_sizing(PositionSizing::Full);

    // Compute trailing_sl as percentage from ATR×0.30
    // For ETH at ~$2000, ATR ~$80, so 0.30×80 = $24 = 1.2%
    let trailing_sl = 0.012; // Approximate 1.2% trailing stop

    let backtest_result = backtester.run(&df, &signals, trailing_sl, 0.0)?;

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  BACKTEST RESULTS".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();
    println!("  Return:      {:>8.1}%", backtest_result.total_return_pct);
    println!("  Sharpe:      {:>8.2}", backtest_result.sharpe_ratio);
    println!("  Max DD:      {:>8.1}%", backtest_result.max_drawdown_pct);
    println!("  Trades:      {:>8}", backtest_result.total_trades);
    println!("  Win Rate:    {:>8.1}%", backtest_result.win_rate);

    // ── RUN PAPER BOT ────────────────────────────────────────────────────
    let strategy2 = registry.create("bollinger_reversion").unwrap();
    let adapter = SignalGeneratorAdapter::new(strategy2, 100, ATR_MULT);
    let mut bot = PaperBot::new(Box::new(adapter), INITIAL_CAPITAL).with_fee(FEE);

    let mut paper_equity_curve = vec![INITIAL_CAPITAL];
    for bar in &bars {
        bot.on_bar(bar);
        paper_equity_curve.push(bot.equity());
    }

    let paper_summary = bot.summary();

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  PAPER BOT RESULTS".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();
    println!("  Return:      {:>8.1}%", paper_summary.total_return_pct);
    println!("  Sharpe:      {:>8.2}", calc_sharpe(&paper_equity_curve));
    println!("  Max DD:      {:>8.1}%", calc_max_dd(&paper_equity_curve));
    println!("  Trades:      {:>8}", paper_summary.total_trades);
    println!("  Win Rate:    {:>8.1}%", paper_summary.win_rate);

    // ── COMPARISON ───────────────────────────────────────────────────────
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  COMPARISON".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    let return_diff = backtest_result.total_return_pct - paper_summary.total_return_pct;
    let trades_diff = backtest_result.total_trades as i64 - paper_summary.total_trades as i64;
    let winrate_diff = backtest_result.win_rate - paper_summary.win_rate;

    println!("  Return Δ:    {:>+8.1}%", return_diff);
    println!("  Trades Δ:    {:>+8}", trades_diff);
    println!("  WinRate Δ:   {:>+8.1}%", winrate_diff);

    // ── TRADE-BY-TRADE ANALYSIS ──────────────────────────────────────────
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  TRADE-BY-TRADE ANALYSIS".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    println!("  Backtest trades (first 10):");
    for (i, trade) in backtest_result.trades.iter().take(10).enumerate() {
        let dir = if trade.direction > 0.0 {
            "LONG "
        } else {
            "SHORT"
        };
        println!(
            "    {} Entry: bar {:3} @ {:.2} → Exit: bar {:3} @ {:.2} ({:+.1}%) [{}]",
            dir,
            trade.entry_bar,
            trade.entry_price,
            trade.exit_bar,
            trade.exit_price,
            trade.pnl_pct * 100.0,
            trade.exit_reason
        );
    }

    // ── SIGNAL INDEXING ANALYSIS ─────────────────────────────────────────
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  SIGNAL INDEXING ANALYSIS".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    // Check signal values at various indices
    let signals_f64 = signals.f64()?;
    println!("  Signal array length: {}", signals_f64.len());
    println!("  First 5 non-zero signals:");

    let mut count = 0;
    for i in 0..signals_f64.len() {
        let sig = signals_f64.get(i).unwrap_or(0.0);
        if sig != 0.0 && count < 5 {
            let prev_sig = if i > 0 {
                signals_f64.get(i - 1).unwrap_or(0.0)
            } else {
                0.0
            };
            println!(
                "    signals[{}] = {:.1}, signals[{}] = {:.1}",
                i,
                sig,
                i - 1,
                prev_sig
            );
            count += 1;
        }
    }

    // ── ROOT CAUSE HYPOTHESIS ────────────────────────────────────────────
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  ROOT CAUSE HYPOTHESIS".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    if trades_diff > 20 {
        println!("  {} MAJOR TRADE COUNT DISCREPANCY", "⚠️".yellow());
        println!();
        println!("  Likely causes:");
        println!("    1. Warmup period: Paper bot needs 20 bars, backtest starts immediately");
        println!("    2. Signal indexing: Paper uses signals[n-2], backtest uses signals[i-1]");
        println!("    3. Stop calculation: Verify trailing stop implementation matches");
    }

    if return_diff.abs() > 50.0 {
        println!();
        println!("  {} MAJOR RETURN DISCREPANCY", "⚠️".yellow());
        println!();
        println!("  Paper bot is using incorrect signal indexing or stop logic.");
    }

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!();

    Ok(())
}

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

    (mean / std) * (252.0_f64).sqrt()
}

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

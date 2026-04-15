//! Full trade-by-trade comparison of paper bot vs backtest.

use anyhow::Result;
use chrono::{DateTime, Utc};
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::engine::{Backtester, PositionSizing, Trade},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
    paper::{Bar, PaperBot, SignalGeneratorAdapter, Strategy},
};

const SYMBOL: &str = "ETHFDUSD";
const INTERVAL: &str = "1d";
const CANDLES: u32 = 500;
const INITIAL_CAPITAL: f64 = 2000.0;
const FEE: f64 = 0.0;
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

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  FULL TRADE-BY-TRADE COMPARISON".bright_cyan().bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());
    println!("\n{} Symbol: {}, Bars: {}", "📊".yellow(), SYMBOL, CANDLES);

    // Load data
    let loader = DataLoader::new(None, None);
    let raw = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;
    let bars = df_to_bars(&df)?;

    // Generate signals
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();
    let signals = strategy.predict(&df)?;

    // ── RUN BACKTEST ─────────────────────────────────────────────────────
    let trailing_sl = 0.012; // 1.2% trailing stop (matches ATR×0.30 for ETH)
    let backtester =
        Backtester::new(INITIAL_CAPITAL, FEE, 0.0).with_position_sizing(PositionSizing::Full);
    let backtest_result = backtester.run(&df, &signals, trailing_sl, 0.0)?;

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  BACKTEST TRADES".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    for (i, trade) in backtest_result.trades.iter().enumerate() {
        let dir = if trade.direction > 0.0 {
            "LONG "
        } else {
            "SHORT"
        };
        let pnl_color = if trade.pnl_pct > 0.0 { "green" } else { "red" };
        println!(
            "  {:2}. {} Bar {:3}->{:<3} @{:<8.2} -> @{:<8.2} | {:>6.1}% | {}",
            i + 1,
            dir,
            trade.entry_bar,
            trade.exit_bar,
            trade.entry_price,
            trade.exit_price,
            trade.pnl_pct * 100.0,
            trade.exit_reason
        );
    }

    // ── RUN PAPER BOT ────────────────────────────────────────────────────
    let strategy2 = registry.create("bollinger_reversion").unwrap();
    let adapter = SignalGeneratorAdapter::new(strategy2, 100, ATR_MULT);
    let mut bot = PaperBot::new(Box::new(adapter), INITIAL_CAPITAL).with_fee(FEE);

    // Track equity changes to detect trades
    let mut paper_trades: Vec<(usize, f64, f64, f64)> = Vec::new(); // (bar_idx, equity_before, equity_after, pct)
    let mut prev_equity = INITIAL_CAPITAL;

    for (idx, bar) in bars.iter().enumerate() {
        bot.on_bar(bar);
        let curr_equity = bot.equity();

        if (curr_equity - prev_equity).abs() > 0.01 {
            let pct = (curr_equity - prev_equity) / prev_equity * 100.0;
            paper_trades.push((idx, prev_equity, curr_equity, pct));
        }
        prev_equity = curr_equity;
    }

    let paper_summary = bot.summary();

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  PAPER BOT TRADES (detected from equity changes)"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    for (i, (bar_idx, eq_before, eq_after, pct)) in paper_trades.iter().enumerate() {
        let color = if *pct > 0.0 { "green" } else { "red" };
        println!(
            "  {:2}. Bar {:3} | ${:.2} -> ${:.2} | {:>+6.1}%",
            i + 1,
            bar_idx,
            eq_before,
            eq_after,
            pct
        );
    }

    // ── COMPARISON ───────────────────────────────────────────────────────
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  COMPARISON".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    println!("  {:<20} {:>15} {:>15}", "Metric", "Backtest", "Paper Bot");
    println!("  {}", "-".repeat(52));
    println!(
        "  {:<20} {:>14.1}% {:>14.1}%",
        "Return", backtest_result.total_return_pct, paper_summary.total_return_pct
    );
    println!(
        "  {:<20} {:>14} {:>14}",
        "Trades", backtest_result.total_trades, paper_summary.total_trades
    );
    println!(
        "  {:<20} {:>14.1}% {:>14.1}%",
        "Win Rate", backtest_result.win_rate, paper_summary.win_rate
    );
    println!(
        "  {:<20} {:>14.1}% {:>14.1}%",
        "Max DD",
        backtest_result.max_drawdown_pct,
        calc_max_dd(&paper_equity_curve(&bars, INITIAL_CAPITAL, FEE, ATR_MULT).unwrap())
    );

    // ── ROOT CAUSE ANALYSIS ──────────────────────────────────────────────
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  ROOT CAUSE ANALYSIS".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!();

    if backtest_result.trades.len() != paper_trades.len() {
        println!("  {} TRADE COUNT MISMATCH", "⚠️".yellow());
        println!("    Backtest: {} trades", backtest_result.trades.len());
        println!("    Paper:    {} trades", paper_trades.len());
        println!();
        println!("  Possible causes:");
        println!("    - Warmup period (paper bot needs 20 bars)");
        println!("    - Signal indexing off-by-one");
        println!("    - Stop loss logic differs");
    }

    // Check first few trades
    println!(
        "\n  First 5 backtest entry bars: {:?}",
        backtest_result
            .trades
            .iter()
            .take(5)
            .map(|t| t.entry_bar)
            .collect::<Vec<_>>()
    );
    println!(
        "  First 5 paper trade bars:    {:?}",
        paper_trades
            .iter()
            .take(5)
            .map(|(bar, _, _, _)| *bar)
            .collect::<Vec<_>>()
    );

    Ok(())
}

fn paper_equity_curve(bars: &[Bar], initial: f64, fee: f64, atr_mult: f64) -> Result<Vec<f64>> {
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();
    let adapter = SignalGeneratorAdapter::new(strategy, 100, atr_mult);
    let mut bot = PaperBot::new(Box::new(adapter), initial).with_fee(fee);

    let mut curve = vec![initial];
    for bar in bars {
        bot.on_bar(bar);
        curve.push(bot.equity());
    }
    Ok(curve)
}

fn calc_max_dd(curve: &[f64]) -> f64 {
    if curve.is_empty() {
        return 0.0;
    }
    let mut peak = curve[0];
    let mut max_dd = 0.0;
    for &eq in curve {
        if eq > peak {
            peak = eq;
        }
        let dd = (peak - eq) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd * 100.0
}

//! Direct comparison between backtest engine and paper bot execution.
//!
//! This example runs both on the same data and compares trade-by-trade
//! to identify where they diverge.

use anyhow::Result;
use chrono::{DateTime, Utc};
use krypto::{
    algo::StrategyRegistry,
    backtest::engine::{Backtester, PositionSizing},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
    paper::{Bar, PaperBot, SignalGeneratorAdapter},
};
use polars::prelude::*;

const SYMBOL: &str = "BTCFDUSD";
const INTERVAL: &str = "1d";
const CANDLES: u32 = 925;
const CAPITAL: f64 = 10_000.0;
const ATR_MULT: f64 = 0.30;

fn df_to_bars(df: &DataFrame) -> Vec<Bar> {
    let time_col = df.column("time").unwrap().datetime().unwrap();
    let open_col = df.column("open").unwrap().f64().unwrap();
    let high_col = df.column("high").unwrap().f64().unwrap();
    let low_col = df.column("low").unwrap().f64().unwrap();
    let close_col = df.column("close").unwrap().f64().unwrap();
    let volume_col = df.column("volume").unwrap().f64().unwrap();

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
    bars
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "=".repeat(80));
    println!("  BACKTEST vs PAPER BOT COMPARISON");
    println!("  Symbol: {}, Interval: {}", SYMBOL, INTERVAL);
    println!("{}", "=".repeat(80));

    // Load data
    let loader = DataLoader::new(None, None);
    let raw = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;

    println!("\nLoaded {} bars", df.height());

    // Create strategy
    let registry = StrategyRegistry::new();
    let strategy = registry.create("bollinger_reversion").unwrap();

    // Generate signals (same for both)
    let signals = strategy.predict(&df)?;
    let signal_vals: Vec<f64> = signals
        .f64()
        .unwrap()
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();

    // Count signals
    let longs = signal_vals.iter().filter(|&&s| s > 0.0).count();
    let shorts = signal_vals.iter().filter(|&&s| s < 0.0).count();
    println!("\nSignal distribution:");
    println!("  Long signals:  {}", longs);
    println!("  Short signals: {}", shorts);
    println!("  Flat signals:  {}", signal_vals.len() - longs - shorts);

    // Show first few signal positions
    let close_col = df.column("close")?.f64()?;
    println!("\n  First 10 signal positions:");
    for i in 0..10 {
        let close_val = close_col.get(i).unwrap_or(0.0);
        println!(
            "    Bar {}: close={:.2}, signal={:.1}",
            i, close_val, signal_vals[i]
        );
    }
    println!("  ...");

    // Find first long and short signals
    let first_long = signal_vals.iter().position(|&s| s > 0.0);
    let first_short = signal_vals.iter().position(|&s| s < 0.0);
    if let Some(idx) = first_long {
        println!(
            "\n  First LONG signal at bar {}, close={:.2}",
            idx,
            close_col.get(idx).unwrap_or(0.0)
        );
    }
    if let Some(idx) = first_short {
        println!(
            "  First SHORT signal at bar {}, close={:.2}",
            idx,
            close_col.get(idx).unwrap_or(0.0)
        );
    }

    // Find bar where close is around 37089
    for i in 0..df.height() {
        let c = close_col.get(i).unwrap_or(0.0);
        if c > 37000.0 && c < 37200.0 {
            println!("\n  Bar {} has close around 37089: {:.2}", i, c);
            if i > 0 {
                println!("  Signal at bar {}: {:.1}", i - 1, signal_vals[i - 1]);
            }
        }
    }

    // Check signals around bar 22-25
    println!("\n  Signals around bar 20-30:");
    for i in 20..30 {
        println!(
            "    Bar {}: close={:.2}, signal={:.1}",
            i,
            close_col.get(i).unwrap_or(0.0),
            signal_vals[i]
        );
    }

    // Check signal at bar 22 and bar 23
    println!("\n  Signal at bar 22: {:.1}", signal_vals[22]);
    println!("  Signal at bar 23: {:.1}", signal_vals[23]);

    // Show bars 20-30 with their signals
    println!("\n  Bars 20-30 with signals:");
    for i in 20..31 {
        let bb_upper = df
            .column("bb_upper")
            .ok()
            .and_then(|c| c.f64().ok())
            .and_then(|c| c.get(i))
            .unwrap_or(0.0);
        let bb_lower = df
            .column("bb_lower")
            .ok()
            .and_then(|c| c.f64().ok())
            .and_then(|c| c.get(i))
            .unwrap_or(0.0);
        let rsi = df
            .column("rsi")
            .ok()
            .and_then(|c| c.f64().ok())
            .and_then(|c| c.get(i))
            .unwrap_or(50.0);
        let close = close_col.get(i).unwrap_or(0.0);
        println!(
            "    Bar {}: close={:.2}, BB_upper={:.2}, BB_lower={:.2}, RSI={:.1}, signal={:.1}",
            i, close, bb_upper, bb_lower, rsi, signal_vals[i]
        );
    }

    // Check if paper bot would see signal at bar 22 when it becomes warm
    println!("\n  Warmup analysis:");
    println!("    With warmup=20, paper bot starts at bar 20");
    println!(
        "    At bar 23: signal_idx=22, signal={:.1}",
        signal_vals[22]
    );
    println!("    If signal is -1.0 and position is 0.0, paper bot should enter SHORT");

    // ── BACKTEST ENGINE ──────────────────────────────────────────────────────
    println!("\n{}", "-".repeat(80));
    println!("  BACKTEST ENGINE");
    println!("{}", "-".repeat(80));

    let bt = Backtester::new(CAPITAL, 0.0, 0.0) // 0% fee, 0 slippage
        .with_position_sizing(PositionSizing::Full);

    // Use ATR×0.30 as trailing stop
    // But wait - backtest uses trailing_sl as a fraction, not ATR multiplier
    // For ATR×0.30 stop, we need to convert ATR to a fraction of price
    let atr = df.column("atr")?.f64()?;
    let close_col = df.column("close")?.f64()?;

    // Get average ATR% to estimate trailing_sl fraction
    let avg_atr_pct: f64 = (0..df.height())
        .map(|i| {
            let a = atr.get(i).unwrap_or(0.0);
            let c = close_col.get(i).unwrap_or(1.0);
            if c > 0.0 {
                a / c
            } else {
                0.0
            }
        })
        .sum::<f64>()
        / df.height() as f64;

    println!("\nAvg ATR%: {:.2}%", avg_atr_pct * 100.0);
    println!(
        "ATR×0.30 = {:.2}% stop distance",
        avg_atr_pct * 0.30 * 100.0
    );

    // Use trailing_sl as ATR×0.30 approximation
    // This is a rough approximation - backtest uses fixed % trailing
    let trailing_sl = avg_atr_pct * ATR_MULT;
    println!("Using trailing_sl = {:.4}", trailing_sl);

    let bt_result = bt.run(&df, &signals, trailing_sl, 0.0)?;

    println!("\nBacktest Results:");
    println!("  Final Equity:   ${:.2}", bt_result.final_equity);
    println!("  Total Return:   {:.1}%", bt_result.total_return_pct);
    println!("  Total Trades:   {}", bt_result.total_trades);
    println!("  Win Rate:       {:.1}%", bt_result.win_rate);
    println!("  Max DD:         {:.1}%", bt_result.max_drawdown_pct);
    println!("  Sharpe:         {:.2}", bt_result.sharpe_ratio);

    // Show first few trades with bar indices
    println!("\n  First 5 trades:");
    for (i, trade) in bt_result.trades.iter().take(5).enumerate() {
        let dir = if trade.direction > 0.0 {
            "LONG "
        } else {
            "SHORT"
        };
        println!(
            "    {} | Bar {}→{} | {} @ {:.2} → @ {:.2} | PnL: {:.2}% | Reason: {}",
            i + 1,
            trade.entry_bar,
            trade.exit_bar,
            dir,
            trade.entry_price,
            trade.exit_price,
            trade.pnl_pct * 100.0,
            trade.exit_reason
        );
    }

    // ── PAPER BOT ─────────────────────────────────────────────────────────────
    println!("\n{}", "-".repeat(80));
    println!("  PAPER BOT (with SignalGeneratorAdapter)");
    println!("{}", "-".repeat(80));

    let bars = df_to_bars(&df);

    let registry2 = StrategyRegistry::new();
    let strategy2 = registry2.create("bollinger_reversion").unwrap();
    let adapter = SignalGeneratorAdapter::new(strategy2, 100, ATR_MULT);

    let mut bot = PaperBot::new(Box::new(adapter), CAPITAL).with_fee(0.0);

    for bar in &bars {
        bot.on_bar(bar);
    }

    let summary = bot.summary();

    println!("\nPaper Bot Results:");
    println!("  Final Equity:   ${:.2}", summary.final_equity);
    println!("  Total Return:   {:.1}%", summary.total_return_pct);
    println!("  Total Trades:   {}", summary.total_trades);
    println!("  Win Rate:       {:.1}%", summary.win_rate);
    println!("  Max DD:         {:.1}%", summary.max_drawdown_pct);

    // Show first few trades
    println!("\n  First 5 trades:");
    for (i, trade) in bot.trades().iter().take(5).enumerate() {
        let dir = if trade.is_long { "LONG " } else { "SHORT" };
        println!(
            "    {} | {} @ {:.2} → @ {:.2} | PnL: {:.2}%",
            i + 1,
            dir,
            trade.entry_price,
            trade.exit_price,
            trade.pnl_pct
        );
    }

    // ── COMPARISON ───────────────────────────────────────────────────────────
    println!("\n{}", "=".repeat(80));
    println!("  COMPARISON");
    println!("{}", "=".repeat(80));

    let bt_return = bt_result.total_return_pct;
    let paper_return = summary.total_return_pct;
    let diff = paper_return - bt_return;

    println!("\n  Backtest Return:  {:>8.1}%", bt_return);
    println!("  Paper Bot Return: {:>8.1}%", paper_return);
    println!("  Difference:       {:>8.1}%", diff);

    println!("\n  Backtest Trades:  {}", bt_result.total_trades);
    println!("  Paper Bot Trades: {}", summary.total_trades);

    if diff.abs() > 10.0 {
        println!("\n  ⚠️  SIGNIFICANT DIVERGENCE DETECTED!");
        println!("  This suggests the paper bot implementation differs from backtest.");
    } else {
        println!("\n  ✓ Results are consistent!");
    }

    Ok(())
}

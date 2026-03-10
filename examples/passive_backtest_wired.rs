//! End-to-end passive execution backtest wired into the main engine.
//!
//! Uses `Backtester::run_with_passive()` to:
//! 1. Simulate limit-order fills on 1m data
//! 2. Run the backtest with 0% maker fees
//! 3. Compare against a standard taker-fee backtest
//!
//! Usage:
//!   cargo run --release --example passive_backtest_wired

use anyhow::Result;
use colored::*;
use krypto::{
    algo::SignalGenerator,
    algo::strategies::*,
    backtest::{
        engine::Backtester,
        passive::{PassiveConfig, TickSize},
    },
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const SYMBOL: &str = "BTCUSDT";
const INTERVAL_HIGH: &str = "1h";
const INTERVAL_LOW: &str = "1m";
const CANDLES_HIGH: u32 = 500;
// 1m candles needed to cover the full higher-TF window: 500 * 60 = 30,000
// fetch_data paginates automatically in chunks of 1,000
const CANDLES_LOW: u32 = 30_000;
const CAPITAL: f64 = 10_000.0;
const TRAILING_STOP: f64 = 0.05;
const TAKE_PROFIT: f64 = 0.15;

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  PASSIVE BACKTEST — Wired into Engine".bright_cyan().bold());
    println!("{}", "  Taker 0.1% vs Passive Maker 0.0%".bright_cyan());
    println!("{}", "━".repeat(72).bright_cyan());

    let loader = DataLoader::new(None, None);

    print!("Fetching {} {} ({} bars)... ", SYMBOL, INTERVAL_HIGH, CANDLES_HIGH);
    let df_high = FeatureEngine::add_technicals(&loader.fetch_data(SYMBOL, INTERVAL_HIGH, CANDLES_HIGH).await?, None)?;
    println!("{}", "✓".green());

    print!("Fetching {} {} ({} bars)... ", SYMBOL, INTERVAL_LOW, CANDLES_LOW);
    let df_low = loader.fetch_data(SYMBOL, INTERVAL_LOW, CANDLES_LOW).await?;
    println!("{}", "✓".green());

    let tick = TickSize::fetch(SYMBOL).await?;
    println!("Tick size: {}", tick.value());

    let strategies: Vec<(&str, Box<dyn SignalGenerator>)> = vec![
        ("dynamic_trend",      Box::new(DynamicTrend::new())),
        ("bollinger",          Box::new(BollingerReversion::new())),
        ("rsi",                Box::new(RsiMeanReversion::new())),
        ("volatility_squeeze", Box::new(VolatilitySqueeze::new())),
        ("macd_trend",         Box::new(MacdTrend::new())),
        ("atr_breakout",       Box::new(AtrBreakout::new())),
    ];

    let passive_cfg = PassiveConfig {
        ticks_below_open: 3,
        tick_size: tick,
        max_wait_bars: 60, // 60 × 1m = 1 full hour to fill
        maker_fee: 0.0,
        update_threshold_ticks: None,
        anchor_to_signal: true,
    };

    let taker_bt = Backtester::new(CAPITAL, 0.001, 5.0);
    let passive_bt = Backtester::new(CAPITAL, 0.0, 0.0);

    println!("\n{:<22} {:>7} {:>10} {:>10} {:>8} {:>10} {:>8}",
        "Strategy", "Trades", "Taker%", "Passive%", "Edge", "Fill%", "AvgBars");
    println!("{}", "─".repeat(80));

    for (name, strat) in strategies {
        let signals = match strat.predict(&df_high) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Standard taker backtest
        let taker = match taker_bt.run(&df_high, &signals, TRAILING_STOP, TAKE_PROFIT) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Passive backtest (wired into engine via run_with_passive)
        let (passive, fill_stats) = match passive_bt
            .run_with_passive(&df_high, &df_low, &signals, TRAILING_STOP, TAKE_PROFIT, passive_cfg.clone())
            .await
        {
            Ok(r) => r,
            Err(e) => { eprintln!("  passive err for {name}: {e}"); continue; }
        };

        if taker.total_trades < 5 { continue; }

        let edge = passive.total_return_pct - taker.total_return_pct;
        let passive_str = if passive.total_return_pct > 0.0 {
            format!("{:>9.1}%", passive.total_return_pct).green().to_string()
        } else {
            format!("{:>9.1}%", passive.total_return_pct).red().to_string()
        };

        println!("{:<22} {:>7} {:>9.1}% {} {:>+7.1}% {:>9.1}% {:>8.1}",
            name,
            taker.total_trades,
            taker.total_return_pct,
            passive_str,
            edge,
            fill_stats.fill_rate * 100.0,
            fill_stats.avg_bars_to_fill,
        );
    }

    println!("{}", "━".repeat(80).bright_cyan());
    println!("\n{}", "Notes:".bright_yellow());
    println!("  • Fill% = % of signals that got a passive fill within {} 1m bars", passive_cfg.max_wait_bars);
    println!("  • AvgBars = average 1m bars until fill");
    println!("  • Edge = passive return - taker return (fee savings + price improvement)");
    println!("  • Passive uses anchor_to_signal=true: limit set once at signal close - {} ticks\n", passive_cfg.ticks_below_open);

    Ok(())
}

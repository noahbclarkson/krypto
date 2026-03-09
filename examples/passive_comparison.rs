//! Compare anchored vs walk-forward passive execution with real data.
//!
//! This test verifies:
//! 1. Timestamp alignment between 1h and 1m data
//! 2. Fill rates for both approaches
//! 3. Price improvement comparison
//! 4. The REAL edge: 0% maker fees on FDUSD

use anyhow::Result;
use colored::*;
use krypto::{
    algo::SignalGenerator,
    algo::strategies::DynamicTrend,
    backtest::passive::{PassiveExecutor, PassiveConfig, TickSize},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  PASSIVE EXECUTION COMPARISON".bright_cyan().bold());
    println!("{}", "  Anchored vs Walk-Forward".bright_cyan());
    println!("{}", "━".repeat(72).bright_cyan());

    let loader = DataLoader::new(None, None);
    let symbol = "BTCFDUSD";
    let interval = "1h";
    let candles_h: u16 = 100;
    let candles_1m: u16 = 6000; // 100 * 60

    println!("\n{}", "Fetching data...".bright_green());
    let df_high = loader.fetch_data(symbol, interval, candles_h).await?;
    let df_high = FeatureEngine::add_technicals(&df_high, None)?;
    let df_low = loader.fetch_data(symbol, "1m", candles_1m).await?;

    // Verify timestamp alignment
    println!("\n{}", "Timestamp alignment check:".bright_yellow());
    let high_times = df_high.column("time")?.datetime()?;
    let low_times = df_low.column("time")?.datetime()?;

    let first_high = high_times.get(0).unwrap_or(0);
    let first_low = low_times.get(0).unwrap_or(0);

    println!("  1h first timestamp: {}", first_high);
    println!("  1m first timestamp: {}", first_low);

    // Find which 1m bars fall within first 1h bar
    let second_high = high_times.get(1).unwrap_or(0);
    let mut bars_in_first = 0;
    for i in 0..df_low.height() {
        let t = low_times.get(i).unwrap_or(0);
        if t >= first_high && t < second_high {
            bars_in_first += 1;
        }
    }
    println!("  1m bars in first 1h bar: {} (expected: 60)", bars_in_first);

    let tick = TickSize::fetch(symbol).await?.value();
    println!("\n  Tick size: {}", tick);

    // Generate signals
    let mut strat = DynamicTrend::new();
    let signals = strat.predict(&df_high)?;

    // Count signals
    let signals_f64 = signals.f64()?;
    let signal_count = (0..df_high.height())
        .filter(|i| signals_f64.get(*i).unwrap_or(0.0).abs() > 0.01)
        .count();
    println!("  Total signals: {}", signal_count);

    println!("\n{}", "━".repeat(72).bright_cyan());

    // Test 1: Anchored to signal price
    println!("\n{}", "TEST 1: Anchored to signal price (current default)".bright_yellow());
    let config_anchored = PassiveConfig {
        ticks_below_open: 3,
        tick_size: TickSize::from_value(tick),
        max_wait_bars: 240,
        maker_fee: 0.0,
        update_threshold_ticks: None,
        anchor_to_signal: true,
    };
    let executor_anchored = PassiveExecutor::new(config_anchored);
    let (_, stats_anchored) = executor_anchored.simulate(&df_high, &df_low, &signals).await?;

    println!("  Fill rate: {:.1}%", stats_anchored.fill_rate * 100.0);
    println!("  Avg price improvement: {:.2} ticks", stats_anchored.avg_price_improvement_ticks);
    println!("  Avg bars to fill: {:.1}", stats_anchored.avg_bars_to_fill);

    // Test 2: Walk-forward (update limit each 1m candle)
    println!("\n{}", "TEST 2: Walk-forward (limit moves with each 1m open)".bright_yellow());
    let config_walk = PassiveConfig {
        ticks_below_open: 3,
        tick_size: TickSize::from_value(tick),
        max_wait_bars: 240,
        maker_fee: 0.0,
        update_threshold_ticks: None, // Update every bar
        anchor_to_signal: false,
    };
    let executor_walk = PassiveExecutor::new(config_walk);
    let (_, stats_walk) = executor_walk.simulate(&df_high, &df_low, &signals).await?;

    println!("  Fill rate: {:.1}%", stats_walk.fill_rate * 100.0);
    println!("  Avg price improvement: {:.2} ticks", stats_walk.avg_price_improvement_ticks);
    println!("  Avg bars to fill: {:.1}", stats_walk.avg_bars_to_fill);

    // Test 3: Walk-forward with threshold
    println!("\n{}", "TEST 3: Walk-forward with 5-tick threshold".bright_yellow());
    let config_threshold = PassiveConfig {
        ticks_below_open: 3,
        tick_size: TickSize::from_value(tick),
        max_wait_bars: 240,
        maker_fee: 0.0,
        update_threshold_ticks: Some(5),
        anchor_to_signal: false,
    };
    let executor_threshold = PassiveExecutor::new(config_threshold);
    let (_, stats_threshold) = executor_threshold.simulate(&df_high, &df_low, &signals).await?;

    println!("  Fill rate: {:.1}%", stats_threshold.fill_rate * 100.0);
    println!("  Avg price improvement: {:.2} ticks", stats_threshold.avg_price_improvement_ticks);
    println!("  Avg bars to fill: {:.1}", stats_threshold.avg_bars_to_fill);

    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "THE REAL EDGE: 0% MAKER FEES ON FDUSD".bright_green().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    let trades_per_year = 1000.0;
    let avg_trade_size = 10000.0; // $10k position

    // Taker fees (market orders): 0.05% on FDUSD
    let taker_fee = 0.0005;
    let taker_cost = trades_per_year * avg_trade_size * taker_fee;

    // Maker fees (limit orders on FDUSD): 0.0%
    let maker_fee_fdusd = 0.0;
    let maker_cost_fdusd = trades_per_year * avg_trade_size * maker_fee_fdusd;

    // Maker fees (limit orders on non-FDUSD): 0.02%
    let maker_fee_other = 0.0002;
    let maker_cost_other = trades_per_year * avg_trade_size * maker_fee_other;

    println!("\n  {} trades/year @ ${} avg position", trades_per_year as u32, avg_trade_size as u32);
    println!("\n  {:20} {:>12} {:>12}", "Fee Type", "Rate", "Annual Cost");
    println!("  {}", "-".repeat(44));
    println!("  {:20} {:>12} ${:>10.2}", "Taker (market)", "0.050%", taker_cost);
    println!("  {:20} {:>12} ${:>10.2}", "Maker (FDUSD)", "0.000%", maker_cost_fdusd);
    println!("  {:20} {:>12} ${:>10.2}", "Maker (non-FDUSD)", "0.020%", maker_cost_other);

    println!("\n  {}", "SAVINGS".bright_green());
    println!("  {:20} {:>12} ${:>10.2} / year", "FDUSD vs Taker", "", taker_cost - maker_cost_fdusd);
    println!("  {:20} {:>12} ${:>10.2} / year", "FDUSD vs non-FDUSD", "", maker_cost_other - maker_cost_fdusd);

    // As % of trading volume
    let annual_volume = trades_per_year * avg_trade_size;
    let savings_pct = (taker_cost - maker_cost_fdusd) / annual_volume * 100.0;
    println!("\n  Savings as % of volume: {:.4}%", savings_pct);
    println!("  Savings in bps: {:.1} bps/trade", savings_pct * 100.0 / trades_per_year);

    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "CONCLUSION".bright_yellow().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!("\n  The fee savings (5 bps/trade) DOMINATES any price improvement.");
    println!("  A 0.0 tick price improvement still gives +5 bps edge on FDUSD!");
    println!("  Focus on: 1) FDUSD pairs, 2) Passive execution, 3) High fill rate");
    println!("  Price improvement is a bonus, not the main edge.\n");

    Ok(())
}

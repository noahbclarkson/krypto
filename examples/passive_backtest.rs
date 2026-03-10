//! Passive backtest: run strategies with real limit order fills + 0% FDUSD fees.
//!
//! This is the "real" backtest — entries happen via limit orders, not market orders.
//! Uses run_passive() which wires PassiveExecutor into the Backtester.

use anyhow::Result;
use colored::*;
use krypto::{
    algo::SignalGenerator,
    algo::strategies::*,
    backtest::engine::Backtester,
    backtest::passive::{PassiveConfig, TickSize},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT"];
const INTERVALS: &[&str] = &["1h", "4h"]; // 1d skipped — requires too many 1m candles
const CANDLES: u32 = 300;   // keep 1m data fetch reasonable
const CAPITAL: f64 = 10_000.0;
const TRAILING_STOP: f64 = 0.12; // 12% — wider than 5%, based on stop sensitivity test
const TAKE_PROFIT: f64 = 0.20;   // 20%
const MIN_TRADES: usize = 10;

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  PASSIVE BACKTEST: Limit orders + 0% FDUSD Maker Fees".bright_cyan().bold());
    println!("{}", "  stop=12%  tp=20%  FDUSD pairs  anchor_to_signal=true".bright_cyan());
    println!("{}", "━".repeat(80).bright_cyan());

    let loader = DataLoader::new(None, None);

    #[allow(clippy::type_complexity)]
    let strategies: Vec<(&str, fn() -> Box<dyn SignalGenerator>)> = vec![
        ("dynamic_trend",      || Box::new(DynamicTrend::new())),
        ("bollinger",          || Box::new(BollingerReversion::new())),
        ("volatility_squeeze", || Box::new(VolatilitySqueeze::new())),
        ("macd_trend",         || Box::new(MacdTrend::new())),
        ("price_momentum",     || Box::new(PriceMomentum::new())),
    ];

    struct Row {
        strategy: &'static str,
        symbol: &'static str,
        interval: &'static str,
        trades: usize,
        fill_rate: f64,
        ret_market: f64,  // market execution, 0% fee
        ret_passive: f64, // passive limit execution, 0% fee
    }

    let mut rows: Vec<Row> = Vec::new();

    // Fetch tick sizes once
    println!("\nFetching tick sizes...");
    let mut tick_sizes = std::collections::HashMap::new();
    for &sym in SYMBOLS {
        let tick = TickSize::fetch(sym).await.unwrap_or(TickSize::from_value(0.01));
        println!("  {} tick: {}", sym, tick.value());
        tick_sizes.insert(sym, tick);
    }

    for symbol in SYMBOLS {
        for interval in INTERVALS {
            let mins_per_bar = match *interval { "1h" => 60u32, "4h" => 240, "1d" => 1440, _ => 60 };
            let candles_1m = CANDLES * mins_per_bar;

            print!("Fetching {} {} + 1m... ", symbol, interval);
            let df_high = match loader.fetch_data(symbol, interval, CANDLES).await {
                Ok(df) => match FeatureEngine::add_technicals(&df, None) {
                    Ok(df) => df,
                    Err(_) => { println!("technicals err"); continue; }
                },
                Err(_) => { println!("fetch err"); continue; }
            };
            let df_low = match loader.fetch_data(symbol, "1m", candles_1m).await {
                Ok(df) => df,
                Err(_) => { println!("1m fetch err"); continue; }
            };
            println!("✓  ({} 1h bars, {} 1m bars)", df_high.height(), df_low.height());

            let tick = tick_sizes.get(symbol).copied().unwrap_or(TickSize::from_value(0.01));

            for (name, make_strat) in &strategies {
                let strat = make_strat();
                let signals = match strat.predict(&df_high) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                // Market backtest (0% fee, for comparison)
                let bt = Backtester::new(CAPITAL, 0.0, 0.0);
                let r_market = match bt.run(&df_high, &signals, TRAILING_STOP, TAKE_PROFIT) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                // Passive backtest
                let passive_cfg = PassiveConfig {
                    ticks_below_open: 3,
                    tick_size: tick,
                    max_wait_bars: mins_per_bar as usize, // 1 full bar window
                    maker_fee: 0.0,
                    update_threshold_ticks: None,
                    anchor_to_signal: true,
                };
                let r_passive = match bt.run_passive(&df_high, &df_low, &signals,
                    TRAILING_STOP, TAKE_PROFIT, passive_cfg).await {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                if r_market.total_trades < MIN_TRADES { continue; }

                // Estimate fill rate from trade count difference
                let fill_rate = if r_market.total_trades > 0 {
                    r_passive.total_trades as f64 / r_market.total_trades as f64
                } else { 0.0 };

                rows.push(Row {
                    strategy: name,
                    symbol,
                    interval,
                    trades: r_passive.total_trades,
                    fill_rate,
                    ret_market: r_market.total_return_pct,
                    ret_passive: r_passive.total_return_pct,
                });
            }
        }
    }

    rows.sort_by(|a, b| b.ret_passive.partial_cmp(&a.ret_passive).unwrap_or(std::cmp::Ordering::Equal));

    println!("\n{}", "━".repeat(90).bright_cyan());
    println!("{:<22} {:<10} {:<4} {:>7} {:>8} {:>10} {:>10} {:>8}",
        "Strategy", "Symbol", "Int", "Trades", "Fill%", "Market%", "Passive%", "Edge");
    println!("{}", "─".repeat(90));

    for r in &rows {
        let _color_fn = if r.ret_passive > 0.0 { "green" } else { "red" };
        let passive_str = if r.ret_passive > 0.0 {
            format!("{:>9.1}%", r.ret_passive).green().to_string()
        } else {
            format!("{:>9.1}%", r.ret_passive).red().to_string()
        };
        println!("{:<22} {:<10} {:<4} {:>7} {:>7.0}% {:>9.1}% {} {:>+7.1}%",
            r.strategy, r.symbol, r.interval,
            r.trades, r.fill_rate * 100.0,
            r.ret_market, passive_str,
            r.ret_passive - r.ret_market);
    }

    let n = rows.len() as f64;
    let avg_market  = rows.iter().map(|r| r.ret_market).sum::<f64>()  / n;
    let avg_passive = rows.iter().map(|r| r.ret_passive).sum::<f64>() / n;
    let avg_fill    = rows.iter().map(|r| r.fill_rate).sum::<f64>()   / n * 100.0;
    let profitable  = rows.iter().filter(|r| r.ret_passive > 0.0).count();

    println!("{}", "━".repeat(90).bright_cyan());
    println!("\n  Runs:               {}", rows.len());
    println!("  Avg fill rate:      {:.1}%", avg_fill);
    println!("  Avg return (market): {:.1}%", avg_market);
    println!("  Avg return (passive): {:.1}%", avg_passive);
    println!("  Profitable (passive): {} / {}", profitable, rows.len());

    Ok(())
}

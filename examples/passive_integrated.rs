//! Integrated passive execution backtest.
//!
//! Shows how to wire PassiveExecutor into a backtest without changing the engine:
//! 1. Fetch both 1h signal data and 1m fill data
//! 2. Generate raw signals from strategy
//! 3. Run passive executor → filtered/resampled signals with real fill times
//! 4. Feed cleaned signals to Backtester::run()
//!
//! This correctly simulates 0% maker fees on FDUSD pairs.

use anyhow::Result;
use colored::*;
use krypto::{
    algo::SignalGenerator,
    algo::strategies::*,
    backtest::{
        engine::{Backtester, BacktestResult},
        passive::{PassiveConfig, PassiveExecutor, TickSize},
    },
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

// FDUSD pairs — 0% maker fee on Binance
const PAIRS: &[(&str, &str)] = &[
    ("BTCFDUSD", "1h"),
    ("BTCFDUSD", "4h"),
    ("BTCFDUSD", "1d"),
    ("SOLFDUSD", "1h"),
    ("SOLFDUSD", "4h"),
    ("SOLFDUSD", "1d"),
    ("ETHFDUSD", "1h"),
    ("ETHFDUSD", "4h"),
    ("ETHFDUSD", "1d"),
];

const CANDLES: u32 = 500;
const CAPITAL: f64 = 10_000.0;
const TRAILING_STOP: f64 = 0.05;
const TAKE_PROFIT: f64 = 0.15;

struct RunResult {
    strategy: &'static str,
    symbol: &'static str,
    interval: &'static str,
    fill_rate: f64,
    // Market execution (taker fee simulation)
    taker: BacktestResult,
    // Passive execution (0% maker fee)
    maker: BacktestResult,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(76).bright_cyan());
    println!("{}", "  INTEGRATED PASSIVE EXECUTION — FDUSD 0% Maker Fees".bright_cyan().bold());
    println!("{}", "━".repeat(76).bright_cyan());
    println!("\n  Strategy → passive executor → 0% fee backtest vs taker baseline");
    println!("  Fill check: 1m data verifies limit order actually hit within the bar\n");

    let loader = DataLoader::new(None, None);

    #[allow(clippy::type_complexity)]
    let strategy_fns: Vec<(&str, fn() -> Box<dyn SignalGenerator>)> = vec![
        ("bollinger",          || Box::new(BollingerReversion::new())),
        ("volatility_squeeze", || Box::new(VolatilitySqueeze::new())),
        ("dynamic_trend",      || Box::new(DynamicTrend::new())),
        ("rsi",                || Box::new(RsiMeanReversion::new())),
        ("price_momentum",     || Box::new(PriceMomentum::new())),
    ];

    let mut results: Vec<RunResult> = Vec::new();

    for (symbol, interval) in PAIRS {
        // Candle counts per timeframe
        let candles_h: u32 = CANDLES;
        let mins_per_bar: u32 = match *interval {
            "1h" => 60,
            "4h" => 240,
            "1d" => 1440,
            _ => 60,
        };
        // 1m data to cover the full backtest window
        let candles_1m = candles_h * mins_per_bar;

        print!("  Fetching {} {} + 1m data... ", symbol, interval);
        let df_high = match loader.fetch_data(symbol, interval, candles_h).await {
            Ok(df) => match FeatureEngine::add_technicals(&df, None) {
                Ok(df) => df,
                Err(e) => { println!("✗ technicals: {e}"); continue; }
            },
            Err(e) => { println!("✗ {e}"); continue; }
        };
        let df_low = match loader.fetch_data(symbol, "1m", candles_1m).await {
            Ok(df) => df,
            Err(e) => { println!("✗ 1m: {e}"); continue; }
        };
        println!("✓ ({} bars high, {} bars 1m)", df_high.height(), df_low.height());

        let tick = TickSize::fetch(symbol).await?.value();

        for (name, make_strat) in &strategy_fns {
            let raw_signals = match make_strat().predict(&df_high) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Skip if too few signals
            let sig_count = raw_signals.f64()?.into_no_null_iter()
                .filter(|&s| s.abs() > 0.01).count();
            if sig_count < 10 { continue; }

            // ── Passive execution filter ────────────────────────────────────
            // This is the core: run the limit order simulator on the raw signals.
            // It verifies via 1m data that each limit would have actually been hit.
            // Signals that weren't filled (gap-open past limit, etc.) are dropped.
            let passive_cfg = PassiveConfig {
                ticks_below_open: 3,
                tick_size: TickSize::from_value(tick),
                max_wait_bars: mins_per_bar as usize,
                maker_fee: 0.0,  // FDUSD = 0% maker
                update_threshold_ticks: None,
                anchor_to_signal: true,
            };
            let executor = PassiveExecutor::new(passive_cfg);
            let (passive_signals, fill_stats) =
                match executor.process_signals(&df_high, &df_low, &raw_signals).await {
                    Ok(r) => r,
                    Err(_) => continue,
                };

            // ── Backtests ───────────────────────────────────────────────────
            // Taker: original signals, 0.1% fee, 5 bps slippage
            let taker_bt = Backtester::new(CAPITAL, 0.001, 5.0);
            let taker = match taker_bt.run(&df_high, &raw_signals, TRAILING_STOP, TAKE_PROFIT) {
                Ok(r) => r,
                Err(_) => continue,
            };

            // Maker: passive-filtered signals, 0% fee, 0 slippage (we set the price)
            let maker_bt = Backtester::new(CAPITAL, 0.0, 0.0);
            let maker = match maker_bt.run(&df_high, &passive_signals, TRAILING_STOP, TAKE_PROFIT) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if maker.total_trades < 5 { continue; }

            results.push(RunResult {
                strategy: name,
                symbol,
                interval,
                fill_rate: fill_stats.fill_rate,
                taker,
                maker,
            });
        }
    }

    // Sort by maker return
    results.sort_by(|a, b| b.maker.total_return_pct.partial_cmp(&a.maker.total_return_pct)
        .unwrap_or(std::cmp::Ordering::Equal));

    println!("\n{}", "━".repeat(76).bright_cyan());
    println!("{:<20} {:<12} {:<6} {:>7} {:>10} {:>10} {:>8} {:>7}",
        "Strategy", "Symbol", "Int", "Fill%", "Taker%", "Maker%", "Edge", "Trades");
    println!("{}", "─".repeat(76));

    for r in &results {
        let maker_str = if r.maker.total_return_pct > 0.0 {
            format!("{:>9.1}%", r.maker.total_return_pct).green().to_string()
        } else {
            format!("{:>9.1}%", r.maker.total_return_pct).red().to_string()
        };
        let edge = r.maker.total_return_pct - r.taker.total_return_pct;
        println!("{:<20} {:<12} {:<6} {:>6.1}% {:>9.1}% {} {:>+7.1}% {:>7}",
            r.strategy, r.symbol, r.interval,
            r.fill_rate * 100.0,
            r.taker.total_return_pct,
            maker_str,
            edge,
            r.maker.total_trades);
    }

    // Profitable count
    let profitable = results.iter().filter(|r| r.maker.total_return_pct > 0.0).count();
    let avg_fill = results.iter().map(|r| r.fill_rate).sum::<f64>() / results.len() as f64;

    println!("{}", "━".repeat(76).bright_cyan());
    println!("\n  Runs: {}  |  Profitable: {}  |  Avg fill rate: {:.1}%",
        results.len(), profitable, avg_fill * 100.0);

    Ok(())
}

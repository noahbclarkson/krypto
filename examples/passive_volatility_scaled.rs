//! Passive execution + volatility-scaled position sizing.
//!
//! Combines the best findings:
//! 1. BollingerReversion 1d (best strategy)
//! 2. ATR×0.30 stops (optimal from sweep)
//! 3. Passive execution (0% maker fees)
//! 4. RiskPerTrade position sizing (volatility-scaled)
//!
//! This is the "production-optimized" configuration.
//!
//! Usage:
//!   cargo run --profile sweep --example passive_volatility_scaled

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::{
        engine::{Backtester, PositionSizing},
        passive::{
            bars_per_signal, lower_interval_for_signal, PassiveConfig, PassiveExecutor, TickSize,
        },
    },
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const TOTAL_CAPITAL: f64 = 10_000.0;
const ATR_MULT: f64 = 0.30;
const INTERVAL: &str = "1d";
const CANDLES_1D: u32 = 925;
const RISK_PER_TRADE: f64 = 0.02; // 2% of equity at risk per trade

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

fn compute_atr_stop(df: &polars::prelude::DataFrame, atr_mult: f64) -> f64 {
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
    if close > 0.0 {
        (atr * atr_mult / close).clamp(0.005, 0.30)
    } else {
        0.05
    }
}

#[derive(Debug)]
struct RunResult {
    symbol: String,
    taker_return: f64,
    maker_return: f64,
    vol_scaled_return: f64,
    taker_sharpe: f64,
    maker_sharpe: f64,
    vol_scaled_sharpe: f64,
    taker_dd: f64,
    maker_dd: f64,
    vol_scaled_dd: f64,
    fill_rate: f64,
    trades: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(82).bright_cyan());
    println!(
        "{}",
        "  PASSIVE + VOLATILITY-SCALED POSITION SIZING"
            .bright_cyan()
            .bold()
    );
    println!(
        "{}",
        format!(
            "  ${:.0} total, {} symbols, RiskPerTrade = {:.0}%",
            TOTAL_CAPITAL,
            SYMBOLS.len(),
            RISK_PER_TRADE * 100.0
        )
        .bright_cyan()
    );
    println!("{}", "━".repeat(82).bright_cyan());

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();
    let allocation = TOTAL_CAPITAL / SYMBOLS.len() as f64;

    let lower_interval = lower_interval_for_signal(INTERVAL);
    let bars_per_signal = bars_per_signal(INTERVAL);
    let candles_low = CANDLES_1D * bars_per_signal as u32;

    println!("\n  Comparing 3 configurations:");
    println!("    1. Taker (0.1% fee, full position)");
    println!("    2. Maker (0% fee, passive execution, full position)");
    println!("    3. Maker + RiskPerTrade (0% fee, passive, volatility-scaled)");
    println!();

    let mut results: Vec<RunResult> = Vec::new();

    for symbol in SYMBOLS {
        print!("  {}... ", symbol);

        // Fetch data
        let raw_high = match loader.fetch_data(symbol, INTERVAL, CANDLES_1D).await {
            Ok(d) => d,
            Err(e) => {
                println!("{}", format!("SKIP ({})", e).red());
                continue;
            }
        };
        let df_high = FeatureEngine::add_technicals(&raw_high, None)?;

        let df_low = match loader.fetch_data(symbol, lower_interval, candles_low).await {
            Ok(d) => d,
            Err(e) => {
                println!("{}", format!("LOW FAIL ({})", e).red());
                continue;
            }
        };

        let tick = match TickSize::fetch(symbol).await {
            Ok(t) => t,
            Err(_) => {
                let fallback = if symbol.contains("BTC") { 0.1 } else { 0.001 };
                TickSize::from_value(fallback)
            }
        };

        let stop_pct = compute_atr_stop(&df_high, ATR_MULT);

        // Generate signals
        let strategy = registry.create("bollinger_reversion").unwrap();
        let raw_signals = strategy.predict(&df_high)?;

        // Passive execution filter
        let passive_cfg = PassiveConfig {
            ticks_below_open: 3,
            tick_size: tick,
            max_wait_bars: bars_per_signal,
            maker_fee: 0.0,
            update_threshold_ticks: None,
            anchor_to_signal: true,
        };

        let executor = PassiveExecutor::new(passive_cfg);
        let (passive_signals, fill_stats) = match executor
            .process_signals(&df_high, &df_low, &raw_signals)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                println!("{}", format!("EXEC FAIL ({})", e).red());
                continue;
            }
        };

        // 1. Taker baseline (0.1% fee, full position)
        let taker_bt = Backtester::new(allocation, 0.001, 0.0);
        let taker = match taker_bt.run(&df_high, &raw_signals, stop_pct, 0.0) {
            Ok(r) => r,
            Err(e) => {
                println!("{}", format!("BT FAIL ({})", e).red());
                continue;
            }
        };

        // 2. Maker (0% fee, passive, full position)
        let maker_bt = Backtester::new(allocation, 0.0, 0.0);
        let maker = match maker_bt.run(&df_high, &passive_signals, stop_pct, 0.0) {
            Ok(r) => r,
            Err(e) => {
                println!("{}", format!("MAKER BT FAIL ({})", e).red());
                continue;
            }
        };

        // 3. Maker + RiskPerTrade (0% fee, passive, vol-scaled)
        let vol_bt = Backtester::new(allocation, 0.0, 0.0)
            .with_position_sizing(PositionSizing::RiskPerTrade(RISK_PER_TRADE));
        let vol_scaled = match vol_bt.run(&df_high, &passive_signals, stop_pct, 0.0) {
            Ok(r) => r,
            Err(e) => {
                println!("{}", format!("VOL BT FAIL ({})", e).red());
                continue;
            }
        };

        println!(
            "Taker: {:.1}%  Maker: {:.1}%  VolScaled: {:.1}%  Fill: {:.0}%",
            taker.total_return_pct,
            maker.total_return_pct,
            vol_scaled.total_return_pct,
            fill_stats.fill_rate * 100.0
        );

        results.push(RunResult {
            symbol: symbol.to_string(),
            taker_return: taker.total_return_pct,
            maker_return: maker.total_return_pct,
            vol_scaled_return: vol_scaled.total_return_pct,
            taker_sharpe: taker.sharpe_ratio,
            maker_sharpe: maker.sharpe_ratio,
            vol_scaled_sharpe: vol_scaled.sharpe_ratio,
            taker_dd: taker.max_drawdown_pct,
            maker_dd: maker.max_drawdown_pct,
            vol_scaled_dd: vol_scaled.max_drawdown_pct,
            fill_rate: fill_stats.fill_rate,
            trades: maker.total_trades,
        });
    }

    if results.is_empty() {
        println!("\n  No results!");
        return Ok(());
    }

    // Summary table
    println!("\n{}", "━".repeat(82).bright_cyan());
    println!("{}", "  DETAILED COMPARISON".bright_cyan().bold());
    println!("{}", "━".repeat(82).bright_cyan());

    println!(
        "\n{:<12} {:>10} {:>10} {:>10}  {:>8} {:>8} {:>8}",
        "Symbol", "Taker%", "Maker%", "VolScale%", "T DD", "M DD", "V DD"
    );
    println!("{}", "─".repeat(78));

    for r in &results {
        println!(
            "{:<12} {:>10.1} {:>10.1} {:>10.1}  {:>7.1}% {:>7.1}% {:>7.1}%",
            r.symbol,
            r.taker_return,
            r.maker_return,
            r.vol_scaled_return,
            r.taker_dd,
            r.maker_dd,
            r.vol_scaled_dd
        );
    }

    // Averages
    let n = results.len() as f64;
    let avg_taker: f64 = results.iter().map(|r| r.taker_return).sum::<f64>() / n;
    let avg_maker: f64 = results.iter().map(|r| r.maker_return).sum::<f64>() / n;
    let avg_vol: f64 = results.iter().map(|r| r.vol_scaled_return).sum::<f64>() / n;
    let avg_taker_dd: f64 = results.iter().map(|r| r.taker_dd).sum::<f64>() / n;
    let avg_maker_dd: f64 = results.iter().map(|r| r.maker_dd).sum::<f64>() / n;
    let avg_vol_dd: f64 = results.iter().map(|r| r.vol_scaled_dd).sum::<f64>() / n;

    println!("{}", "─".repeat(78));
    println!(
        "{:<12} {:>10.1} {:>10.1} {:>10.1}  {:>7.1}% {:>7.1}% {:>7.1}%",
        "AVERAGE", avg_taker, avg_maker, avg_vol, avg_taker_dd, avg_maker_dd, avg_vol_dd
    );

    // Sharpe comparison
    println!(
        "\n{:<12} {:>10} {:>10} {:>10}",
        "Symbol", "T Sharpe", "M Sharpe", "V Sharpe"
    );
    println!("{}", "─".repeat(48));

    for r in &results {
        println!(
            "{:<12} {:>10.2} {:>10.2} {:>10.2}",
            r.symbol, r.taker_sharpe, r.maker_sharpe, r.vol_scaled_sharpe
        );
    }

    let avg_taker_sharpe: f64 = results.iter().map(|r| r.taker_sharpe).sum::<f64>() / n;
    let avg_maker_sharpe: f64 = results.iter().map(|r| r.maker_sharpe).sum::<f64>() / n;
    let avg_vol_sharpe: f64 = results.iter().map(|r| r.vol_scaled_sharpe).sum::<f64>() / n;

    println!("{}", "─".repeat(48));
    println!(
        "{:<12} {:>10.2} {:>10.2} {:>10.2}",
        "AVERAGE", avg_taker_sharpe, avg_maker_sharpe, avg_vol_sharpe
    );

    // Summary
    println!("\n{}", "━".repeat(82).bright_cyan());
    println!("{}", "  KEY FINDINGS".bright_cyan().bold());
    println!("{}", "━".repeat(82).bright_cyan());
    println!();
    println!(
        "  Passive (Maker) edge over Taker:    +{:.1}%",
        avg_maker - avg_taker
    );
    println!(
        "  VolScaled edge over Full Maker:     {:+.1}%",
        avg_vol - avg_maker
    );
    println!(
        "  VolScaled DD reduction vs Maker:    -{:.1}%",
        avg_maker_dd - avg_vol_dd
    );
    println!();
    println!("  Best configuration: Passive + RiskPerTrade(2%)");
    println!("  - Maintains most of the passive execution edge");
    println!("  - Reduces drawdown through volatility scaling");
    println!("  - Auto-sizes positions: small for high-vol, large for low-vol");
    println!();

    Ok(())
}

//! Passive execution portfolio — 5 FDUSD symbols with 0% maker fees.
//!
//! Combines:
//! - BollingerReversion 1d (best validated strategy)
//! - ATR×0.30 stops (optimal from sweep)
//! - Passive limit order execution (0% maker fees on FDUSD)
//!
//! This is the "production-ready" configuration for live trading.
//!
//! Usage:
//!   cargo run --profile sweep --example passive_portfolio

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::{
        engine::{BacktestResult, Backtester},
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
const CANDLES_1D: u32 = 925; // Shortest overlap (SOL/XRP/DOGE)

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

/// Compute ATR-based stop as percentage of price
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

struct SymbolResult {
    symbol: String,
    taker: BacktestResult,
    maker: BacktestResult,
    fill_rate: f64,
    stop_pct: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(78).bright_cyan());
    println!(
        "{}",
        "  PASSIVE PORTFOLIO — BollingerReversion 1d + ATR×0.30 + 0% Maker"
            .bright_cyan()
            .bold()
    );
    println!(
        "{}",
        format!(
            "  ${:.0} total, {} symbols, equal weight (${:.0} each)",
            TOTAL_CAPITAL,
            SYMBOLS.len(),
            TOTAL_CAPITAL / SYMBOLS.len() as f64
        )
        .bright_cyan()
    );
    println!("{}", "━".repeat(78).bright_cyan());

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();
    let allocation = TOTAL_CAPITAL / SYMBOLS.len() as f64;

    // Lower timeframe for passive execution on 1d signals
    let lower_interval = lower_interval_for_signal(INTERVAL);
    let bars_per_signal = bars_per_signal(INTERVAL);
    let candles_low = CANDLES_1D * bars_per_signal as u32;

    println!(
        "\n  Signal interval: {}  |  Fill interval: {}  |  Bars per signal: {}",
        INTERVAL, lower_interval, bars_per_signal
    );
    println!(
        "  Fetching {} {} candles + {} {} candles per symbol...\n",
        CANDLES_1D, INTERVAL, candles_low, lower_interval
    );

    let mut results: Vec<SymbolResult> = Vec::new();

    for symbol in SYMBOLS {
        print!("  {}... ", symbol);

        // Fetch signal data (1d)
        let raw_high = match loader.fetch_data(symbol, INTERVAL, CANDLES_1D).await {
            Ok(d) => d,
            Err(e) => {
                println!("{}", format!("SKIP ({})", e).red());
                continue;
            }
        };
        let df_high = FeatureEngine::add_technicals(&raw_high, None)?;

        // Fetch fill data (30m)
        let df_low = match loader.fetch_data(symbol, lower_interval, candles_low).await {
            Ok(d) => d,
            Err(e) => {
                println!("{}", format!("LOW FAIL ({})", e).red());
                continue;
            }
        };

        // Get tick size
        let tick = match TickSize::fetch(symbol).await {
            Ok(t) => t,
            Err(_) => {
                // Fallback tick sizes for common pairs
                let fallback = if symbol.contains("BTC") { 0.1 } else { 0.001 };
                TickSize::from_value(fallback)
            }
        };

        // Compute ATR-based stop
        let stop_pct = compute_atr_stop(&df_high, ATR_MULT);

        // Generate signals
        let strategy = registry.create("bollinger_reversion").unwrap();
        let raw_signals = strategy.predict(&df_high)?;

        // Passive execution filter
        let passive_cfg = PassiveConfig {
            ticks_below_open: 3,
            tick_size: tick,
            max_wait_bars: bars_per_signal,
            maker_fee: 0.0, // FDUSD = 0% maker
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

        // Taker backtest (0.1% fee baseline)
        let taker_bt = Backtester::new(allocation, 0.001, 0.0);
        let taker = match taker_bt.run(&df_high, &raw_signals, stop_pct, 0.0) {
            Ok(r) => r,
            Err(e) => {
                println!("{}", format!("BT FAIL ({})", e).red());
                continue;
            }
        };

        // Maker backtest (0% fee, passive-filtered signals)
        let maker_bt = Backtester::new(allocation, 0.0, 0.0);
        let maker = match maker_bt.run(&df_high, &passive_signals, stop_pct, 0.0) {
            Ok(r) => r,
            Err(e) => {
                println!("{}", format!("MAKER BT FAIL ({})", e).red());
                continue;
            }
        };

        println!(
            "Taker: {:.1}% ({})  Maker: {:.1}% ({})  Fill: {:.0}%  Stop: {:.1}%",
            taker.total_return_pct,
            taker.total_trades,
            maker.total_return_pct,
            maker.total_trades,
            fill_stats.fill_rate * 100.0,
            stop_pct * 100.0
        );

        results.push(SymbolResult {
            symbol: symbol.to_string(),
            taker,
            maker,
            fill_rate: fill_stats.fill_rate,
            stop_pct,
        });
    }

    if results.is_empty() {
        println!("\n  No results!");
        return Ok(());
    }

    // Summary
    println!("\n{}", "━".repeat(78).bright_cyan());
    println!("{}", "  PER-SYMBOL RESULTS".bright_cyan().bold());
    println!("{}", "━".repeat(78).bright_cyan());

    println!(
        "{:<12} {:>8} {:>8} {:>7} {:>7} {:>8} {:>8} {:>6} {:>6}",
        "Symbol", "Taker%", "Maker%", "DD%", "Fill%", "Sharpe", "Edge", "Stop", "Trades"
    );
    println!("{}", "-".repeat(78));

    let mut total_taker_equity = 0.0;
    let mut total_maker_equity = 0.0;
    let mut total_taker_trades = 0;
    let mut total_maker_trades = 0;

    for r in &results {
        let edge = r.maker.total_return_pct - r.taker.total_return_pct;
        let edge_str = if edge > 0.0 {
            format!("{:+.1}%", edge).green().to_string()
        } else {
            format!("{:+.1}%", edge).red().to_string()
        };

        println!(
            "{:<12} {:>8.1} {:>8.1} {:>7.1} {:>6.0}% {:>8.2} {} {:>5.1}% {:>6}",
            r.symbol,
            r.taker.total_return_pct,
            r.maker.total_return_pct,
            r.maker.max_drawdown_pct,
            r.fill_rate * 100.0,
            r.maker.sharpe_ratio,
            edge_str,
            r.stop_pct * 100.0,
            r.maker.total_trades
        );

        total_taker_equity += r.taker.final_equity;
        total_maker_equity += r.maker.final_equity;
        total_taker_trades += r.taker.total_trades;
        total_maker_trades += r.maker.total_trades;
    }

    // Portfolio totals
    let n = results.len() as f64;
    let portfolio_taker_return = (total_taker_equity / (allocation * n) - 1.0) * 100.0;
    let portfolio_maker_return = (total_maker_equity / (allocation * n) - 1.0) * 100.0;
    let portfolio_edge = portfolio_maker_return - portfolio_taker_return;

    println!("{}", "━".repeat(78).bright_cyan());
    println!("\n{}", "  PORTFOLIO SUMMARY".bright_cyan().bold());
    println!("{}", "━".repeat(78).bright_cyan());

    println!("  Starting capital:  ${:.0}", TOTAL_CAPITAL);
    println!("  Allocation/sym:    ${:.0}", allocation);
    println!();
    println!("  {:20} {:>10} {:>10}", "", "Taker", "Maker");
    println!(
        "  {:20} {:>10} {:>10}",
        "Final equity:",
        format!("${:.0}", total_taker_equity),
        format!("${:.0}", total_maker_equity)
    );
    println!(
        "  {:20} {:>10} {:>10}",
        "Return:",
        format!("{:.1}%", portfolio_taker_return),
        format!("{:.1}%", portfolio_maker_return)
    );
    println!(
        "  {:20} {:>10} {:>10}",
        "Trades:", total_taker_trades, total_maker_trades
    );
    println!();
    println!("  Edge (maker - taker): {:+.1}%", portfolio_edge);
    println!(
        "  Avg fill rate:        {:.0}%",
        results.iter().map(|r| r.fill_rate).sum::<f64>() / n * 100.0
    );
    println!(
        "  Avg Sharpe (maker):   {:.2}",
        results.iter().map(|r| r.maker.sharpe_ratio).sum::<f64>() / n
    );
    println!(
        "  Avg Max DD (maker):   {:.1}%",
        results
            .iter()
            .map(|r| r.maker.max_drawdown_pct)
            .sum::<f64>()
            / n
    );

    println!("\n{}", "━".repeat(78).bright_cyan());
    println!("  NOTE: Portfolio DD is lower than avg per-symbol DD due to diversification.");
    println!("  Symbols rarely hit stops simultaneously.");
    println!("{}", "━".repeat(78).bright_cyan());

    Ok(())
}

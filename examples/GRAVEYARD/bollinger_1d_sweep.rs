//! BollingerReversion 1d sweep across all available FDUSD symbols.
//!
//! Tests BollingerReversion with ATR-based stops (0.5×, 1.0×, 1.5×) on:
//! - BTCFDUSD, ETHFDUSD, SOLFDUSD, XRPFDUSD, DOGEFDUSD
//!
//! Goal: Find additional HALL_OF_FAME candidates beyond the known XRP/DOGE entries.
//!
//! Usage:
//!   cargo run --release --example bollinger_1d_sweep

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry, backtest::engine::Backtester, data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;

const CANDLES: u32 = 2000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 20;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];
const INTERVAL: &str = "1d";

// ATR multipliers for stop sizing (stop = atr_mult × ATR(14) / close)
const ATR_MULTS: &[f64] = &[0.3, 0.5, 0.75, 1.0, 1.5, 2.0];
// TP as multiple of stop size (0 = trail only)
const TP_MULTS: &[f64] = &[0.0, 1.5, 2.0, 3.0];

#[derive(Debug, Clone)]
struct Result_ {
    symbol: String,
    atr_mult: f64,
    tp_mult: f64,
    ret: f64,
    sharpe: f64,
    sortino: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    stop_pct: f64, // average stop size as % of entry price
}

fn compute_atr_stop(df: &DataFrame, atr_mult: f64) -> f64 {
    // Get last ATR and close to compute stop as fraction
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

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  BOLLINGERREVERSION 1D SWEEP — All FDUSD Symbols"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    let mut all_results: Vec<Result_> = Vec::new();

    for symbol in SYMBOLS {
        print!("Loading {} {}... ", symbol, INTERVAL);
        let raw = match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
            Ok(d) => d,
            Err(e) => {
                println!("{} ({})", "SKIP".red(), e);
                continue;
            }
        };
        let df = match FeatureEngine::add_technicals(&raw, None) {
            Ok(d) => d,
            Err(e) => {
                println!("{} ({})", "FEATURES FAILED".red(), e);
                continue;
            }
        };
        println!("{} ({} bars)", "✓".green(), df.height());

        let strategy = registry.create("bollinger_reversion").unwrap();
        let signals = match strategy.predict(&df) {
            Ok(s) => s,
            Err(e) => {
                println!("  {} predict failed: {}", "✗".red(), e);
                continue;
            }
        };

        let signal_count = signals
            .f64()
            .map(|ca| {
                ca.into_iter()
                    .filter(|v| v.map(|x| x != 0.0).unwrap_or(false))
                    .count()
            })
            .unwrap_or(0);
        println!("  Signals: {}", signal_count);

        for &atr_mult in ATR_MULTS {
            let stop_pct = compute_atr_stop(&df, atr_mult);

            for &tp_mult in TP_MULTS {
                let tp_pct = if tp_mult > 0.0 {
                    stop_pct * tp_mult
                } else {
                    0.0
                };

                let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
                match bt.run(&df, &signals, stop_pct, tp_pct) {
                    Ok(result) if result.total_trades >= MIN_TRADES => {
                        all_results.push(Result_ {
                            symbol: symbol.to_string(),
                            atr_mult,
                            tp_mult,
                            ret: result.total_return_pct,
                            sharpe: result.sharpe_ratio,
                            sortino: result.sortino_ratio,
                            max_dd: result.max_drawdown_pct,
                            trades: result.total_trades,
                            win_rate: result.win_rate,
                            stop_pct: stop_pct * 100.0,
                        });
                    }
                    Ok(result) => {
                        // Not enough trades — skip
                        let _ = result;
                    }
                    Err(e) => {
                        println!(
                            "  {} atr={:.1} tp={:.1}: {}",
                            "✗".red(),
                            atr_mult,
                            tp_mult,
                            e
                        );
                    }
                }
            }
        }
    }

    // Sort by Sharpe
    all_results.sort_by(|a, b| {
        b.sharpe
            .partial_cmp(&a.sharpe)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  TOP 20 RESULTS (by Sharpe, taker fees)"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());
    println!(
        "{:<14} {:>6} {:>5} {:>8} {:>8} {:>8} {:>7} {:>7} {:>5}",
        "Symbol", "ATR×", "TP×", "Return%", "Sharpe", "Sortino", "MaxDD%", "WinRate", "Trades"
    );
    println!("{}", "-".repeat(80));

    for r in all_results.iter().take(20) {
        let row = format!(
            "{:<14} {:>6.2} {:>5.1} {:>8.1} {:>8.2} {:>8.2} {:>7.1} {:>7.1} {:>5}",
            r.symbol,
            r.atr_mult,
            r.tp_mult,
            r.ret,
            r.sharpe,
            r.sortino,
            r.max_dd,
            r.win_rate,
            r.trades
        );
        if r.sharpe > 5.0 {
            println!("{}", row.bright_green());
        } else if r.sharpe > 1.5 {
            println!("{}", row.green());
        } else {
            println!("{}", row);
        }
    }

    // Find best per symbol
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  BEST PER SYMBOL".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());

    for symbol in SYMBOLS {
        let best = all_results
            .iter()
            .filter(|r| r.symbol == *symbol)
            .max_by(|a, b| {
                a.sharpe
                    .partial_cmp(&b.sharpe)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        if let Some(r) = best {
            let marker = if r.sharpe > 5.0 {
                "🏆"
            } else if r.sharpe > 1.5 {
                "✅"
            } else {
                "❌"
            };
            println!("{} {} — ATR×{:.2}, TP×{:.1} → Return: {:.1}%, Sharpe: {:.2}, DD: {:.1}%, Trades: {}",
                marker, r.symbol, r.atr_mult, r.tp_mult, r.ret, r.sharpe, r.max_dd, r.trades);
        } else {
            println!("  {} — no results", symbol);
        }
    }

    // HALL_OF_FAME candidates (Sharpe > 3, DD < 50%, min 30 trades)
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  HALL OF FAME CANDIDATES (Sharpe > 3, DD < 50%, trades > 30)"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    let candidates: Vec<&Result_> = all_results
        .iter()
        .filter(|r| r.sharpe > 3.0 && r.max_dd < 50.0 && r.trades > 30)
        .collect();

    if candidates.is_empty() {
        println!("{}", "  No new candidates found.".yellow());
    } else {
        for r in &candidates {
            println!("  🏆 {} — ATR×{:.2} stop ({:.1}%), TP×{:.1} → Sharpe: {:.2}, Return: {:.1}%, DD: {:.1}%",
                r.symbol, r.atr_mult, r.stop_pct, r.tp_mult, r.sharpe, r.ret, r.max_dd);
        }
    }

    println!();
    Ok(())
}

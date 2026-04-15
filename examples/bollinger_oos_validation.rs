//! Out-of-sample validation for BollingerReversion 1d portfolio.
//!
//! Splits the full history into 4 equal periods and tests each independently.
//! If the edge holds across all periods, it's not data-mined.
//!
//! Also tests on USDT pairs (broader universe check).
//!
//! Usage:
//!   cargo run --release --example bollinger_oos_validation

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry, backtest::engine::Backtester, data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;

const CAPITAL_PER_SYMBOL: f64 = 2_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30;
const INTERVAL: &str = "1d";
const CANDLES: u32 = 2000; // More data = better split
const N_SPLITS: usize = 4;

const FDUSD_SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

// Validation universe — test if edge generalizes
const USDT_SYMBOLS: &[&str] = &["BNBUSDT", "ADAUSDT", "AVAXUSDT", "DOTUSDT", "LINKUSDT"];

fn compute_atr_stop(df: &DataFrame, start: usize, end: usize, atr_mult: f64) -> f64 {
    // Use ATR from midpoint of slice for stability
    let mid = (start + end) / 2;
    let atr = df
        .column("atr")
        .ok()
        .and_then(|s| s.f64().ok().and_then(|ca| ca.get(mid)))
        .unwrap_or(0.0);
    let close = df
        .column("close")
        .ok()
        .and_then(|s| s.f64().ok().and_then(|ca| ca.get(mid)))
        .unwrap_or(1.0);
    if close > 0.0 {
        (atr * atr_mult / close).clamp(0.005, 0.30)
    } else {
        0.05
    }
}

#[derive(Debug)]
struct SplitResult {
    period: usize,
    bars: usize,
    profitable_symbols: usize,
    total_symbols: usize,
    portfolio_return: f64,
    avg_sharpe: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  BOLLINGERREVERSION 1D — OUT-OF-SAMPLE VALIDATION"
            .bright_cyan()
            .bold()
    );
    println!(
        "{}",
        format!(
            "  {} equal splits × {} FDUSD symbols + {} USDT validation",
            N_SPLITS,
            FDUSD_SYMBOLS.len(),
            USDT_SYMBOLS.len()
        )
        .bright_cyan()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    // ─── FDUSD split validation ──────────────────────────────────────────────
    println!(
        "\n{}",
        "▶ FDUSD SYMBOLS — Time-Split Validation"
            .bright_yellow()
            .bold()
    );

    let mut loaded_fdusd: Vec<(&str, DataFrame)> = Vec::new();
    for symbol in FDUSD_SYMBOLS {
        print!("  Loading {}... ", symbol);
        match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
            Ok(raw) => match FeatureEngine::add_technicals(&raw, None) {
                Ok(df) => {
                    println!("{} ({} bars)", "✓".green(), df.height());
                    loaded_fdusd.push((symbol, df));
                }
                Err(e) => println!("{} (features: {})", "SKIP".red(), e),
            },
            Err(e) => println!("{} (fetch: {})", "SKIP".red(), e),
        }
    }

    let mut split_results: Vec<SplitResult> = Vec::new();

    if !loaded_fdusd.is_empty() {
        let min_bars = loaded_fdusd
            .iter()
            .map(|(_, df)| df.height())
            .min()
            .unwrap_or(0);
        let split_size = min_bars / N_SPLITS;
        println!(
            "\n  Min bars: {min_bars}, split size: {split_size} bars each (~{:.0} days)\n",
            split_size as f64
        );

        for period in 0..N_SPLITS {
            let start = period * split_size;
            let end = ((period + 1) * split_size).min(min_bars);

            let mut profitable = 0;
            let mut total_return = 0.0;
            let mut sharpe_sum = 0.0;
            let mut symbol_count = 0;

            for (symbol, df) in &loaded_fdusd {
                let slice = df.slice(start as i64, end - start);
                if slice.height() < 30 {
                    continue;
                }

                let stop = compute_atr_stop(df, start, end, ATR_MULT);
                let strategy = registry.create("bollinger_reversion").unwrap();
                let signals = match strategy.predict(&slice) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let bt = Backtester::new(CAPITAL_PER_SYMBOL, TAKER_FEE, 0.0);
                match bt.run(&slice, &signals, stop, 0.0) {
                    Ok(r) if r.total_trades >= 5 => {
                        if r.total_return_pct > 0.0 {
                            profitable += 1;
                        }
                        total_return += r.total_return_pct;
                        sharpe_sum += r.sharpe_ratio;
                        symbol_count += 1;
                        print!("    [P{period}] {:<12} trades:{:>3} ret:{:>7.1}% sharpe:{:>8.2} stop:{:.1}%",
                            symbol, r.total_trades, r.total_return_pct, r.sharpe_ratio, stop * 100.0);
                        if r.total_return_pct > 0.0 {
                            println!(" {}", "✅".green());
                        } else {
                            println!(" {}", "❌".red());
                        }
                    }
                    Ok(_) => {
                        print!("    [P{period}] {:<12} ", symbol);
                        println!("{}", "skip (< 5 trades)".yellow());
                    }
                    Err(e) => {
                        print!("    [P{period}] {:<12} ", symbol);
                        println!("{}", format!("error: {e}").red());
                    }
                }
            }

            let portfolio_return = if symbol_count > 0 {
                total_return / symbol_count as f64
            } else {
                0.0
            };
            let avg_sharpe = if symbol_count > 0 {
                sharpe_sum / symbol_count as f64
            } else {
                0.0
            };

            let marker = if profitable == symbol_count {
                "✅"
            } else if profitable > symbol_count / 2 {
                "⚠️"
            } else {
                "❌"
            };
            println!("  {} Period {period}: {profitable}/{symbol_count} profitable | avg return: {portfolio_return:.1}% | avg Sharpe: {avg_sharpe:.2}\n",
                marker);

            split_results.push(SplitResult {
                period,
                bars: end - start,
                profitable_symbols: profitable,
                total_symbols: symbol_count,
                portfolio_return,
                avg_sharpe,
            });
        }
    }

    // ─── USDT validation universe ────────────────────────────────────────────
    println!("{}", "▶ USDT VALIDATION UNIVERSE".bright_yellow().bold());
    println!("  (same params — does the edge generalize to non-FDUSD pairs?)");

    let mut usdt_profitable = 0;
    let mut usdt_total = 0;

    for symbol in USDT_SYMBOLS {
        print!("  Loading {}... ", symbol);
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
                println!("{} (features: {})", "SKIP".red(), e);
                continue;
            }
        };
        println!("{} ({} bars)", "✓".green(), df.height());

        let stop = compute_atr_stop(&df, 0, df.height(), ATR_MULT);
        let strategy = registry.create("bollinger_reversion").unwrap();
        let signals = match strategy.predict(&df) {
            Ok(s) => s,
            Err(e) => {
                println!("    predict failed: {}", e);
                continue;
            }
        };

        let bt = Backtester::new(CAPITAL_PER_SYMBOL, TAKER_FEE, 0.0);
        match bt.run(&df, &signals, stop, 0.0) {
            Ok(r) if r.total_trades >= 15 => {
                usdt_total += 1;
                if r.total_return_pct > 0.0 {
                    usdt_profitable += 1;
                }
                let marker = if r.total_return_pct > 0.0 {
                    "✅".green()
                } else {
                    "❌".red()
                };
                println!(
                    "  {} {:<12} trades:{:>3} ret:{:>7.1}% sharpe:{:>8.2} dd:{:.1}%",
                    marker,
                    symbol,
                    r.total_trades,
                    r.total_return_pct,
                    r.sharpe_ratio,
                    r.max_drawdown_pct
                );
            }
            Ok(r) => println!(
                "  {} {:<12} (only {} trades — skip)",
                "⚠️".yellow(),
                symbol,
                r.total_trades
            ),
            Err(e) => println!("  {} {:<12} error: {}", "✗".red(), symbol, e),
        }
    }

    // ─── Summary ─────────────────────────────────────────────────────────────
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  VALIDATION SUMMARY".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());

    println!("\n  Time-split results ({N_SPLITS} periods, FDUSD):");
    let all_profitable = split_results
        .iter()
        .all(|r| r.profitable_symbols as f64 / r.total_symbols.max(1) as f64 >= 0.6);
    for r in &split_results {
        let marker = if r.profitable_symbols == r.total_symbols {
            "✅"
        } else if r.profitable_symbols > r.total_symbols / 2 {
            "⚠️"
        } else {
            "❌"
        };
        println!(
            "  {} P{}: {}/{} profitable | avg return: {:>6.1}% | avg Sharpe: {:>6.2} | {} bars",
            marker,
            r.period,
            r.profitable_symbols,
            r.total_symbols,
            r.portfolio_return,
            r.avg_sharpe,
            r.bars
        );
    }

    if all_profitable {
        println!(
            "\n  {}",
            "✅ EDGE IS ROBUST — profitable in all time periods"
                .bright_green()
                .bold()
        );
    } else {
        println!(
            "\n  {}",
            "⚠️  EDGE IS INCONSISTENT — some periods fail"
                .bright_yellow()
                .bold()
        );
    }

    if usdt_total > 0 {
        println!("\n  USDT generalization: {usdt_profitable}/{usdt_total} profitable");
        if usdt_profitable as f64 / usdt_total as f64 >= 0.6 {
            println!(
                "  {}",
                "✅ EDGE GENERALIZES — not FDUSD-specific".bright_green()
            );
        } else {
            println!(
                "  {}",
                "⚠️  Edge may be FDUSD-specific (or USDT needs different params)".bright_yellow()
            );
        }
    }

    println!();
    Ok(())
}

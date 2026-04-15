//! Test different stop sizes to find what actually works on 1h/4h timeframes.
//!
//! Hypothesis: 5% stops are too tight for 1h/4h volatility, getting stopped out
//! before trades play out. Test 5%, 8%, 10%, 12%, 15% stops.

use anyhow::Result;
use colored::*;
use krypto::{
    algo::strategies::*, algo::SignalGenerator, backtest::engine::Backtester,
    data::loader::DataLoader, features::indicators::FeatureEngine,
};

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT"];
const INTERVALS: &[&str] = &["1h", "4h"];
const CANDLES: u32 = 3000;
const CAPITAL: f64 = 10_000.0;
const TAKE_PROFIT: f64 = 0.15; // 15% fixed
const MAKER_FEE: f64 = 0.0; // FDUSD 0%

const STOP_SIZES: &[f64] = &[0.05, 0.08, 0.10, 0.12, 0.15];

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  STOP SIZE SENSITIVITY TEST".bright_cyan().bold());
    println!("{}", "  1h/4h timeframes, 0% maker fees".bright_cyan());
    println!("{}", "━".repeat(72).bright_cyan());

    let loader = DataLoader::new(None, None);

    #[allow(clippy::type_complexity)]
    let strategies: Vec<(&str, fn() -> Box<dyn SignalGenerator>)> = vec![
        ("dynamic_trend", || Box::new(DynamicTrend::new())),
        ("bollinger", || Box::new(BollingerReversion::new())),
        ("volatility_squeeze", || Box::new(VolatilitySqueeze::new())),
        ("macd_trend", || Box::new(MacdTrend::new())),
    ];

    struct Row {
        strategy: &'static str,
        symbol: &'static str,
        interval: &'static str,
        stop_pct: f64,
        trades: usize,
        return_pct: f64,
        win_rate: f64,
        max_dd: f64,
    }

    let mut rows: Vec<Row> = Vec::new();

    for symbol in SYMBOLS {
        for interval in INTERVALS {
            print!("Fetching {} {}... ", symbol, interval);
            let df = match loader.fetch_data(symbol, interval, CANDLES).await {
                Ok(df) => match FeatureEngine::add_technicals(&df, None) {
                    Ok(df) => {
                        println!("✓");
                        df
                    }
                    Err(_) => {
                        println!("technicals err");
                        continue;
                    }
                },
                Err(_) => {
                    println!("fetch err");
                    continue;
                }
            };

            for (name, make_strat) in &strategies {
                let strat = make_strat();
                let signals = match strat.predict(&df) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                for &stop in STOP_SIZES {
                    let bt = Backtester::new(CAPITAL, MAKER_FEE, 0.0);
                    let result = match bt.run(&df, &signals, stop, TAKE_PROFIT) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };

                    if result.total_trades < 15 {
                        continue;
                    }

                    rows.push(Row {
                        strategy: name,
                        symbol,
                        interval,
                        stop_pct: stop * 100.0,
                        trades: result.total_trades,
                        return_pct: result.total_return_pct,
                        win_rate: result.win_rate,
                        max_dd: result.max_drawdown_pct,
                    });
                }
            }
        }
    }

    // Sort by return descending
    rows.sort_by(|a, b| {
        b.return_pct
            .partial_cmp(&a.return_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("\n{}", "━".repeat(90).bright_cyan());
    println!(
        "{:<20} {:<10} {:<4} {:>5} {:>7} {:>10} {:>6} {:>7}",
        "Strategy", "Symbol", "Int", "Stop%", "Trades", "Return%", "Win%", "MaxDD"
    );
    println!("{}", "─".repeat(90));

    for r in rows.iter().take(40) {
        let _ret_color = if r.return_pct > 0.0 { "green" } else { "red" };
        println!(
            "{:<20} {:<10} {:<4} {:>4.0}% {:>7} {:>9.1}% {:>5.0}% {:>6.0}%",
            r.strategy,
            r.symbol,
            r.interval,
            r.stop_pct,
            r.trades,
            r.return_pct,
            r.win_rate,
            r.max_dd
        );
    }

    // Aggregate by stop size
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "AGGREGATE BY STOP SIZE".bright_yellow().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    for &stop in STOP_SIZES {
        let stop_rows: Vec<_> = rows
            .iter()
            .filter(|r| (r.stop_pct - stop * 100.0).abs() < 0.01)
            .collect();
        if stop_rows.is_empty() {
            continue;
        }

        let n = stop_rows.len() as f64;
        let avg_ret = stop_rows.iter().map(|r| r.return_pct).sum::<f64>() / n;
        let profitable = stop_rows.iter().filter(|r| r.return_pct > 0.0).count();
        let avg_trades = stop_rows.iter().map(|r| r.trades as f64).sum::<f64>() / n;

        println!("  {:>4.0}% stop: {:>3} runs, avg trades {:>5.0}, avg return {:>8.1}%, profitable {:>2}/{}",
            stop * 100.0, stop_rows.len(), avg_trades, avg_ret, profitable, stop_rows.len());
    }

    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "CONCLUSION".bright_yellow().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    // Find best stop size
    let best_stop = STOP_SIZES.iter().max_by(|a, b| {
        let avg_a: f64 = rows
            .iter()
            .filter(|r| (r.stop_pct - **a * 100.0).abs() < 0.01)
            .map(|r| r.return_pct)
            .sum::<f64>();
        let avg_b: f64 = rows
            .iter()
            .filter(|r| (r.stop_pct - **b * 100.0).abs() < 0.01)
            .map(|r| r.return_pct)
            .sum::<f64>();
        avg_a
            .partial_cmp(&avg_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if let Some(best) = best_stop {
        println!("\n  Best stop size: {:.0}%", best * 100.0);
    }

    Ok(())
}

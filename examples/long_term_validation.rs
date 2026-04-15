//! Long-term validation on USDT perpetual data (7-8 years)
//!
//! Tests BollingerReversion 1d on XRPUSDT and DOGEUSDT with full history
//! to validate if the edge persists across multiple market cycles.
//!
//! Usage:
//!   cargo run --profile sweep --example long_term_validation

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry, backtest::engine::Backtester, data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const CANDLES: u32 = 5000; // Full history
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;

const SYMBOLS: &[&str] = &["XRPUSDT", "DOGEUSDT"];
const ATR_MULTS: &[f64] = &[0.20, 0.25, 0.30, 0.35, 0.40, 0.50];

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  LONG-TERM VALIDATION — Full USDT History"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    for symbol in SYMBOLS {
        println!("\n{}", format!("▶ {}", symbol).bright_white().bold());

        let raw = match loader.fetch_data(symbol, "1d", CANDLES).await {
            Ok(d) => d,
            Err(e) => {
                println!("  {} Failed to load: {}", "✗".red(), e);
                continue;
            }
        };
        let df = match FeatureEngine::add_technicals(&raw, None) {
            Ok(d) => d,
            Err(e) => {
                println!("  {} Features failed: {}", "✗".red(), e);
                continue;
            }
        };

        // Compute date range
        let n_bars = df.height();
        let years = n_bars as f64 / 365.0;
        println!("  {} bars ({:.1} years)", n_bars, years);

        let strategy = registry.create("bollinger_reversion").unwrap();
        let signals = match strategy.predict(&df) {
            Ok(s) => s,
            Err(e) => {
                println!("  {} Predict failed: {}", "✗".red(), e);
                continue;
            }
        };

        let sig_vals = signals.f64().unwrap();
        let n_longs = sig_vals
            .into_iter()
            .filter(|v| v.map(|x| x > 0.0).unwrap_or(false))
            .count();
        let n_shorts = sig_vals
            .into_iter()
            .filter(|v| v.map(|x| x < 0.0).unwrap_or(false))
            .count();
        println!(
            "  Signals: {} longs, {} shorts ({} total)",
            n_longs,
            n_shorts,
            n_longs + n_shorts
        );

        println!(
            "\n  {:<8} {:>10} {:>10} {:>10} {:>8} {:>8}",
            "ATR×", "Return%", "Sharpe", "MaxDD%", "Trades", "WinRate"
        );
        println!("  {}", "-".repeat(60));

        let mut best_sharpe = 0.0;
        let mut best_mult = 0.0;

        for &atr_mult in ATR_MULTS {
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
            let stop_pct = if close > 0.0 {
                (atr * atr_mult / close).clamp(0.005, 0.30)
            } else {
                0.05
            };

            let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
            match bt.run(&df, &signals, stop_pct, 0.0) {
                Ok(result) if result.total_trades >= 30 => {
                    let marker = if result.sharpe_ratio > best_sharpe {
                        best_sharpe = result.sharpe_ratio;
                        best_mult = atr_mult;
                        "🏆"
                    } else if result.sharpe_ratio > 1.0 {
                        "✅"
                    } else {
                        "  "
                    };
                    println!(
                        "  {} {:<8.2} {:>10.1} {:>10.2} {:>10.1} {:>8} {:>8.1}%",
                        marker,
                        atr_mult,
                        result.total_return_pct,
                        result.sharpe_ratio,
                        result.max_drawdown_pct,
                        result.total_trades,
                        result.win_rate
                    );
                }
                Ok(result) => {
                    println!(
                        "    {:<8.2} {:>10} (only {} trades)",
                        atr_mult, "SKIP", result.total_trades
                    );
                }
                Err(e) => {
                    println!("    {:<8.2} ERROR: {}", atr_mult, e);
                }
            }
        }

        println!(
            "\n  Best: ATR×{:.2} with Sharpe {:.2}",
            best_mult, best_sharpe
        );
    }

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("  ANALYSIS");
    println!("{}", "━".repeat(80).bright_cyan());
    println!("Compare long-term USDT results to FDUSD short-term results.");
    println!("If ATR×0.20-0.30 works on 7-8 years, edge is more robust.");
    println!("If edge disappears on longer data, overfitting is likely.");

    Ok(())
}

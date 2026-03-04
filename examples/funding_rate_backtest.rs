//! Funding Rate Mean-Reversion Backtest
//!
//! Fetches real Binance perpetual funding rate history + spot OHLCV,
//! aligns them, then runs walk-forward validation on FundingRateReversion.
//!
//! Usage: cargo run --example funding_rate_backtest --release

use colored::*;
use krypto::algo::strategies::FundingRateReversion;
use krypto::backtest::walk_forward::{WalkForwardBacktester, WalkForwardConfig};
use krypto::data::funding_rate::FundingRateLoader;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use std::path::PathBuf;

const CACHE_DIR: &str = "examples/cache";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("{}", "═══ FUNDING RATE MEAN-REVERSION BACKTEST ═══".cyan().bold());
    println!("Fetching Binance perpetual funding rates + 1h OHLCV...");
    println!();

    let symbols = vec![
        ("BTCUSDT", "BTCFDUSD"),
        ("ETHUSDT", "ETHFDUSD"),
        ("SOLUSDT", "SOLFDUSD"),
    ];

    let price_loader = DataLoader::new(None, None);
    let fr_loader = FundingRateLoader::with_cache(PathBuf::from(CACHE_DIR));

    for (futures_sym, spot_sym) in &symbols {
        println!("{}", format!("── {} ──", futures_sym).yellow().bold());

        // Fetch funding rate history (paginated, cached after first fetch)
        print!("  Fetching funding rates for {}...", futures_sym);
        let rates = match fr_loader.fetch_all(futures_sym).await {
            Ok(r) => {
                println!(" {} records", r.len());
                r
            }
            Err(e) => {
                println!(" ERROR: {}", e);
                continue;
            }
        };

        let stats = FundingRateLoader::compute_stats(&rates);
        println!(
            "  Funding stats: mean={:.4}%  std={:.4}%  p5={:.4}%  p95={:.4}%",
            stats.mean * 100.0,
            stats.std * 100.0,
            stats.p5 * 100.0,
            stats.p95 * 100.0
        );

        // Fetch price OHLCV (1h, max history)
        print!("  Fetching 1h OHLCV for {}...", spot_sym);
        let price_df = match price_loader.fetch_data(spot_sym, "1h", 10_000).await {
            Ok(df) => {
                println!(" {} bars", df.height());
                df
            }
            Err(e) => {
                println!(" ERROR: {}", e);
                continue;
            }
        };

        // Add technical indicators
        let df_tech = FeatureEngine::add_technicals(&price_df, None)?;

        // Align funding rates to price data
        let df_with_funding = match fr_loader.align_to_ohlcv(&rates, &df_tech, "1h") {
            Ok(df) => df,
            Err(e) => {
                println!("  Failed to align funding rates: {}", e);
                continue;
            }
        };

        println!(
            "  DataFrame: {} rows with funding_rate, funding_rate_z, funding_rate_ma8",
            df_with_funding.height()
        );

        // Run walk-forward validation
        let config = WalkForwardConfig {
            train_bars: 4000,  // ~6 months of 1h data
            test_bars: 1500,   // ~2 months
            optimizer_iterations: 300,
            monte_carlo_n: 300,
            ..Default::default()
        };

        let mut strategy = FundingRateReversion::default();
        let wf = WalkForwardBacktester::new(config);

        println!("  Running walk-forward validation...");
        match wf.run(&mut strategy, &df_with_funding) {
            Ok(result) => {
                let status = if result.is_robust {
                    "✅ ROBUST".green().bold()
                } else {
                    "❌ not robust".red()
                };
                println!("  Result: {}", status);
                result.print_summary();

                // Save equity curve
                if !result.equity_curve.is_empty() {
                    let csv: String = std::iter::once("equity".to_string())
                        .chain(result.equity_curve.iter().map(|e| format!("{:.6}", e)))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let path = format!("funding_equity_{}.csv", futures_sym.to_lowercase());
                    std::fs::write(&path, csv)?;
                    println!("  Equity curve saved to {}", path);
                }
            }
            Err(e) => {
                println!("  Walk-forward error: {}", e);
            }
        }
        println!();
    }

    Ok(())
}

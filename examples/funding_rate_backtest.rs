//! Funding Rate Mean Reversion — Walk-Forward Validation
//!
//! Tests FundingRateReversion on BTC/ETH/SOL perpetual futures (BTCUSDT etc.)
//! using real funding rate data from Binance.
//!
//! The strategy shorts when funding is extreme-positive (longs are squeezed)
//! and goes long when funding is extreme-negative (shorts are squeezed).

use colored::*;
use krypto::algo::strategies::FundingRateReversion;
use krypto::backtest::walk_forward::{WalkForwardBacktester, WalkForwardConfig};
use krypto::data::funding_rate::FundingRateLoader;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;

const CACHE_DIR: &str = "examples/cache/funding";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("{}", "═══ FUNDING RATE MEAN REVERSION BACKTEST ═══".cyan().bold());
    println!("Strategy: short on extreme positive funding, long on extreme negative");
    println!("Data: Binance perpetual futures (fapi) — 8h funding intervals");
    println!();

    std::fs::create_dir_all(CACHE_DIR)?;

    // Note: perpetual futures use BTCUSDT (not BTCFDUSD) — same price, different market
    let symbols = vec!["BTCUSDT", "ETHUSDT", "SOLUSDT"];
    // 4h bars: funding updates every 8h = every 2 bars — good alignment
    let interval = "4h";
    let limit: u16 = 5000; // ~2.3 years of 4h data

    let price_loader = DataLoader::new(None, None);
    let fr_loader = FundingRateLoader::with_cache(CACHE_DIR);

    // Walk-forward config: 1000-bar train (~170 days), 400-bar test (~67 days)
    let wf_config = WalkForwardConfig {
        train_bars: 1000,
        test_bars: 400,
        optimizer_iterations: 300,
        monte_carlo_n: 300,
        min_train_trades: 15, // funding events are rarer than price crossovers
        min_test_trades: 5,
        ..Default::default()
    };

    let mut any_robust = false;

    for symbol in &symbols {
        println!("{}", format!("── {} ──", symbol).yellow().bold());

        // Load OHLCV from perpetual futures endpoint
        let price_df = match price_loader.fetch_data(symbol, interval, limit).await {
            Ok(df) => df,
            Err(e) => {
                println!("  ❌ Failed to fetch price data: {}", e);
                continue;
            }
        };
        println!("  Loaded {} price bars", price_df.height());

        // Load full funding rate history
        let funding_records = match fr_loader.fetch_all(symbol).await {
            Ok(r) => r,
            Err(e) => {
                println!("  ❌ Failed to fetch funding rates: {}", e);
                continue;
            }
        };
        println!("  Loaded {} funding rate records", funding_records.len());

        if funding_records.is_empty() {
            println!("  ⚠️ No funding data — skipping");
            continue;
        }

        // Print funding rate stats
        let stats = FundingRateLoader::compute_stats(&funding_records);
        println!(
            "  Funding stats: mean={:.4}%  std={:.4}%  min={:.4}%  max={:.4}%",
            stats.mean * 100.0,
            stats.std * 100.0,
            stats.min * 100.0,
            stats.max * 100.0
        );

        // Align funding rates to price bars
        let df_with_funding = match fr_loader.align_to_ohlcv(&funding_records, &price_df, interval) {
            Ok(df) => df,
            Err(e) => {
                println!("  ❌ Failed to align funding rates: {}", e);
                continue;
            }
        };

        // Add technical features (RSI needed for confirm filter)
        let df_tech = match FeatureEngine::add_technicals(&df_with_funding, None) {
            Ok(df) => df,
            Err(e) => {
                println!("  ❌ Failed to compute features: {}", e);
                continue;
            }
        };

        // Run walk-forward
        let wf = WalkForwardBacktester::new(wf_config.clone());
        let mut strategy = FundingRateReversion::default();

        match wf.run(&mut strategy, &df_tech) {
            Ok(result) => {
                result.print_summary();

                if result.is_robust {
                    any_robust = true;
                    println!(
                        "{}",
                        format!(
                            "✅ {} PASSED — OOS return: {:.1}% | Sharpe: {:.3} | MC_p: {:.3}",
                            symbol,
                            result.combined_total_return_pct,
                            result.combined_sharpe,
                            result.avg_monte_carlo_p.unwrap_or(f64::NAN)
                        )
                        .green()
                        .bold()
                    );
                } else {
                    println!(
                        "{}",
                        format!(
                            "❌ {} failed — OOS return: {:.1}% | Sharpe: {:.3} | wins: {}/{}",
                            symbol,
                            result.combined_total_return_pct,
                            result.combined_sharpe,
                            result.windows_passed,
                            result.windows_total
                        )
                        .red()
                    );
                }
            }
            Err(e) => println!("  ❌ Walk-forward failed: {}", e),
        }

        println!();
    }

    if !any_robust {
        println!(
            "{}",
            "No symbols passed. Funding rate mean-reversion may need further refinement \
             or a different entry threshold for current market conditions."
                .yellow()
        );
    }

    Ok(())
}

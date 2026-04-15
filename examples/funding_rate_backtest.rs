//! Funding Rate Mean Reversion — Walk-Forward Backtest
//!
//! Fetches Binance perpetual funding rate history, aligns it to OHLCV data,
//! and runs FundingRateReversion through strict walk-forward validation.
//!
//! Funding rate mean reversion is Tier 1 structural alpha:
//! - Grounded in real market mechanics (longs/shorts paying to hold positions)
//! - Extreme funding = crowded trade → mean reversion expected
//! - Not a curve-fitted technical indicator pattern

use colored::*;
use krypto::algo::strategies::FundingRateReversion;
use krypto::backtest::walk_forward::{WalkForwardBacktester, WalkForwardConfig};
use krypto::data::funding_rate::FundingRateLoader;
use krypto::data::{align_to_ohlcv, DataLoader};
use krypto::features::indicators::FeatureEngine;

// Funding is for USDT-M perpetuals — use USDT pairs
const SYMBOLS: &[(&str, &str)] = &[
    ("BTCUSDT", "BTCFDUSD"),
    ("ETHUSDT", "ETHFDUSD"),
    ("SOLUSDT", "SOLFDUSD"),
];
const INTERVALS: &[&str] = &["1h", "4h"];
const OHLCV_LIMIT: u32 = 10_000;
/// Funding z-score window: 90 periods × 8h = 30 days of rolling baseline
const Z_WINDOW: usize = 90;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!(
        "{}",
        "═══ FUNDING RATE MEAN REVERSION — WALK-FORWARD ═══"
            .cyan()
            .bold()
    );
    println!("Strategy: FundingRateReversion (contrarian entry on extreme funding z-score)");
    println!("Gates: train_sharpe>0.05, pf>1.2, train_trades>20, OOS_trades>10, robustness>0.4");
    println!();

    let funding_loader = FundingRateLoader::with_cache_dir("examples/funding_cache");
    let ohlcv_loader = DataLoader::new(None, None);

    for (perp_symbol, spot_symbol) in SYMBOLS {
        println!("{}", format!("── {} ──", perp_symbol).yellow().bold());

        // Fetch funding rate history (cached after first run)
        println!("  Fetching funding rates for {}...", perp_symbol);
        let funding_df = funding_loader.fetch(perp_symbol, None, None).await?;
        println!("  {} funding rate records", funding_df.height());

        for interval in INTERVALS {
            // Fetch/cache OHLCV
            let (ohlcv_df, cached) = {
                let path = std::path::PathBuf::from(format!(
                    "examples/cache/{spot_symbol}_{interval}_{OHLCV_LIMIT}.bin"
                ));
                if path.exists() {
                    // Use existing cache
                    (
                        ohlcv_loader
                            .fetch_data(spot_symbol, interval, OHLCV_LIMIT)
                            .await?,
                        true,
                    )
                } else {
                    (
                        ohlcv_loader
                            .fetch_data(spot_symbol, interval, OHLCV_LIMIT)
                            .await?,
                        false,
                    )
                }
            };

            print!(
                "  {} {} ({} candles{}) — ",
                spot_symbol,
                interval,
                ohlcv_df.height(),
                if cached { ", cached" } else { "" }
            );

            // Add technical indicators first
            let df_tech = match FeatureEngine::add_technicals(&ohlcv_df, None) {
                Ok(df) => df,
                Err(e) => {
                    println!("ERROR adding technicals: {e}");
                    continue;
                }
            };

            // Align funding rate to OHLCV timestamps
            let df_with_funding = match align_to_ohlcv(&df_tech, &funding_df, Z_WINDOW) {
                Ok(df) => df,
                Err(e) => {
                    println!("ERROR aligning funding: {e}");
                    continue;
                }
            };

            // Walk-forward config tuned per interval
            let cfg = match *interval {
                "1h" => WalkForwardConfig {
                    train_bars: 4000,
                    test_bars: 1500,
                    optimizer_iterations: 200,
                    monte_carlo_n: 200,
                    ..Default::default()
                },
                "4h" => WalkForwardConfig {
                    train_bars: 1500,
                    test_bars: 500,
                    optimizer_iterations: 200,
                    monte_carlo_n: 200,
                    ..Default::default()
                },
                _ => WalkForwardConfig::default(),
            };

            let wf = WalkForwardBacktester::new(cfg);
            let mut strat = FundingRateReversion::default();

            match wf.run(&mut strat, &df_with_funding) {
                Ok(result) => {
                    if result.is_robust {
                        println!(
                            "{} wins={}/{} OOS_sh={:.3} OOS_ret={:.1}% MC_p={:.3}",
                            "✅ ROBUST".green().bold(),
                            result.windows_passed,
                            result.windows_total,
                            result.avg_test_sharpe,
                            result.avg_test_return_pct,
                            result.avg_monte_carlo_p.unwrap_or(f64::NAN)
                        );
                        result.print_summary();
                    } else {
                        println!(
                            "{} wins={}/{} OOS_sh={:.3} OOS_ret={:.1}%",
                            "❌ not robust".red(),
                            result.windows_passed,
                            result.windows_total,
                            result.avg_test_sharpe,
                            result.avg_test_return_pct
                        );
                    }
                }
                Err(e) => println!("ERROR: {e}"),
            }
        }
        println!();
    }

    Ok(())
}

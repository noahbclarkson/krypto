//! Cross-Sectional Momentum — Walk-Forward Backtest
//!
//! Loads OHLCV data for all 5 symbols simultaneously, computes cross-sectional
//! momentum rank features (which asset has strongest recent relative performance),
//! then runs CrossSectionalMomentum strategy through walk-forward validation.
//!
//! This is a Tier 2 academically-grounded strategy:
//! - Proven in equities literature (Jegadeesh & Titman 1993)
//! - Documented to work in crypto (e.g., Liu et al. 2022)
//! - Trades relative strength, not absolute levels → inherently adaptive

use colored::*;
use krypto::algo::strategies::CrossSectionalMomentum;
use krypto::backtest::walk_forward::{WalkForwardBacktester, WalkForwardConfig};
use krypto::data::DataLoader;
use krypto::features::compute_cs_features;
use krypto::features::indicators::FeatureEngine;
use std::collections::HashMap;
use std::path::PathBuf;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "DOGEFDUSD", "XRPFDUSD"];
const OHLCV_LIMIT: u16 = 10_000;
const CACHE_DIR: &str = "examples/cache";

/// Load from cache (bin file from advanced_backtest example format) or API
async fn load_cached(
    loader: &DataLoader,
    symbol: &str,
    interval: &str,
) -> anyhow::Result<polars::frame::DataFrame> {
    let path = PathBuf::from(CACHE_DIR).join(format!("{symbol}_{interval}_{OHLCV_LIMIT}.bin"));

    if path.exists() {
        // Re-fetch via API (cache file format from examples isn't parquet, so just re-use API)
        // For now: direct fetch — will be cached by DataLoader's internal cache if configured
    }

    loader.fetch_data(symbol, interval, OHLCV_LIMIT).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("{}", "═══ CROSS-SECTIONAL MOMENTUM — WALK-FORWARD ═══".cyan().bold());
    println!("Strategy: CrossSectionalMomentum (rank assets by N-bar return, long top/short bottom)");
    println!("Universe: {} symbols", SYMBOLS.len());
    println!();

    let loader = DataLoader::new(None, None);

    for interval in &["4h", "1h"] {
        println!("{}", format!("── Interval: {interval} ──").yellow().bold());

        // Load all symbols
        let mut raw_dfs: HashMap<String, polars::frame::DataFrame> = HashMap::new();
        for sym in SYMBOLS {
            match load_cached(&loader, sym, interval).await {
                Ok(df) => {
                    match FeatureEngine::add_technicals(&df, None) {
                        Ok(df_tech) => {
                            raw_dfs.insert(sym.to_string(), df_tech);
                        }
                        Err(e) => eprintln!("  Warning: technicals failed for {sym}: {e}"),
                    }
                }
                Err(e) => eprintln!("  Warning: failed to load {sym}: {e}"),
            }
        }

        if raw_dfs.len() < 2 {
            println!("  Not enough assets loaded for cross-sectional ranking. Skipping.");
            continue;
        }

        // Try different momentum periods
        for momentum_period in &[20usize, 50, 100] {
            println!("  Momentum period: {} bars", momentum_period);

            // Compute cross-sectional rank features across all assets
            let enriched = match compute_cs_features(&raw_dfs, *momentum_period) {
                Ok(e) => e,
                Err(err) => {
                    println!("  ERROR computing CS features: {err}");
                    continue;
                }
            };

            // Walk-forward config
            let cfg = match *interval {
                "1h" => WalkForwardConfig {
                    train_bars: 4000,
                    test_bars: 1500,
                    optimizer_iterations: 150,
                    monte_carlo_n: 150,
                    ..Default::default()
                },
                _ => WalkForwardConfig {
                    train_bars: 1500,
                    test_bars: 500,
                    optimizer_iterations: 150,
                    monte_carlo_n: 150,
                    ..Default::default()
                },
            };

            // Run walk-forward on each asset using its enriched DataFrame
            let mut any_robust = false;
            for sym in SYMBOLS {
                let df = match enriched.get(*sym) {
                    Some(d) => d,
                    None => continue,
                };

                let wf = WalkForwardBacktester::new(cfg.clone());
                let mut strat = CrossSectionalMomentum::default();

                match wf.run(&mut strat, df) {
                    Ok(result) => {
                        if result.is_robust {
                            any_robust = true;
                            println!(
                                "    {} {} {} wins={}/{} OOS_sh={:.3} OOS_ret={:.1}% MC_p={:.3}",
                                "✅ ROBUST".green().bold(),
                                sym,
                                interval,
                                result.windows_passed,
                                result.windows_total,
                                result.avg_test_sharpe,
                                result.avg_test_return_pct,
                                result.avg_monte_carlo_p.unwrap_or(f64::NAN)
                            );
                            result.print_summary();
                        } else {
                            println!(
                                "    {} {} wins={}/{} OOS_sh={:.3} OOS_ret={:.1}%",
                                "❌".red(),
                                sym,
                                result.windows_passed,
                                result.windows_total,
                                result.avg_test_sharpe,
                                result.avg_test_return_pct
                            );
                        }
                    }
                    Err(e) => println!("    ERROR {sym}: {e}"),
                }
            }

            if !any_robust {
                println!("    No assets robust with momentum_period={momentum_period}");
            }
            println!();
        }
    }

    Ok(())
}

//! Cross-Sectional Momentum — Walk-Forward Backtest
//!
//! Evaluates the CrossSectionalMomentum strategy across a basket of assets.
//! Instead of trading an asset based on its own history alone, it compares
//! performance across BTC, ETH, SOL, DOGE, and XRP, going long the leaders
//! and short the laggards.

use colored::*;
use krypto::algo::strategies::CrossSectionalMomentum;
use krypto::backtest::walk_forward::{WalkForwardBacktester, WalkForwardConfig};
use krypto::data::universe::{compute_cross_sectional_features, Universe};
use krypto::features::indicators::FeatureEngine;


const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "DOGEFDUSD", "XRPFDUSD"];
const INTERVALS: &[&str] = &["4h", "1h"];
const LIMIT: u32 = 10_000;
const CS_LOOKBACK: usize = 42; // e.g. 7 days of 4h data

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("{}", "═══ CROSS-SECTIONAL MOMENTUM — WALK-FORWARD ═══".cyan().bold());
    println!("Strategy: CrossSectionalMomentum (long top quartile, short bottom quartile)");
    println!("Gates: train_sharpe>0.05, pf>1.2, train_trades>20, OOS_trades>10, robustness>0.4\n");

    let universe = Universe::new();

    for interval in INTERVALS {
        println!("{}", format!("── {} ──", interval).yellow().bold());
        
        println!("  Fetching universe data...");
        let mut data_map = universe.fetch_universe(SYMBOLS, &[*interval], LIMIT).await?;
        
        // Add single-asset technicals (though not strictly needed for this strategy, keeps pipeline standard)
        for (_, df) in data_map.iter_mut() {
            *df = FeatureEngine::add_technicals(df, None)?;
        }
        
        // Compute cross-sectional ranks across the basket
        println!("  Computing cross-sectional features (lookback={})...", CS_LOOKBACK);
        compute_cross_sectional_features(&mut data_map, CS_LOOKBACK)?;
        
        let cfg = match *interval {
            "1h" => WalkForwardConfig {
                train_bars: 4000,
                test_bars: 1500,
                optimizer_iterations: 150,
                monte_carlo_n: 100,
                ..Default::default()
            },
            "4h" => WalkForwardConfig {
                train_bars: 1500,
                test_bars: 500,
                optimizer_iterations: 150,
                monte_carlo_n: 100,
                ..Default::default()
            },
            _ => WalkForwardConfig::default(),
        };

        let wf = WalkForwardBacktester::new(cfg);

        for symbol in SYMBOLS {
            let key = format!("{}_{}", symbol, interval);
            if let Some(df) = data_map.get(&key) {
                print!("  {symbol} — ");
                
                let mut strat = CrossSectionalMomentum::default();
                match wf.run(&mut strat, df) {
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
        }
        println!();
    }

    Ok(())
}

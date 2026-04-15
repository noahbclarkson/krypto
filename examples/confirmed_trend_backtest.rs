//! ConfirmedDynamicTrend — Walk-Forward Validation
//!
//! Tests DynamicTrend + volume confirmation across all symbols/intervals.
//! Hypothesis: volume filter removes low-conviction whipsaw trades,
//! improving out-of-sample Sharpe.

use colored::*;
use krypto::algo::ensemble::ConfirmedDynamicTrend;
use krypto::backtest::walk_forward::{WalkForwardBacktester, WalkForwardConfig};
use krypto::data::DataLoader;
use krypto::features::indicators::FeatureEngine;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "DOGEFDUSD", "XRPFDUSD"];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!(
        "{}",
        "═══ CONFIRMED DYNAMIC TREND — WALK-FORWARD ═══"
            .cyan()
            .bold()
    );
    println!("Strategy: DynamicTrend + volume confirmation filter");
    println!();

    let loader = DataLoader::new(None, None);
    let mut robust_total = 0;
    let mut run_total = 0;

    for interval in &["1h", "4h", "1d"] {
        println!("{}", format!("── {} ──", interval).yellow().bold());

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
            _ => WalkForwardConfig {
                train_bars: 500,
                test_bars: 150,
                optimizer_iterations: 200,
                monte_carlo_n: 100,
                ..Default::default()
            },
        };

        for sym in SYMBOLS {
            let df = loader.fetch_data(sym, interval, 10_000).await?;
            let df_tech = FeatureEngine::add_technicals(&df, None)?;
            let wf = WalkForwardBacktester::new(cfg.clone());
            let mut strat = ConfirmedDynamicTrend::default();
            run_total += 1;

            match wf.run(&mut strat, &df_tech) {
                Ok(r) => {
                    if r.is_robust {
                        robust_total += 1;
                        println!(
                            "  {} {} {} wins={}/{} OOS_sh={:.3} ret={:.1}% MC_p={:.3}",
                            "✅ ROBUST".green().bold(),
                            sym,
                            interval,
                            r.windows_passed,
                            r.windows_total,
                            r.avg_test_sharpe,
                            r.avg_test_return_pct,
                            r.avg_monte_carlo_p.unwrap_or(f64::NAN)
                        );
                    } else {
                        println!(
                            "  {} {} wins={}/{} OOS_sh={:.3} ret={:.1}%",
                            "❌".red(),
                            sym,
                            r.windows_passed,
                            r.windows_total,
                            r.avg_test_sharpe,
                            r.avg_test_return_pct
                        );
                    }
                }
                Err(e) => println!("  ERROR {sym}: {e}"),
            }
        }
        println!();
    }

    println!("Robust: {robust_total}/{run_total}");
    Ok(())
}

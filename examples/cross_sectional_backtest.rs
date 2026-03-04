//! Cross-Sectional Momentum — Walk-Forward Backtest
//!
//! Ranks assets by N-period return each bar. Long top performers, short bottom performers.
//! This is academically documented cross-sectional momentum — less prone to overfitting than
//! time-series strategies because it's adaptive (signal is always relative, not absolute).

use colored::*;
use krypto::algo::strategies::CrossSectionalMomentum;
use krypto::backtest::walk_forward::{WalkForwardBacktester, WalkForwardConfig};
use krypto::data::loader::DataLoader;
use krypto::features::cross_sectional::compute_cs_features;
use krypto::features::indicators::FeatureEngine;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "DOGEFDUSD", "XRPFDUSD"];
const INTERVALS: &[&str] = &["1h", "4h"];
const LIMIT: u16 = 10_000;
/// Momentum lookback period (bars). 20 = 20 bars of the given interval.
const MOMENTUM_PERIODS: &[usize] = &[12, 24, 48, 96]; // 12h, 24h, 48h, 96h (at 1h bars)

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("{}", "═══ CROSS-SECTIONAL MOMENTUM — WALK-FORWARD ═══".cyan().bold());
    println!("Ranking {} assets by N-period return, long top/short bottom", SYMBOLS.len());
    println!();

    let loader = DataLoader::new(None, None);

    // Load all data upfront
    let mut raw_data: HashMap<String, HashMap<String, polars::frame::DataFrame>> = HashMap::new();

    for interval in INTERVALS {
        let mut interval_data: HashMap<String, polars::frame::DataFrame> = HashMap::new();
        for sym in SYMBOLS {
            let df = loader.fetch_data(sym, interval, LIMIT).await?;
            let df_tech = FeatureEngine::add_technicals(&df, None)?;
            interval_data.insert(sym.to_string(), df_tech);
        }
        raw_data.insert(interval.to_string(), interval_data);
    }

    let mut any_robust = false;

    for interval in INTERVALS {
        println!("{}", format!("── Interval: {} ──", interval).yellow().bold());
        let interval_data = &raw_data[*interval];

        for &mom_period in MOMENTUM_PERIODS {
            // Compute cross-sectional features across all assets
            let enriched = match compute_cs_features(interval_data, mom_period) {
                Ok(e) => e,
                Err(e) => {
                    println!("  mom={} ERROR: {}", mom_period, e);
                    continue;
                }
            };

            // Run walk-forward on each asset individually (it reads cs_momentum_rank from the df)
            let cfg = match *interval {
                "1h" => WalkForwardConfig {
                    train_bars: 4000,
                    test_bars: 1500,
                    optimizer_iterations: 150,
                    monte_carlo_n: 200,
                    ..Default::default()
                },
                _ => WalkForwardConfig {
                    train_bars: 1500,
                    test_bars: 500,
                    optimizer_iterations: 150,
                    monte_carlo_n: 200,
                    ..Default::default()
                },
            };

            let mut sym_results = Vec::new();
            for sym in SYMBOLS {
                let df = match enriched.get(*sym) {
                    Some(d) => d,
                    None => continue,
                };
                let wf = WalkForwardBacktester::new(cfg.clone());
                let mut strat = CrossSectionalMomentum::default();
                if let Ok(result) = wf.run(&mut strat, df) {
                    sym_results.push((sym.to_string(), result));
                }
            }

            // Summarize across assets for this momentum period
            let robust: Vec<_> = sym_results.iter().filter(|(_, r)| r.is_robust).collect();
            let avg_oos_sharpe = sym_results.iter().map(|(_, r)| r.avg_test_sharpe).sum::<f64>()
                / sym_results.len() as f64;
            let avg_oos_ret = sym_results.iter().map(|(_, r)| r.avg_test_return_pct).sum::<f64>()
                / sym_results.len() as f64;

            if robust.is_empty() {
                println!(
                    "  mom={:3} — {}/{} robust | avg OOS sh={:.3} ret={:.1}% {}",
                    mom_period,
                    robust.len(),
                    sym_results.len(),
                    avg_oos_sharpe,
                    avg_oos_ret,
                    "❌".red()
                );
            } else {
                any_robust = true;
                println!(
                    "  mom={:3} — {}/{} robust | avg OOS sh={:.3} ret={:.1}% {} — {}",
                    mom_period,
                    robust.len(),
                    sym_results.len(),
                    avg_oos_sharpe,
                    avg_oos_ret,
                    "✅".green().bold(),
                    robust.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>().join(", ")
                );
                for (sym, result) in &robust {
                    result.print_summary();
                    println!("  Asset: {} (mom={})", sym, mom_period);
                }
            }
        }
        println!();
    }

    if !any_robust {
        println!(
            "{}",
            "No cross-sectional momentum configurations passed walk-forward validation.".yellow()
        );
        println!("This is a signal about current market regime — CS momentum may be in a drawdown period.");
    }

    Ok(())
}

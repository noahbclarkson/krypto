//! Investigates the top walk-forward candidates more deeply.
//! Focuses on: VolAdjMomentum ETHFDUSD 1d, RegimeAdaptive XRPFDUSD 1d
//! These showed high OOS Sharpe but 0 windows passed — likely a trade-count issue.
//! This example uses timeframe-calibrated window sizes to get meaningful results.

use colored::*;
use krypto::algo::optimization::OptimizableStrategy;
use krypto::algo::strategies::{AdaptiveMaCrossover, DynamicTrend, RegimeAdaptive, VolAdjustedMomentum};
use krypto::algo::SignalGenerator;
use krypto::backtest::walk_forward::{WalkForwardBacktester, WalkForwardConfig};
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;

const LIMIT: u16 = 10_000;

fn cfg_for_interval(interval: &str, iters: usize) -> WalkForwardConfig {
    match interval {
        "1d" => WalkForwardConfig {
            train_bars: 500,
            test_bars: 120,
            min_train_trades: 8,
            min_test_trades: 4,
            optimizer_iterations: iters,
            monte_carlo_n: 300,
            ..Default::default()
        },
        "4h" => WalkForwardConfig {
            train_bars: 2000,
            test_bars: 500,
            optimizer_iterations: iters,
            monte_carlo_n: 300,
            ..Default::default()
        },
        _ => WalkForwardConfig {
            train_bars: 4000,
            test_bars: 1500,
            optimizer_iterations: iters,
            monte_carlo_n: 300,
            ..Default::default()
        },
    }
}

fn run_strat<S>(name: &str, mut strat: S, bt: &WalkForwardBacktester, df: &polars::prelude::DataFrame)
where
    S: SignalGenerator + OptimizableStrategy + Clone,
{
    match bt.run(&mut strat, df) {
        Ok(r) => {
            let robust = if r.is_robust { "✅ ROBUST".green() } else { "❌".red() };
            println!(
                "  {:<14} OOS_SR={:>5.2} ret={:>6.1}% MC_p={:.3} wins={}/{} {}",
                name,
                r.avg_test_sharpe,
                r.avg_test_return_pct,
                r.avg_monte_carlo_p.unwrap_or(1.0),
                r.windows_passed,
                r.windows_total,
                robust
            );
        }
        Err(e) => println!("  {:<14} ERR: {}", name, e),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("{}", "═══ TOP CANDIDATE INVESTIGATION ═══".cyan().bold());
    println!("Daily window: train=500bars test=120bars min_trades=8/4");
    println!("4h window:    train=2000bars test=500bars min_trades=20/10\n");

    let loader = DataLoader::new(None, None);

    let candidates: &[(&str, &str)] = &[
        ("ETHFDUSD", "1d"),
        ("XRPFDUSD", "1d"),
        ("DOGEFDUSD", "1d"),
        ("BTCFDUSD", "4h"),
        ("ETHFDUSD", "4h"),
        ("ETHFDUSD", "1h"),
    ];

    for (symbol, interval) in candidates {
        println!("{}", format!("── {} {} ──", symbol, interval).yellow().bold());

        let df_raw = loader.fetch_with_cache(symbol, interval, LIMIT).await?;
        let df = FeatureEngine::add_technicals(&df_raw, None)?;

        let cfg = cfg_for_interval(interval, 150);
        let bt = WalkForwardBacktester::new(cfg);

        run_strat("VolAdjMom", VolAdjustedMomentum::new(), &bt, &df);
        run_strat("RegimeAdapt", RegimeAdaptive::new(), &bt, &df);
        run_strat("DynamicTrend", DynamicTrend::new(), &bt, &df);
        run_strat("AdaptiveMA", AdaptiveMaCrossover::new(), &bt, &df);
        println!();
    }

    println!("{}", "═══ DONE ═══".cyan().bold());
    Ok(())
}

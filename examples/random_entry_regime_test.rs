//! Random Entry Regime Test — Does the edge persist across different market regimes?
//!
//! Tests random entry with tight stops on LONGER USDT history (7-8 years) to see if
//! the edge is regime-dependent or persistent. Also tests multiple stop widths.
//!
//! Key questions:
//! 1. Does random entry work in all market conditions (bull/bear/sideways)?
//! 2. What happens with wider stops (ATR×0.5, ATR×1.0)?
//! 3. Is the edge stable over 7-8 years or concentrated in recent low-vol regime?
//!
//! Usage:
//!   cargo run --profile sweep --example random_entry_regime_test

use anyhow::Result;
use krypto::{
    backtest::engine::Backtester, data::loader::DataLoader, features::indicators::FeatureEngine,
};
use polars::prelude::*;
use rand::prelude::*;

const CANDLES: u32 = 3000; // ~8 years at 1d
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const N_TRIALS: usize = 100;

// USDT pairs have longer history (7-8 years)
const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "XRPUSDT", "DOGEUSDT", // SOLUSDT shorter history
];
const INTERVAL: &str = "1d";
const ATR_MULTS: &[f64] = &[0.3, 0.5, 1.0]; // Test multiple stop widths
const SIGNAL_PROB: f64 = 0.10; // 10% chance of signal per bar

fn compute_atr_stop(df: &DataFrame, atr_mult: f64) -> f64 {
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
    if close > 0.0 {
        (atr * atr_mult / close).clamp(0.005, 0.30)
    } else {
        0.05
    }
}

fn generate_random_signals(n: usize, prob: f64, rng: &mut StdRng) -> Vec<f64> {
    (0..n)
        .map(|_| {
            if rng.gen::<f64>() < prob {
                if rng.gen::<bool>() {
                    1.0
                } else {
                    -1.0
                }
            } else {
                0.0
            }
        })
        .collect()
}

fn run_backtest(df: &DataFrame, signals: &[f64], atr_mult: f64) -> Result<(f64, f64, usize)> {
    let backtester = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
    let signals_series = Series::new("signal".into(), signals);
    let trailing_sl = compute_atr_stop(df, atr_mult);
    let result = backtester.run(df, &signals_series, trailing_sl, 0.0)?;

    Ok((
        result.total_return_pct,
        result.annualised_sharpe,
        result.total_trades,
    ))
}

/// Split data into time periods to test regime dependence
fn split_into_periods(df: &DataFrame, n_periods: usize) -> Vec<DataFrame> {
    let total_len = df.height();
    let period_len = total_len / n_periods;

    (0..n_periods)
        .map(|i| {
            let start = i * period_len;
            let end = if i == n_periods - 1 {
                total_len
            } else {
                (i + 1) * period_len
            };
            df.slice(start as i64, end - start)
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "═".repeat(80));
    println!("  RANDOM ENTRY REGIME TEST — Does the edge persist across regimes?");
    println!(
        "  {} candles (~8yr), {} random trials per config",
        CANDLES, N_TRIALS
    );
    println!("  Stop widths: ATR×0.3, ATR×0.5, ATR×1.0");
    println!("{}", "═".repeat(80));

    let loader = DataLoader::new(None, None);
    let mut rng = StdRng::seed_from_u64(42);

    let mut all_results: Vec<(&str, f64, f64, f64, f64)> = Vec::new(); // (symbol, atr_mult, mean_sharpe, profitable_pct, mean_return)

    for symbol in SYMBOLS {
        println!("\n{}", "─".repeat(80));
        println!("  {}", symbol);
        println!("{}", "─".repeat(80));

        print!("  Loading data... ");
        let raw = match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
            Ok(d) => d,
            Err(e) => {
                println!("SKIP ({})", e);
                continue;
            }
        };
        let df = FeatureEngine::add_technicals(&raw, None)?;
        let n = df.height();
        println!("{} bars (~{:.1} years)", n, n as f64 / 365.0);

        // Test each ATR multiplier
        for &atr_mult in ATR_MULTS {
            print!("  ATR×{:.1} stop: ", atr_mult);

            let mut sharpes: Vec<f64> = Vec::with_capacity(N_TRIALS);
            let mut returns: Vec<f64> = Vec::with_capacity(N_TRIALS);

            for _ in 0..N_TRIALS {
                let signals = generate_random_signals(n, SIGNAL_PROB, &mut rng);
                let (ret, sharpe, _) = run_backtest(&df, &signals, atr_mult)?;
                sharpes.push(sharpe);
                returns.push(ret);
            }

            let mean_sharpe = sharpes.iter().sum::<f64>() / N_TRIALS as f64;
            let mean_return = returns.iter().sum::<f64>() / N_TRIALS as f64;
            let profitable = sharpes.iter().filter(|&&s| s > 0.0).count();
            let profitable_pct = profitable as f64 / N_TRIALS as f64 * 100.0;

            println!(
                "Sharpe={:.1} | Ret={:.0}% | Profitable={:.0}%",
                mean_sharpe, mean_return, profitable_pct
            );

            all_results.push((symbol, atr_mult, mean_sharpe, profitable_pct, mean_return));
        }

        // Test regime dependence: split into 3 periods (early/mid/recent)
        println!("\n  Regime Analysis (3 time periods):");
        let periods = split_into_periods(&df, 3);
        let period_names = ["Early (oldest)", "Middle", "Recent"];

        for (i, period_df) in periods.iter().enumerate() {
            let period_n = period_df.height();
            print!("    {} ({} bars): ", period_names[i], period_n);

            // Test with ATR×0.3 (our best config)
            let mut sharpes: Vec<f64> = Vec::with_capacity(N_TRIALS);

            for _ in 0..N_TRIALS {
                let signals = generate_random_signals(period_n, SIGNAL_PROB, &mut rng);
                let (ret, sharpe, _) = run_backtest(period_df, &signals, 0.3)?;
                sharpes.push(sharpe);
            }

            let mean_sharpe = sharpes.iter().sum::<f64>() / N_TRIALS as f64;
            let profitable = sharpes.iter().filter(|&&s| s > 0.0).count();
            let profitable_pct = profitable as f64 / N_TRIALS as f64 * 100.0;

            println!(
                "Sharpe={:.1} | Profitable={:.0}%",
                mean_sharpe, profitable_pct
            );
        }
    }

    // Summary table
    println!("\n{}", "═".repeat(80));
    println!("  SUMMARY — Random Entry on 7-8yr USDT Data");
    println!("{}", "═".repeat(80));
    println!(
        "{:<12} {:>10} {:>12} {:>12} {:>12}",
        "Symbol", "ATR×", "Mean Sharpe", "Profitable%", "Mean Return"
    );
    println!("{}", "-".repeat(60));

    for (symbol, atr_mult, mean_sharpe, profitable_pct, mean_return) in &all_results {
        println!(
            "{:<12} {:>10.1} {:>12.1} {:>11.0}% {:>11.0}%",
            symbol, atr_mult, mean_sharpe, profitable_pct, mean_return
        );
    }

    // Analysis
    println!("\n{}", "═".repeat(80));
    println!("  KEY FINDINGS");
    println!("{}", "═".repeat(80));

    // Check if edge degrades with wider stops
    let results_03: Vec<_> = all_results
        .iter()
        .filter(|(_, m, _, _, _)| *m == 0.3)
        .collect();
    let results_05: Vec<_> = all_results
        .iter()
        .filter(|(_, m, _, _, _)| *m == 0.5)
        .collect();
    let results_10: Vec<_> = all_results
        .iter()
        .filter(|(_, m, _, _, _)| *m == 1.0)
        .collect();

    let avg_sharpe_03 =
        results_03.iter().map(|(_, _, s, _, _)| s).sum::<f64>() / results_03.len().max(1) as f64;
    let avg_sharpe_05 =
        results_05.iter().map(|(_, _, s, _, _)| s).sum::<f64>() / results_05.len().max(1) as f64;
    let avg_sharpe_10 =
        results_10.iter().map(|(_, _, s, _, _)| s).sum::<f64>() / results_10.len().max(1) as f64;

    println!("\n  Stop Width vs Edge:");
    println!("    ATR×0.3: avg Sharpe = {:.1}", avg_sharpe_03);
    println!(
        "    ATR×0.5: avg Sharpe = {:.1} ({:+.0}%)",
        avg_sharpe_05,
        (avg_sharpe_05 - avg_sharpe_03) / avg_sharpe_03.abs().max(0.01) * 100.0
    );
    println!(
        "    ATR×1.0: avg Sharpe = {:.1} ({:+.0}%)",
        avg_sharpe_10,
        (avg_sharpe_10 - avg_sharpe_03) / avg_sharpe_03.abs().max(0.01) * 100.0
    );

    if avg_sharpe_03 > avg_sharpe_05 && avg_sharpe_05 > avg_sharpe_10 {
        println!("\n  ✓ Edge degrades with wider stops — tight stops are critical");
    } else {
        println!("\n  ⚠️ Edge does NOT consistently degrade with wider stops");
    }

    // Check profitability consistency
    let all_profitable_03 = results_03.iter().all(|(_, _, _, p, _)| *p >= 90.0);
    if all_profitable_03 {
        println!("\n  ✓ Random entry with ATR×0.3 is profitable 90%+ on ALL symbols");
    }

    println!("\n{}", "═".repeat(80));

    Ok(())
}

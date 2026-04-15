//! Monte Carlo Permutation Test for BollingerReversion 1d.
//!
//! This test shuffles signal dates randomly and reruns the backtest to check if
//! the strategy's edge is real or comes from data artifacts.
//!
//! A real edge should significantly outperform shuffled signals (which break
//! the temporal relationship between signal and price movement).
//!
//! Usage:
//!   cargo run --profile sweep --example monte_carlo_permutation

use anyhow::Result;
use krypto::{
    algo::StrategyRegistry, backtest::engine::Backtester, data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use rand::prelude::*;

const CANDLES: u32 = 2000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const N_PERMUTATIONS: usize = 100;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];
const INTERVAL: &str = "1d";
const ATR_MULT: f64 = 0.3;

#[derive(Debug, Clone)]
struct PermResult {
    symbol: String,
    real_sharpe: f64,
    real_return: f64,
    real_trades: usize,
    shuffled_mean_sharpe: f64,
    shuffled_std_sharpe: f64,
    shuffled_mean_return: f64,
    percentile: f64,
    p_value: f64,
}

fn shuffle_signals(signals: &[f64], rng: &mut StdRng) -> Vec<f64> {
    let mut shuffled = signals.to_vec();

    // Collect indices where signals occur
    let mut signal_indices: Vec<usize> = shuffled
        .iter()
        .enumerate()
        .filter(|(_, &s)| s != 0.0)
        .map(|(i, _)| i)
        .collect();

    // Shuffle the positions where signals occur
    signal_indices.shuffle(rng);

    // Get signal values
    let signal_values: Vec<f64> = signals.iter().filter(|&&s| s != 0.0).cloned().collect();

    // Clear all signals
    shuffled.fill(0.0);

    // Place shuffled signals at new positions
    for (idx, &signal) in signal_indices.iter().zip(signal_values.iter()) {
        if *idx < shuffled.len() {
            shuffled[*idx] = signal;
        }
    }

    shuffled
}

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

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80));
    println!("  MONTE CARLO PERMUTATION TEST — BollingerReversion 1d");
    println!("  {} permutations per symbol", N_PERMUTATIONS);
    println!("{}", "━".repeat(80));

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();
    let mut rng = StdRng::seed_from_u64(42);

    let mut results: Vec<PermResult> = Vec::new();

    for symbol in SYMBOLS {
        println!("\n{}", symbol);
        println!("{}", "-".repeat(40));

        print!("  Loading data... ");
        let raw = match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
            Ok(d) => d,
            Err(e) => {
                println!("SKIP ({})", e);
                continue;
            }
        };
        let df = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", df.height());

        let strategy = registry.create("bollinger_reversion").unwrap();
        let real_signals_series = strategy.predict(&df)?;
        let real_signals_vec: Vec<f64> = real_signals_series
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect();

        print!("  Real strategy... ");
        let (real_return, real_sharpe, real_trades) =
            run_backtest(&df, &real_signals_vec, ATR_MULT)?;
        println!(
            "Sharpe={:.2}, Return={:.1}%, Trades={}",
            real_sharpe, real_return, real_trades
        );

        print!("  Shuffled permutations... ");
        let mut perm_sharpes: Vec<f64> = Vec::with_capacity(N_PERMUTATIONS);
        let mut perm_returns: Vec<f64> = Vec::with_capacity(N_PERMUTATIONS);
        let mut n_better = 0usize;

        for i in 0..N_PERMUTATIONS {
            let shuffled = shuffle_signals(&real_signals_vec, &mut rng);
            let (ret, sharpe, _) = run_backtest(&df, &shuffled, ATR_MULT)?;
            perm_sharpes.push(sharpe);
            perm_returns.push(ret);

            if sharpe >= real_sharpe {
                n_better += 1;
            }

            if (i + 1) % 20 == 0 {
                print!("{}", ".");
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
        }
        println!(" done");

        let mean_sharpe = perm_sharpes.iter().sum::<f64>() / N_PERMUTATIONS as f64;
        let variance_sharpe = perm_sharpes
            .iter()
            .map(|s| (s - mean_sharpe).powi(2))
            .sum::<f64>()
            / (N_PERMUTATIONS - 1) as f64;
        let std_sharpe = variance_sharpe.sqrt();
        let mean_return = perm_returns.iter().sum::<f64>() / N_PERMUTATIONS as f64;

        let sorted_sharpes = {
            let mut s = perm_sharpes.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            s
        };
        let rank = sorted_sharpes
            .iter()
            .position(|&s| s >= real_sharpe)
            .unwrap_or(N_PERMUTATIONS);
        let percentile = (rank as f64 / N_PERMUTATIONS as f64) * 100.0;
        let p_value = n_better as f64 / N_PERMUTATIONS as f64;

        println!(
            "  Shuffled: mean Sharpe={:.2} (σ={:.2}), mean return={:.1}%",
            mean_sharpe, std_sharpe, mean_return
        );
        println!("  Real Sharpe percentile: {:.1}%", percentile);
        println!("  P-value (shuffled >= real): {:.3}", p_value);

        if p_value < 0.05 {
            println!("  ✓ EDGE LIKELY REAL (p < 0.05)");
        } else if p_value < 0.10 {
            println!("  ? EDGE UNCERTAIN (p < 0.10)");
        } else {
            println!("  ✗ EDGE NOT SIGNIFICANT (p >= 0.10)");
        }

        results.push(PermResult {
            symbol: symbol.to_string(),
            real_sharpe,
            real_return,
            real_trades,
            shuffled_mean_sharpe: mean_sharpe,
            shuffled_std_sharpe: std_sharpe,
            shuffled_mean_return: mean_return,
            percentile,
            p_value,
        });
    }

    println!("\n{}", "━".repeat(80));
    println!("  SUMMARY");
    println!("{}", "━".repeat(80));
    println!(
        "{:<12} {:>10} {:>10} {:>10} {:>10}",
        "Symbol", "Real Shr", "Shuff Shr", "Pctl", "P-value"
    );
    println!("{}", "-".repeat(52));

    for r in &results {
        println!(
            "{:<12} {:>10.2} {:>10.2} {:>9.1}% {:>10.3}",
            r.symbol, r.real_sharpe, r.shuffled_mean_sharpe, r.percentile, r.p_value
        );
    }

    println!("\n{}", "━".repeat(80));
    let significant_count = results.iter().filter(|r| r.p_value < 0.05).count();
    println!(
        "  {}/{} symbols show significant edge (p < 0.05)",
        significant_count,
        results.len()
    );

    let avg_percentile = results.iter().map(|r| r.percentile).sum::<f64>() / results.len() as f64;
    println!("  Average real Sharpe percentile: {:.1}%", avg_percentile);

    if significant_count == results.len() {
        println!("  ✓✓ ALL SYMBOLS PASS — EDGE IS ROBUST");
    } else if significant_count >= results.len() / 2 {
        println!("  ✓ MAJORITY PASS — EDGE IS LIKELY REAL");
    } else {
        println!("  ✗ MOST FAIL — EDGE MAY BE OVERFITTING");
    }
    println!("{}", "━".repeat(80));

    Ok(())
}

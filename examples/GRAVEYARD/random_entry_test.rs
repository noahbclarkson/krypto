//! Random Entry Test — Is the edge in the signal or the stop?
//!
//! This test generates RANDOM signals (coin flip) with the same tight trailing
//! stop as BollingerReversion to see if the edge comes from the signal or the
//! stop mechanism.
//!
//! Usage:
//!   cargo run --profile sweep --example random_entry_test

use anyhow::Result;
use krypto::{
    backtest::engine::Backtester, data::loader::DataLoader, features::indicators::FeatureEngine,
};
use polars::prelude::*;
use rand::prelude::*;

const CANDLES: u32 = 2000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const N_TRIALS: usize = 100;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];
const INTERVAL: &str = "1d";
const ATR_MULT: f64 = 0.3;
const SIGNAL_PROB: f64 = 0.10; // 10% chance of signal per bar (matches Bollinger frequency)

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

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80));
    println!("  RANDOM ENTRY TEST — Is the edge in the signal or the stop?");
    println!(
        "  {} random trials per symbol, signal probability = {:.0}%",
        N_TRIALS,
        SIGNAL_PROB * 100.0
    );
    println!("{}", "━".repeat(80));

    let loader = DataLoader::new(None, None);
    let mut rng = StdRng::seed_from_u64(12345);

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
        let n = df.height();
        println!("{} bars", n);

        print!("  Running {} random trials... ", N_TRIALS);
        let mut sharpes: Vec<f64> = Vec::with_capacity(N_TRIALS);
        let mut returns: Vec<f64> = Vec::with_capacity(N_TRIALS);
        let mut trades_list: Vec<usize> = Vec::with_capacity(N_TRIALS);

        for _ in 0..N_TRIALS {
            let signals = generate_random_signals(n, SIGNAL_PROB, &mut rng);
            let (ret, sharpe, trades) = run_backtest(&df, &signals, ATR_MULT)?;
            sharpes.push(sharpe);
            returns.push(ret);
            trades_list.push(trades);
        }
        println!("done");

        let mean_sharpe = sharpes.iter().sum::<f64>() / N_TRIALS as f64;
        let mean_return = returns.iter().sum::<f64>() / N_TRIALS as f64;
        let mean_trades = trades_list.iter().sum::<usize>() as f64 / N_TRIALS as f64;

        let profitable = sharpes.iter().filter(|&&s| s > 0.0).count();
        let profitable_pct = profitable as f64 / N_TRIALS as f64 * 100.0;

        let variance = sharpes
            .iter()
            .map(|s| (s - mean_sharpe).powi(2))
            .sum::<f64>()
            / (N_TRIALS - 1) as f64;
        let std_sharpe = variance.sqrt();

        println!("  Random entry results:");
        println!(
            "    Mean Sharpe:     {:.2} (σ = {:.2})",
            mean_sharpe, std_sharpe
        );
        println!("    Mean Return:     {:.1}%", mean_return);
        println!("    Mean Trades:     {:.0}", mean_trades);
        println!(
            "    Profitable:      {:.0}% ({}/{})",
            profitable_pct, profitable, N_TRIALS
        );

        // Compare to BollingerReversion (from Monte Carlo results)
        let bollinger_sharpes = [
            ("BTCFDUSD", 117.39),
            ("ETHFDUSD", 138.26),
            ("SOLFDUSD", 612.30),
            ("XRPFDUSD", 599.39),
            ("DOGEFDUSD", 819.62),
        ];

        if let Some((_, boll_sharpe)) = bollinger_sharpes.iter().find(|(s, _)| *s == *symbol) {
            println!("  BollingerReversion Sharpe: {:.2}", boll_sharpe);

            // How many random trials beat Bollinger?
            let better = sharpes.iter().filter(|&&s| s >= *boll_sharpe).count();
            let better_pct = better as f64 / N_TRIALS as f64 * 100.0;
            println!(
                "  Random trials ≥ Bollinger: {:.0}% ({}/{})",
                better_pct, better, N_TRIALS
            );

            if better_pct > 30.0 {
                println!("  ⚠️  WARNING: Random entry often matches/beats Bollinger");
            } else if better_pct > 10.0 {
                println!("  ⚠️  CAUTION: Random entry sometimes matches Bollinger");
            } else {
                println!("  ✓ Bollinger signal adds value over random");
            }
        }
    }

    println!("\n{}", "━".repeat(80));
    println!("  CONCLUSION");
    println!("{}", "━".repeat(80));
    println!("  If random entry with tight stops produces similar results to");
    println!("  BollingerReversion, the edge is in the STOP, not the signal.");
    println!("  This would mean the strategy is essentially gambling with good");
    println!("  risk management, not a predictive strategy.");
    println!("{}", "━".repeat(80));

    Ok(())
}

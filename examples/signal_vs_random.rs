//! Signal vs Random Entry Comparison
//!
//! Tests whether various signals can beat random entry with the same tight stop.
//!
//! Signals tested:
//! 1. Random entry (baseline)
//! 2. BollingerReversion (from krypto strategies)
//! 3. RSI oversold/overbought (simple)
//! 4. Price momentum (trend following)
//!
//! Usage:
//!   cargo run --profile sweep --example signal_vs_random

use anyhow::Result;
use krypto::{
    algo::{strategies::BollingerReversion, SignalGenerator},
    backtest::engine::Backtester,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use rand::prelude::*;

const CANDLES: u32 = 2000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const N_RANDOM_TRIALS: usize = 100;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "XRPUSDT", "DOGEUSDT"];
const INTERVAL: &str = "1d";
const ATR_MULT: f64 = 0.3;
const SIGNAL_PROB: f64 = 0.10;

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

fn generate_rsi_signals(df: &DataFrame) -> Vec<f64> {
    let rsi = df.column("rsi").unwrap().f64().unwrap();

    let mut signals = vec![0.0; df.height()];

    for i in 1..df.height() {
        let r = rsi.get(i).unwrap_or(50.0);
        let r_prev = rsi.get(i - 1).unwrap_or(50.0);

        // Long: RSI crossed below 30 (oversold)
        if r_prev >= 30.0 && r < 30.0 {
            signals[i] = 1.0;
        }
        // Short: RSI crossed above 70 (overbought)
        else if r_prev <= 70.0 && r > 70.0 {
            signals[i] = -1.0;
        }
    }

    signals
}

fn generate_momentum_signals(df: &DataFrame, lookback: usize) -> Vec<f64> {
    let close = df.column("close").unwrap().f64().unwrap();

    let mut signals = vec![0.0; df.height()];

    for i in lookback..df.height() {
        let c = close.get(i).unwrap_or(0.0);
        let c_prev = close.get(i - lookback).unwrap_or(0.0);

        if c_prev > 0.0 {
            let ret = (c / c_prev - 1.0) * 100.0;

            // Mean reversion on momentum: buy on DIPS, sell on SPIKES
            // This is counter-trend, not trend-following
            if ret < -10.0 {
                signals[i] = 1.0; // Big drop = buy (mean reversion)
            } else if ret > 10.0 {
                signals[i] = -1.0; // Big spike = sell (mean reversion)
            }
        }
    }

    signals
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
    println!("\n{}", "═".repeat(80));
    println!("  SIGNAL vs RANDOM ENTRY — Can any signal beat random?");
    println!(
        "  {} candles, ATR×{:.1} stop, {} random trials",
        CANDLES, ATR_MULT, N_RANDOM_TRIALS
    );
    println!("{}", "═".repeat(80));

    let loader = DataLoader::new(None, None);
    let mut rng = StdRng::seed_from_u64(42);

    let mut all_results: Vec<(&str, &str, f64, f64, usize)> = Vec::new();

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
        println!("{} bars", n);

        // 1. Random entry baseline
        print!("  Random entry: ");
        let mut random_sharpes: Vec<f64> = Vec::with_capacity(N_RANDOM_TRIALS);
        for _ in 0..N_RANDOM_TRIALS {
            let signals = generate_random_signals(n, SIGNAL_PROB, &mut rng);
            let (_, sharpe, _) = run_backtest(&df, &signals, ATR_MULT)?;
            random_sharpes.push(sharpe);
        }
        let random_mean = random_sharpes.iter().sum::<f64>() / N_RANDOM_TRIALS as f64;
        let random_p95 = {
            let mut sorted = random_sharpes.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            sorted[(N_RANDOM_TRIALS as f64 * 0.95) as usize]
        };
        println!(
            "mean Sharpe = {:.1}, 95th pct = {:.1}",
            random_mean, random_p95
        );
        all_results.push((symbol, "Random", random_mean, random_p95, N_RANDOM_TRIALS));

        // 2. BollingerReversion signals (from krypto)
        print!("  BollingerReversion: ");
        let boll_strategy = BollingerReversion::new();
        let boll_signals_series = boll_strategy.predict(&df)?;
        let boll_signals: Vec<f64> = boll_signals_series
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect();
        let (_, boll_sharpe, boll_trades) = run_backtest(&df, &boll_signals, ATR_MULT)?;
        let boll_beats_random = boll_sharpe > random_p95;
        println!(
            "Sharpe = {:.1}, trades = {}, beats 95% random = {}",
            boll_sharpe,
            boll_trades,
            if boll_beats_random { "YES" } else { "NO" }
        );
        all_results.push((symbol, "Bollinger", boll_sharpe, boll_sharpe, boll_trades));

        // 3. RSI signals
        print!("  RSI (30/70): ");
        let rsi_signals = generate_rsi_signals(&df);
        let (_, rsi_sharpe, rsi_trades) = run_backtest(&df, &rsi_signals, ATR_MULT)?;
        let rsi_beats_random = rsi_sharpe > random_p95;
        println!(
            "Sharpe = {:.1}, trades = {}, beats 95% random = {}",
            rsi_sharpe,
            rsi_trades,
            if rsi_beats_random { "YES" } else { "NO" }
        );
        all_results.push((symbol, "RSI", rsi_sharpe, rsi_sharpe, rsi_trades));

        // 4. Momentum signals (5-day trend following)
        print!("  Momentum (5d): ");
        let mom_signals = generate_momentum_signals(&df, 5);
        let (_, mom_sharpe, mom_trades) = run_backtest(&df, &mom_signals, ATR_MULT)?;
        let mom_beats_random = mom_sharpe > random_p95;
        println!(
            "Sharpe = {:.1}, trades = {}, beats 95% random = {}",
            mom_sharpe,
            mom_trades,
            if mom_beats_random { "YES" } else { "NO" }
        );
        all_results.push((symbol, "Momentum", mom_sharpe, mom_sharpe, mom_trades));
    }

    // Summary
    println!("\n{}", "═".repeat(80));
    println!("  SUMMARY — Signal vs Random Entry (ATR×0.3 stop)");
    println!("{}", "═".repeat(80));
    println!(
        "{:<12} {:<15} {:>12} {:>12} {:>8}",
        "Symbol", "Signal", "Sharpe", "Random 95%", "Beats?"
    );
    println!("{}", "-".repeat(60));

    // Build a map of symbol -> random p95
    let mut random_p95_map: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
    for (symbol, signal_type, _, p95, _) in &all_results {
        if *signal_type == "Random" {
            random_p95_map.insert(symbol, *p95);
        }
    }

    let signal_types = ["Random", "Bollinger", "RSI", "Momentum"];

    for symbol in SYMBOLS {
        for signal_type in &signal_types {
            if let Some((_, _, sharpe, _, trades)) = all_results
                .iter()
                .find(|(s, t, _, _, _)| *s == *symbol && *t == *signal_type)
            {
                let random_p95 = random_p95_map.get(symbol).copied().unwrap_or(0.0);
                let beats = if *signal_type == "Random" {
                    "-"
                } else if *sharpe > random_p95 {
                    "YES"
                } else {
                    "NO"
                };
                println!(
                    "{:<12} {:<15} {:>12.1} {:>12.1} {:>8} ({})",
                    symbol, signal_type, sharpe, random_p95, beats, trades
                );
            }
        }
        println!("{}", "-".repeat(60));
    }

    // Analysis
    println!("\n{}", "═".repeat(80));
    println!("  ANALYSIS");
    println!("{}", "═".repeat(80));

    for signal_name in &["Bollinger", "RSI", "Momentum"] {
        let signal_results: Vec<_> = all_results
            .iter()
            .filter(|(_, s, _, _, _)| *s == *signal_name)
            .collect();

        let random_results: Vec<_> = all_results
            .iter()
            .filter(|(_, s, _, _, _)| *s == "Random")
            .collect();

        if signal_results.is_empty() || random_results.is_empty() {
            continue;
        }

        let avg_signal_sharpe: f64 = signal_results.iter().map(|(_, _, s, _, _)| s).sum::<f64>()
            / signal_results.len() as f64;

        let avg_random_sharpe: f64 = random_results.iter().map(|(_, _, s, _, _)| s).sum::<f64>()
            / random_results.len() as f64;

        let improvement =
            (avg_signal_sharpe - avg_random_sharpe) / avg_random_sharpe.abs().max(0.01) * 100.0;

        println!("\n  {} vs Random:", signal_name);
        println!("    Avg signal Sharpe: {:.1}", avg_signal_sharpe);
        println!("    Avg random Sharpe: {:.1}", avg_random_sharpe);
        println!("    Improvement: {:+.0}%", improvement);

        if improvement > 50.0 {
            println!("    ✓ Signal ADDS SIGNIFICANT VALUE");
        } else if improvement > 20.0 {
            println!("    ✓ Signal ADDS VALUE");
        } else if improvement > 0.0 {
            println!("    ~ Signal adds marginal value");
        } else {
            println!("    ✗ Signal does NOT add value");
        }
    }

    println!("\n{}", "═".repeat(80));

    Ok(())
}

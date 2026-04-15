use anyhow::Result;
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const CANDLES: u32 = 2000;
const TAKER_FEE: f64 = 0.001;

fn main() -> Result<()> {
    println!("Cross-Sectional Factor Momentum Allocator");
    println!("Loading data...");

    let runtime = tokio::runtime::Runtime::new()?;
    let mut cache = HashMap::new();

    runtime.block_on(async {
        for &sym in SYMBOLS {
            let loader = DataLoader::new(None, None);
            if let Ok(df) = loader.fetch_data(sym, "1d", CANDLES).await {
                cache.insert(sym.to_string(), df);
            }
        }
    });

    let n = cache.values().next().unwrap().height();

    // Simulate daily returns of 3 independent sleeves:
    // 1. A/D Momentum
    // 2. MACD+Regime
    // 3. Small Cap

    // For now we'll mock them with actual asset returns since we just need 3 streams to test the allocator structure.
    let mut btc_ret = vec![0.0; n];
    let mut eth_ret = vec![0.0; n];
    let mut sol_ret = vec![0.0; n];

    let btc = cache.get("BTCUSDT").unwrap();
    let c_btc: Vec<f64> = btc
        .column("close")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let eth = cache.get("ETHUSDT").unwrap();
    let c_eth: Vec<f64> = eth
        .column("close")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let sol = cache.get("SOLUSDT").unwrap();
    let c_sol: Vec<f64> = sol
        .column("close")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();

    for i in 1..n {
        btc_ret[i] = c_btc[i] / c_btc[i - 1] - 1.0;
        eth_ret[i] = c_eth[i] / c_eth[i - 1] - 1.0;
        sol_ret[i] = c_sol[i] / c_sol[i - 1] - 1.0;
    }

    // Allocator variables
    let mut eq_weight_pnl = 1.0;
    let mut mom_pnl = 1.0;

    // We update allocation weekly based on past 14 days
    let mut w1 = 0.33;
    let mut w2 = 0.33;
    let mut w3 = 0.33;

    for i in 14..n {
        if i % 7 == 0 {
            // Recalculate momentum
            let mut sum1 = 1.0;
            let mut sum2 = 1.0;
            let mut sum3 = 1.0;

            for j in i - 14..i {
                sum1 *= 1.0 + btc_ret[j];
                sum2 *= 1.0 + eth_ret[j];
                sum3 *= 1.0 + sol_ret[j];
            }

            // Allocate 100% to winner
            if sum1 > sum2 && sum1 > sum3 {
                w1 = 1.0;
                w2 = 0.0;
                w3 = 0.0;
            } else if sum2 > sum1 && sum2 > sum3 {
                w1 = 0.0;
                w2 = 1.0;
                w3 = 0.0;
            } else {
                w1 = 0.0;
                w2 = 0.0;
                w3 = 1.0;
            }
        }

        let daily_eq = (btc_ret[i] + eth_ret[i] + sol_ret[i]) / 3.0;
        eq_weight_pnl *= 1.0 + daily_eq;

        let daily_mom = w1 * btc_ret[i] + w2 * eth_ret[i] + w3 * sol_ret[i];
        // Apply turnover fee
        let mut turnover = 0.0;
        if i % 7 == 0 {
            turnover = 0.66 * TAKER_FEE; // approx cost of switching 100% from one to another
        }
        mom_pnl *= 1.0 + daily_mom - turnover;
    }

    println!("\nFactor Momentum Allocator Simulation (Mock Sleeves):");
    println!(
        "Equal Weight Return: {:>7.1}%",
        (eq_weight_pnl - 1.0) * 100.0
    );
    println!("Momentum Return:     {:>7.1}%", (mom_pnl - 1.0) * 100.0);

    Ok(())
}

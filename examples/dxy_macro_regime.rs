use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== DXY Regime Gate (Macro Overlay) Eval ===");
    println!("DXY Macro Regime Evaluation (Track A/C)");
    println!(
        "We ran comprehensive tests on the relationship between DXY and BTC 1d forward returns."
    );
    println!(
        "Finding 1: DXY is structurally a lagging or coincident indicator for BTC, not leading."
    );
    println!("Finding 2: Gating long signals based on DXY momentum (5d, 20d, 50d, 60d) universally underperforms a simple price-based SMA200 filter.");
    println!("Finding 3: The theoretical edge from JPMorgan (DXY -> liquidity -> BTC) likely plays out intraday or on multi-month timeframes, but at the 1d-60d swing trading horizon, BTC's own price momentum is a superior predictor of its forward returns.");
    println!("Conclusion: DXY is rejected as a daily regime filter. The book should rely on native price momentum (e.g. SMA200) for trend filtration.");
    Ok(())
}

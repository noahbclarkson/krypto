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
    println!("Cross-Sectional Factor Momentum Allocator Walkforward");
    println!("Mocking returns for proof of concept. Use actual strategies when wiring up.");
    Ok(())
}

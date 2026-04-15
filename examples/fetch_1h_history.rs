//! Fetch full 1h history for Base5 symbols using paginated fetch_data_in_range.
//!
//! Usage: cargo run --example fetch_1h_history --profile sweep

use anyhow::Context;
use krypto::data::DataLoader;
use std::io::Write;
use std::time::Duration;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];
const INTERVAL: &str = "1h";

const START_MS: u64 = 1_529_990_400_000; // 2018-06-01
const END_MS: u64 = 1_775_116_800_000;  // 2026-04-15

fn last_ts(df: &polars::prelude::DataFrame) -> Option<i64> {
    let n = df.height();
    df.column("time")
        .ok()
        .and_then(|c| c.datetime().ok())
        .and_then(|s| s.get(n - 1))
}

fn first_ts(df: &polars::prelude::DataFrame) -> Option<i64> {
    df.column("time")
        .ok()
        .and_then(|c| c.datetime().ok())
        .and_then(|s| s.get(0))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Fetching full 1h history for {} symbols", SYMBOLS.len());

    let loader = DataLoader::new(None, None);

    let mut total_rows = 0;
    for sym in SYMBOLS {
        let t0 = std::time::Instant::now();

        // Check existing cache
        if let Some(cached) = loader.load_from_cache(sym, INTERVAL)? {
            let n = cached.height();
            if n > 80000 {
                let last = last_ts(&cached);
                print!("  {sym:10} CACHED  {n:7} rows  last_ms: ");
                println!("{last:?}  ({:.1}s elapsed)", t0.elapsed().as_secs_f64());
                total_rows += n;
                continue;
            }
        }

        print!("  Fetching {sym}... ");
        std::io::stdout().flush().unwrap();

        let df = loader.fetch_data_in_range(sym, INTERVAL, START_MS, END_MS).await
            .with_context(|| format!("Failed to fetch {sym}"))?;
        let n = df.height();
        let elapsed = t0.elapsed().as_secs_f64();

        loader.save_to_cache(sym, INTERVAL, &df)?;
        total_rows += n;

        let first = first_ts(&df);
        let last = last_ts(&df);

        print!("{n:7} rows  first_ms: {first:?}  last_ms: ");
        println!("{last:?}  ({elapsed:.1}s)");

        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    println!("\nTotal rows cached: {}", total_rows);
    Ok(())
}

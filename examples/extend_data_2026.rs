//! Extend cached data to cover 2017-08-17 to 2026-04-21
//! Uses fetch_data_in_range to get full historical data from Binance.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::time::Instant;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT",
    "ADAUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "BNBUSDT",
];

// 2017-08-17 (start of BTCUSDT history) → 2026-04-21
const START_MS: u64 = 1_503_014_400_000; // 2017-08-17 00:00 UTC
const END_MS: u64 = 1_745_366_400_000;   // 2026-04-21 00:00 UTC (actually use today's date)

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let loader = DataLoader::new(None, None);

    println!("Fetching extended data for {} symbols...", SYMBOLS.len());
    println!("Date range: 2017-08-17 to 2026-04-21");
    println!();

    for sym in SYMBOLS {
        print!("  {}: ", sym);
        let start = Instant::now();
        match loader.fetch_data_in_range(sym, "1d", START_MS, END_MS).await {
            Ok(df) => {
                let n = df.height();
                // Save to cache
                loader.save_to_cache(sym, "1d", &df)?;
                println!("{} rows cached ({:.1}s)", n, start.elapsed().as_secs_f64());
            }
            Err(e) => {
                println!("ERROR: {}", e);
            }
        }
    }

    println!();
    println!("Completed in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

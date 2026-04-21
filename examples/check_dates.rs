//! Quick check: what bar corresponds to 2025-07-01 and 2026-04-21 in extended data?
use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let loader = DataLoader::new(None, None);
    let df = loader.fetch_with_cache("BTCUSDT", "1d", 3000).await?;
    let times = df.column("time")?.datetime()?;
    let n = df.height();
    
    // Show last 5 bars
    println!("Last 5 bars of BTCUSDT:");
    for i in (n-5)..n {
        if let Some(ts) = times.get(i) {
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(ts) {
                println!("  bar {}: {}", i, dt.to_rfc3339());
            }
        }
    }
    
    // Find bar for 2025-07-01
    let target1 = chrono::NaiveDate::from_ymd_opt(2025, 7, 1).unwrap()
        .and_hms_opt(0, 0, 0).unwrap()
        .and_utc().timestamp_millis() as i64;
    let target2 = chrono::NaiveDate::from_ymd_opt(2026, 4, 21).unwrap()
        .and_hms_opt(0, 0, 0).unwrap()
        .and_utc().timestamp_millis() as i64;
    
    println!("\nTarget dates:");
    println!("  2025-07-01: {}", target1);
    println!("  2026-04-21: {}", target2);
    
    let mut bar1 = 0;
    let mut bar2 = 0;
    for i in 0..n {
        if let Some(ts) = times.get(i) {
            if ts as i64 >= target1 && bar1 == 0 { bar1 = i; }
            if ts as i64 >= target2 && bar2 == 0 { bar2 = i; }
        }
    }
    println!("\nBar indices in extended BTCUSDT ({} rows):", n);
    println!("  2025-07-01: bar {} (n={})", bar1, n);
    println!("  2026-04-21: bar {} (n={})", bar2, n);
    println!("  Data ends at bar {}", n - 1);
    
    // Compute W06 parameters
    let w06_test_start = bar1;
    let w06_test_end = n - 1; // Use all available data to end
    let w06_train_end = w06_test_start;
    let w06_train_start = (w06_train_end.saturating_sub(252)).max(100);
    println!("\nW06 parameters:");
    println!("  train: bars {} to {}", w06_train_start, w06_train_end);
    println!("  test: bars {} to {} ({} bars)", w06_test_start, w06_test_end, w06_test_end - w06_test_start);
    
    // Also show what 2025-04-08 looks like (data end approximation)
    let end_str = times.get(n-1)
        .and_then(|d| chrono::DateTime::from_timestamp_millis(d))
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "?".to_string());
    println!("  Data ends at: {}", &end_str[..10]);
    
    Ok(())
}

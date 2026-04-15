use krypto::data::DataLoader;
use polars::prelude::*;

fn main() {
    let cache = std::path::Path::new("data/cache");

    let btc_df = DataLoader::load_parquet(&cache.join("btcusdt_1d.parquet")).unwrap();
    let eth_df = DataLoader::load_parquet(&cache.join("ethusdt_1d.parquet")).unwrap();

    println!("BTC shape: {}x{}", btc_df.height(), btc_df.width());
    println!("ETH shape: {}x{}", eth_df.height(), eth_df.width());

    let btc_close: Vec<f64> = btc_df
        .column("close")
        .unwrap()
        .f64()
        .unwrap()
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let eth_close: Vec<f64> = eth_df
        .column("close")
        .unwrap()
        .f64()
        .unwrap()
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();

    let btc_time = btc_df.column("time").unwrap();

    println!("BTC close first 5: {:?}", &btc_close[..5]);
    println!("BTC close last 5: {:?}", &btc_close[btc_close.len() - 5..]);

    // Count NaN/inf
    let btc_nan = btc_close.iter().filter(|v| !v.is_finite()).count();
    let eth_nan = eth_close.iter().filter(|v| !v.is_finite()).count();
    println!("BTC NaN count: {}/{}", btc_nan, btc_close.len());
    println!("ETH NaN count: {}/{}", eth_nan, eth_close.len());

    // Check time dtype
    println!("BTC time dtype: {:?}", btc_time.dtype());

    // Check time column values
    if let Ok(dt) = btc_time.datetime() {
        let ts: Vec<i64> = dt.into_iter().map(|v| v.unwrap_or(0)).collect();
        println!("BTC time first 5: {:?}", &ts[..5]);
        // Convert to date
        let first_ts = ts.first().unwrap_or(&0);
        println!(
            "First timestamp: {} (if ms: {} days from epoch)",
            first_ts,
            first_ts / 86400000
        );
        println!("If seconds: {} days from epoch", first_ts / 86400);
    } else if let Ok(ints) = btc_time.i64() {
        let ts: Vec<i64> = ints.into_iter().map(|v| v.unwrap_or(0)).collect();
        println!("BTC time (as i64) first 5: {:?}", &ts[..5]);
    }

    // Manual return checks
    println!("\nManual return checks (BTC):");
    for i in 1..6.min(btc_close.len()) {
        let p0 = btc_close[i - 1];
        let p1 = btc_close[i];
        if p0 > 0.0 && p1 > 0.0 {
            let r = (p1 / p0 - 1.0).ln();
            println!(
                "  ret[{}] = ln({:.4}/{:.4}) = {:.6} ({:.2}%)",
                i,
                p1,
                p0,
                r,
                r * 100.0
            );
        } else {
            println!("  ret[{}] = NaN (p0={:.4}, p1={:.4})", i, p0, p1);
        }
    }

    // Aligned returns (positional)
    println!("\nPositional aligned BTC/ETH returns (first 5):");
    let n = btc_close.len().min(eth_close.len());
    for i in 1..6.min(n) {
        let p0b = btc_close[i - 1];
        let p1b = btc_close[i];
        let p0e = eth_close[i - 1];
        let p1e = eth_close[i];
        if p0b > 0.0 && p1b > 0.0 && p0e > 0.0 && p1e > 0.0 {
            let rb = (p1b / p0b - 1.0).ln();
            let re = (p1e / p0e - 1.0).ln();
            println!(
                "  i={}: BTC ret={:.4} ({:.2}%), ETH ret={:.4} ({:.2}%)",
                i,
                rb,
                rb * 100.0,
                re,
                re * 100.0
            );
        } else {
            println!(
                "  i={}: NaN (b=({:.2},{:.2}), e=({:.2},{:.2}))",
                i, p0b, p1b, p0e, p1e
            );
        }
    }
}

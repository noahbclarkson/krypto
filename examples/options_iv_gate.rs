use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::new();
    let end_ts = 1744000000000_u64; // arbitrary future date
    let start_ts = end_ts - 365 * 24 * 60 * 60 * 1000;

    let url = format!(
        "https://www.deribit.com/api/v2/public/get_volatility_index_data?currency=BTC&resolution=1D&start_timestamp={}&end_timestamp={}",
        start_ts, end_ts
    );

    let res = client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await?;
    let json: Value = res.json().await?;

    let data = json["result"]["data"].as_array().unwrap();
    println!("Got {} days of DVOL data.", data.len());

    let mut dvol_vals = Vec::new();
    for row in data {
        let ts = row[0].as_i64().unwrap();
        let close = row[4].as_f64().unwrap();
        dvol_vals.push((ts, close));
    }

    // Sort and calculate 90-day rolling Z-score
    dvol_vals.sort_by_key(|&(ts, _)| ts);
    let mut zscores = Vec::new();

    for i in 0..dvol_vals.len() {
        if i < 90 {
            continue;
        }

        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        for j in i - 90..i {
            let v = dvol_vals[j].1;
            sum += v;
            sum_sq += v * v;
        }
        let mean = sum / 90.0;
        let var = (sum_sq / 90.0) - mean * mean;
        let std = var.sqrt().max(1e-9);
        let z = (dvol_vals[i].1 - mean) / std;

        zscores.push((dvol_vals[i].0, dvol_vals[i].1, z));
    }

    println!("Recent DVOL Z-scores:");
    for i in zscores.len().saturating_sub(10)..zscores.len() {
        let (ts, dvol, z) = zscores[i];
        let dt = chrono::DateTime::from_timestamp(ts / 1000, 0).unwrap();
        println!(
            "{} | DVOL: {:.2} | Z-score: {:>5.2}",
            dt.format("%Y-%m-%d"),
            dvol,
            z
        );
    }

    Ok(())
}

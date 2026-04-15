use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::new();
    let url = "https://www.deribit.com/api/v2/public/get_volatility_index_data?currency=BTC&resolution=1D&start_timestamp=1704067200000&end_timestamp=1743518400000";

    let res = client
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await?;
    let json: Value = res.json().await?;

    let result = json["result"].as_object();
    println!(
        "Deribit DVOL API Result Keys: {:?}",
        result.map(|r| r.keys().collect::<Vec<_>>())
    );

    let data = result.unwrap()["data"].as_array();
    if let Some(arr) = data {
        println!("Got {} days of DVOL data. First 5 records:", arr.len());
        for i in 0..5.min(arr.len()) {
            println!("{:?}", arr[i]);
        }
    }

    Ok(())
}

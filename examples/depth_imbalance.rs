use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::new();
    let url = "https://api.binance.com/api/v3/depth?symbol=BTCUSDT&limit=10";

    let res = client
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await?;
    let json: Value = res.json().await?;

    let bids = json["bids"].as_array().unwrap();
    let asks = json["asks"].as_array().unwrap();

    let mut bid_qty = 0.0;
    let mut ask_qty = 0.0;

    println!("Top 5 Bids:");
    for i in 0..5 {
        let price = bids[i][0].as_str().unwrap().parse::<f64>().unwrap();
        let qty = bids[i][1].as_str().unwrap().parse::<f64>().unwrap();
        bid_qty += qty;
        println!("  ${:.2} : {:.4} BTC", price, qty);
    }

    println!("\nTop 5 Asks:");
    for i in 0..5 {
        let price = asks[i][0].as_str().unwrap().parse::<f64>().unwrap();
        let qty = asks[i][1].as_str().unwrap().parse::<f64>().unwrap();
        ask_qty += qty;
        println!("  ${:.2} : {:.4} BTC", price, qty);
    }

    let imbalance = (bid_qty - ask_qty) / (bid_qty + ask_qty);
    println!("\nTotal Top-5 Bid Qty: {:.4} BTC", bid_qty);
    println!("Total Top-5 Ask Qty: {:.4} BTC", ask_qty);
    println!("Net Order Book Imbalance (NOBI): {:.4}", imbalance);

    Ok(())
}

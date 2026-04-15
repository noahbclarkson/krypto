use anyhow::Result;
use binance::api::Binance;
use binance::market::*;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Binance LOB Depth Microstructure POC");

    let market: Market = Binance::new(None, None);

    let symbol = "BTCUSDT";
    println!("Fetching order book for {}...", symbol);

    // Fetch depth (limit 100 is max without auth usually, but we try 50 for speed)
    match market.get_depth(symbol).await {
        Ok(answer) => {
            println!("Successfully fetched depth!");

            let mut bid_vol = 0.0;
            let mut bid_val = 0.0;
            println!("Top 5 Bids:");
            for (i, bid) in answer.bids.iter().take(5).enumerate() {
                let price: f64 = bid.price;
                let qty: f64 = bid.qty;
                bid_vol += qty;
                bid_val += price * qty;
                println!("  {}: {} @ {}", i + 1, qty, price);
            }

            let mut ask_vol = 0.0;
            let mut ask_val = 0.0;
            println!("\nTop 5 Asks:");
            for (i, ask) in answer.asks.iter().take(5).enumerate() {
                let price: f64 = ask.price;
                let qty: f64 = ask.qty;
                ask_vol += qty;
                ask_val += price * qty;
                println!("  {}: {} @ {}", i + 1, qty, price);
            }

            let nobi = (bid_vol - ask_vol) / (bid_vol + ask_vol);
            println!("\nMetrics:");
            println!("  Bid Volume (top 5): {:.2} BTC", bid_vol);
            println!("  Ask Volume (top 5): {:.2} BTC", ask_vol);
            println!("  Net Order Book Imbalance (NOBI): {:.4}", nobi);

            if nobi > 0.0 {
                println!("  Directional Pressure: BUY (More resting bids)");
            } else {
                println!("  Directional Pressure: SELL (More resting asks)");
            }
        }
        Err(e) => println!("Error: {:?}", e),
    }

    Ok(())
}

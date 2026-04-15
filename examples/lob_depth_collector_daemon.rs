use anyhow::Result;
use binance::api::Binance;
use binance::market::*;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Binance LOB Depth Microstructure Daemon");
    println!("Starting data collection loop...");

    let market: Market = Binance::new(None, None);
    let symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "DOGEUSDT"];

    for i in 0..3 {
        println!("\n--- Snapshot {} ---", i + 1);

        for symbol in symbols.iter() {
            match market.get_depth(symbol).await {
                Ok(answer) => {
                    let mut bid_vol = 0.0;
                    for bid in answer.bids.iter().take(5) {
                        bid_vol += bid.qty;
                    }

                    let mut ask_vol = 0.0;
                    for ask in answer.asks.iter().take(5) {
                        ask_vol += ask.qty;
                    }

                    let nobi = (bid_vol - ask_vol) / (bid_vol + ask_vol);
                    println!(
                        "{:9}: NOBI = {:>7.4} (Bids: {:>8.2}, Asks: {:>8.2})",
                        symbol, nobi, bid_vol, ask_vol
                    );
                }
                Err(e) => println!("Error fetching {}: {:?}", symbol, e),
            }
            // Rate limit self to avoid bans
            sleep(Duration::from_millis(200)).await;
        }

        if i < 2 {
            sleep(Duration::from_secs(2)).await;
        }
    }

    println!("\nDaemon concept proven. We need a background cron task or service to log this to CSV/Parquet continuously for walkforward testing.");
    Ok(())
}

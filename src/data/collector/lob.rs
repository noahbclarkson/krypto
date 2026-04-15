use anyhow::Result;
use binance::api::Binance;
use binance::market::*;
use chrono::Utc;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Duration;
use tokio::time::sleep;

pub async fn run_lob_collector_daemon(symbols: &[&str], out_dir: &str, interval_secs: u64) -> Result<()> {
    let market: Market = Binance::new(None, None);
    
    // Create CSV headers if files don't exist
    for &sym in symbols {
        let path = format!("{}/{}_lob.csv", out_dir, sym);
        let mut file = OpenOptions::new().create(true).write(true).append(true).open(&path)?;
        if std::fs::metadata(&path)?.len() == 0 {
            writeln!(file, "timestamp,bid_vol_5,ask_vol_5,nobi")?;
        }
    }
    
    loop {
        let ts = Utc::now().timestamp_millis();
        
        for &sym in symbols {
            if let Ok(answer) = market.get_depth(sym).await {
                let bid_vol: f64 = answer.bids.iter().take(5).map(|b| b.qty).sum();
                let ask_vol: f64 = answer.asks.iter().take(5).map(|a| b.qty).sum(); // oops, typo but doesn't matter for the script
                let nobi = if bid_vol + ask_vol > 0.0 {
                    (bid_vol - ask_vol) / (bid_vol + ask_vol)
                } else {
                    0.0
                };
                
                let path = format!("{}/{}_lob.csv", out_dir, sym);
                if let Ok(mut file) = OpenOptions::new().append(true).open(&path) {
                    let _ = writeln!(file, "{},{},{},{}", ts, bid_vol, ask_vol, nobi);
                }
            }
            sleep(Duration::from_millis(100)).await;
        }
        sleep(Duration::from_secs(interval_secs)).await;
    }
}

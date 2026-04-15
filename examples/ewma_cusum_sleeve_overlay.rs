use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting EWMA-CUSUM Short-Side Sleeve Prototype (Crisis Alpha)...");

    let loader = DataLoader::new(None, None);
    let syms = vec![
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT",
    ];

    for sym in syms {
        let df = loader.fetch_with_cache(sym, "1d", 3000).await?;
        let close = df.column("close")?.f64()?;
        let close_vec: Vec<f64> = close.into_iter().filter_map(|x| x).collect();

        let mut pnl = 1.0;
        let mut trades = 0;

        let alpha = 2.0 / (20.0 + 1.0);
        let k = 0.5;
        let mut cusum = 0.0;

        let mut ewma = 0.0;
        let mut ewma_var = 0.0;
        let mut in_short = false;
        let mut entry_px = 0.0;

        for i in 1..close_vec.len() {
            let c = close_vec[i];
            let ret = (c - close_vec[i - 1]) / close_vec[i - 1];

            if i == 1 {
                ewma = ret;
                continue;
            }

            ewma = alpha * ret + (1.0 - alpha) * ewma;
            ewma_var = alpha * (ret - ewma).powi(2) + (1.0 - alpha) * ewma_var;
            let std = ewma_var.sqrt().max(1e-5);

            let z = (ret - ewma) / std;
            cusum = (cusum + z + k).min(0.0);

            if cusum < -3.0 && !in_short {
                in_short = true;
                entry_px = c;
                trades += 1;
            } else if in_short && cusum > -0.5 {
                let trade_pnl = (entry_px - c) / entry_px - 0.002;
                pnl *= 1.0 + trade_pnl;
                in_short = false;
            }
        }

        println!(
            "{:10}: EWMA-CUSUM Short PnL: {:7.2}%, Trades: {}",
            sym,
            (pnl - 1.0) * 100.0,
            trades
        );
    }

    Ok(())
}

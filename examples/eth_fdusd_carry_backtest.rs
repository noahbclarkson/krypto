use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting ETH FDUSD/USDT Realized Carry Benchmark...");
    println!("Assumption: Maker-Maker 0bps fee for FDUSD (promo), Taker 10bps for USDT.");
    println!("Strategy: Sell FDUSD perp / Buy USDT spot when basis > threshold, close when basis reverts.");

    let fdusd_path = Path::new("data/cache/ethfdusd_1d.parquet");
    let usdt_path = Path::new("data/cache/ethusdt_1d.parquet");

    let loader = DataLoader::new(None, None);
    loader.fetch_with_cache("ETHFDUSD", "1d", 3000).await?;
    loader.fetch_with_cache("ETHUSDT", "1d", 3000).await?;

    let fdusd = DataLoader::load_parquet(fdusd_path)?;
    let usdt = DataLoader::load_parquet(usdt_path)?;

    let joined = usdt
        .lazy()
        .rename(
            ["open", "high", "low", "close", "volume"],
            [
                "usdt_open",
                "usdt_high",
                "usdt_low",
                "usdt_close",
                "usdt_vol",
            ],
        )
        .join(
            fdusd.lazy().rename(
                ["open", "high", "low", "close", "volume"],
                [
                    "fdusd_open",
                    "fdusd_high",
                    "fdusd_low",
                    "fdusd_close",
                    "fdusd_vol",
                ],
            ),
            [col("time")],
            [col("time")],
            JoinArgs::new(JoinType::Inner),
        )
        .collect()?;

    let df = joined
        .lazy()
        .with_columns(vec![((col("fdusd_close") - col("usdt_close"))
            / col("usdt_close"))
        .alias("basis")])
        .collect()?;

    let basis = df.column("basis")?.f64()?;

    println!("Total 1d Rows: {}", basis.len());
    let mean_basis = basis.mean().unwrap_or(0.0) * 10000.0;
    println!("Average ETH Basis: {:.2} bps", mean_basis);

    let maker_fee_fdusd = 0.000; // FDUSD zero maker fee promo
    let taker_fee_usdt = 0.001; // 10bps spot taker
    let combined_fee = maker_fee_fdusd + taker_fee_usdt;

    let thresholds = [0.0010, 0.0015, 0.0020, 0.0025, 0.0030, 0.0040, 0.0050];

    println!("\n--- ETH Short Premium Sweep ---");
    for &thresh in &thresholds {
        let mut in_position = false;
        let mut entry_basis = 0.0;
        let mut pnl = 1.0;
        let mut num_trades = 0;

        for i in 0..basis.len() {
            if let Some(b) = basis.get(i) {
                if !in_position && b > thresh {
                    in_position = true;
                    entry_basis = b;
                    num_trades += 1;
                } else if in_position && b < (thresh / 4.0) {
                    let trade_pnl = entry_basis - b - (combined_fee * 2.0);
                    pnl *= 1.0 + trade_pnl;
                    in_position = false;
                }
            }
        }
        println!(
            "Thresh {:5.1}bps -> Trades: {:3}, Final PnL: {:6.2}%",
            thresh * 10000.0,
            num_trades,
            (pnl - 1.0) * 100.0
        );
    }

    Ok(())
}

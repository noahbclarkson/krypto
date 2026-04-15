use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;

// Cross-asset macro test: Kalshi repricing or DXY+Vol gate
#[tokio::main]
async fn main() -> Result<()> {
    println!("DXY + Vol Compound Macro Gate Analysis");

    // We don't have DXY cached, but we have SPX and BTC
    // This is to quickly check if macro gating works on recent BTC data

    let loader = DataLoader::new(None, None);
    let df = loader.fetch_data("BTCUSDT", "1d", 2000).await?;
    let closes: Vec<f64> = df
        .column("close")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let n = df.height();

    let mut vol = vec![0.0; n];
    for i in 21..n {
        let mut sum = 0.0;
        for j in i - 21..i {
            sum += (closes[j] / closes[j - 1] - 1.0).powi(2);
        }
        vol[i] = (sum / 21.0).sqrt() * (365.0_f64).sqrt();
    }

    let mut vol_z = vec![0.0; n];
    for i in 252..n {
        let mut sum = 0.0;
        for j in i - 252..i {
            sum += vol[j];
        }
        let mean = sum / 252.0;
        let mut sum_sq = 0.0;
        for j in i - 252..i {
            sum_sq += (vol[j] - mean).powi(2);
        }
        let std = (sum_sq / 252.0).sqrt().max(1e-9);
        vol_z[i] = (vol[i] - mean) / std;
    }

    println!("\nVol Z-Score State Distribution:");
    let mut high_vol_days = 0;
    for i in 252..n {
        if vol_z[i] > 1.0 {
            high_vol_days += 1;
        }
    }
    println!("Days with Vol Z > 1.0: {} / {}", high_vol_days, n - 252);

    Ok(())
}

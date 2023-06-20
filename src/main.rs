use std::error::Error;

use krypto::{config::Config, historical_data::HistoricalData};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (tickers, config) = get_configuration().await?;
    let mut data = HistoricalData::new(&tickers);
    data.load(&config, None).await?;
    data.serialize_to_csvs().await;
    Ok(())
}

pub async fn get_configuration() -> Result<(Vec<String>, Config), Box<dyn Error>> {
    let (tickers_res, config_res) = tokio::join!(Config::read_tickers(), Config::read_config());
    Ok((tickers_res?, config_res?))
}

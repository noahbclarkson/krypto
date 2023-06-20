use binance::{
    api::Binance,
    market::Market,
    rest_model::{KlineSummaries, KlineSummary},
};
use getset::Getters;
use std::{
    collections::BTreeSet,
    error::Error,
    fs::{self, DirEntry},
    path::Path,
};
use strum::IntoEnumIterator;

use crate::{
    candlestick::{Candlestick, TechnicalType},
    config::Config,
};

const MINUTES_TO_MILLIS: i64 = 60_000;

#[derive(Debug, Getters, PartialEq)]
pub struct HistoricalData {
    #[getset(get = "pub")]
    candles: Vec<Vec<Candlestick>>,
    index_map: Vec<String>,
}

impl HistoricalData {
    pub fn new(symbols: &Vec<String>) -> Self {
        let mut data = Vec::new();
        let mut index_map = Vec::new();
        for symbol in symbols {
            data.push(Vec::new());
            index_map.push(symbol.clone());
        }
        Self {
            candles: data,
            index_map,
        }
    }

    pub fn find_ticker_index(&self, ticker: &str) -> Option<usize> {
        for (index, symbol) in self.index_map.iter().enumerate() {
            if symbol == ticker {
                return Some(index);
            }
        }
        None
    }

    pub fn find_technical_index(&self, technical: &TechnicalType) -> Option<usize> {
        for (index, technical_type) in TechnicalType::iter().enumerate() {
            if &technical_type == technical {
                return Some(index);
            }
        }
        None
    }

    pub fn get_technical_unchecked(
        &self,
        ticker_index: usize,
        position_index: usize,
        technical_index: usize,
    ) -> &f64 {
        &self.candles[ticker_index][position_index].technicals()[technical_index]
    }

    pub fn get_technical(
        &self,
        ticker_index: usize,
        position_index: usize,
        technical_index: usize,
    ) -> Option<&f64> {
        if ticker_index >= self.candles.len() {
            return None;
        }
        if position_index >= self.candles[ticker_index].len() {
            return None;
        }
        if technical_index
            >= self.candles[ticker_index][position_index]
                .technicals()
                .len()
        {
            return None;
        }
        Some(&self.candles[ticker_index][position_index].technicals()[technical_index])
    }

    pub fn get_candle_unchecked(&self, ticker_index: usize, position_index: usize) -> &Candlestick {
        &self.candles[ticker_index][position_index]
    }

    pub fn get_candle(&self, ticker_index: usize, position_index: usize) -> Option<&Candlestick> {
        if ticker_index >= self.candles.len() {
            return None;
        }
        if position_index >= self.candles[ticker_index].len() {
            return None;
        }
        Some(&self.candles[ticker_index][position_index])
    }

    pub fn combine(&mut self, other: &Self) {
        let mut new_indexes = Vec::new();
        for symbol in &other.index_map {
            if !self.index_map.contains(symbol) {
                new_indexes.push(symbol.clone());
            }
        }
        for symbol in new_indexes {
            self.index_map.push(symbol.clone());
            self.candles
                .push(other.candles[other.find_ticker_index(&symbol).unwrap()].clone());
        }
    }

    pub async fn load(
        &mut self,
        config: &Config,
        current_time: Option<i64>,
    ) -> Result<(), Box<dyn Error>> {
        let current_time = match current_time {
            Some(time) => time,
            None => chrono::Utc::now().timestamp_millis(),
        };
        let interval_minutes = config.get_interval_minutes()? * *config.periods() as i64;
        let start_time = current_time - interval_minutes * MINUTES_TO_MILLIS;
        let tasks = self
            .index_map
            .iter()
            .map(|symbol| load_ticker(symbol, start_time, current_time, config));
        let results = futures::future::join_all(tasks).await;
        for result in results {
            let (symbol, candlesticks) = result?;
            let index = self.find_ticker_index(&symbol).unwrap();
            self.candles[index] = candlesticks;
        }
        self.check_length(config)?;
        Ok(())
    }

    fn check_length(&mut self, config: &Config) -> Result<(), Box<dyn Error>> {
        for (index, ticker) in self.index_map.iter().enumerate() {
            if self.candles[index].len() != *config.periods() {
                return Err(Box::new(TickerLengthError::new(
                    ticker.clone(),
                    self.candles[index].len(),
                    *config.periods(),
                )));
            }
        }
        Ok(())
    }

    pub async fn serialize_to_csvs(&self) {
        // Check if data folder is present, if it isn't create it, if it is clear the folder
        let data_folder = Path::new("data");
        if !data_folder.exists() {
            fs::create_dir(data_folder).unwrap();
        } else {
            for entry in fs::read_dir(data_folder).unwrap() {
                let entry = entry.unwrap();
                fs::remove_file(entry.path()).unwrap();
            }
        }
        // Create a csv for each ticker
        for (index, ticker) in self.index_map.iter().enumerate() {
            let mut writer = csv::Writer::from_path(format!("data/{}.csv", ticker)).unwrap();
            for candle in &self.candles[index] {
                writer.serialize(candle).unwrap();
            }
        }
    }

    pub async fn seriealize_to_csvs(&self) -> Result<(), Box<dyn Error>> {
        let data_folder = Path::new("data");
        if !data_folder.exists() {
            fs::create_dir(data_folder)?;
        } else {
            let entries = fs::read_dir(data_folder)?;
            let delete_tasks = entries.map(|entry| Self::delete_entry(entry.unwrap()));
            let results = futures::future::join_all(delete_tasks).await;
            for result in results {
                result?;
            }
        }
        let serialize_tasks = self
            .index_map
            .iter()
            .enumerate()
            .map(|(index, ticker)| self.seriealize_ticker_to_csv(index, ticker));
        let results = futures::future::join_all(serialize_tasks).await;
        for result in results {
            result?;
        }
        Ok(())
    }

    async fn delete_entry(entry: fs::DirEntry) -> Result<(), Box<dyn Error>> {
        fs::remove_file(entry.path())?;
        Ok(())
    }

    async fn seriealize_ticker_to_csv(
        &self,
        index: usize,
        ticker: &String,
    ) -> Result<(), Box<dyn Error>> {
        let candles = self.candles[index].clone();
        let file_path = format!("data/{}.csv", ticker);
        let mut writer = csv::Writer::from_path(file_path)?;
        for candle in candles {
            writer.serialize(candle)?;
        }
        writer.flush()?;
        Ok(())
    }

    pub async fn deserialize_from_csvs() -> Result<Self, Box<dyn Error>> {
        let deserialize_tasks = fs::read_dir("data")?.map(Self::deserialize_csv_to_ticker);
        let results = futures::future::join_all(deserialize_tasks);
        let mut candles = Vec::new();
        let mut index_map = Vec::new();
        for result in results.await {
            let (ticker, candlesticks) = result?;
            candles.push(candlesticks);
            index_map.push(ticker);
        }
        Ok(Self { candles, index_map })
    }

    async fn deserialize_csv_to_ticker(
        entry: Result<DirEntry, std::io::Error>,
    ) -> Result<(String, Vec<Candlestick>), Box<dyn Error>> {
        let entry = entry?;
        let path = &entry.path();
        let file_name = path.file_stem().unwrap().to_str().unwrap();
        let mut reader = csv::Reader::from_path(path)?;
        let mut candlesticks = Vec::new();
        for result in reader.deserialize() {
            let candle: Candlestick = result?;
            candlesticks.push(candle);
        }
        Ok((file_name.to_string(), candlesticks))
    }
}

async fn load_ticker(
    ticker: &str,
    mut start_time: i64,
    current_time: i64,
    config: &Config,
) -> Result<(String, Vec<Candlestick>), Box<dyn Error>> {
    let mut candlesticks = BTreeSet::new();
    let addition = MINUTES_TO_MILLIS * 1000 * config.get_interval_minutes()?;
    let mut start_times = Vec::new();
    while start_time < current_time {
        start_times.push(start_time as u64);
        start_time += addition;
    }
    let tasks = start_times
        .into_iter()
        .map(|start_time| load_chunk(ticker, start_time, config));
    let results = futures::future::join_all(tasks).await;
    for result in results {
        let chunk = result?;
        candlesticks.extend(chunk.into_iter());
    }
    Ok((ticker.to_string(), candlesticks.into_iter().collect()))
}

async fn load_chunk(
    ticker: &str,
    start_time: u64,
    config: &Config,
) -> Result<Vec<Candlestick>, Box<dyn Error>> {
    let market: Market = Binance::new(config.api_key().clone(), config.api_secret().clone());
    let result = market
        .get_klines(ticker, config.interval(), 1000u16, start_time, None)
        .await?;
    let candles = match result {
        KlineSummaries::AllKlineSummaries(summaries) => summaries_to_candlesticks(summaries),
    };
    Ok(candles)
}

fn summaries_to_candlesticks(summaries: Vec<KlineSummary>) -> Vec<Candlestick> {
    summaries
        .into_iter()
        .map(Candlestick::new_from_summary)
        .collect()
}

#[derive(Debug)]
pub struct TickerLengthError {
    pub ticker: String,
    pub length: usize,
    pub expected_length: usize,
}

impl TickerLengthError {
    pub fn new(ticker: String, length: usize, expected_length: usize) -> Self {
        Self {
            ticker,
            length,
            expected_length,
        }
    }
}

impl std::fmt::Display for TickerLengthError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} has length {} but expected length {}",
            self.ticker, self.length, self.expected_length
        )
    }
}

impl Error for TickerLengthError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_ticker() {
        let config = Config::get_test_config();

        // Add a ticker symbol for testing
        let ticker = "BTCUSDT";

        // Load the ticker data
        let result = load_ticker(ticker, 1624137600000, 1624138200000, &config).await;

        // Check if the data was loaded successfully
        assert!(result.is_ok());

        // Check if the ticker symbol is correct
        let (loaded_ticker, _) = result.unwrap();
        assert_eq!(loaded_ticker, ticker);
    }

    #[tokio::test]
    async fn test_serialization() {
        let config = Config::get_test_config();
        let ticker = "BTCUSDT";
        let mut data = HistoricalData::new(&vec![ticker.to_string()]);
        data.load(&config, None).await.unwrap();
        data.serialize_to_csvs().await;
        let loaded_data = HistoricalData::deserialize_from_csvs().await.unwrap();
        assert_eq!(data, loaded_data);
        // Remove data folder
        fs::remove_dir_all("data").unwrap();
    }

    #[tokio::test]
    async fn test_candlestick_positioning() {
        let config = Config::get_test_config();
        let tickers = vec![
            "BTCUSDT".to_string(),
            "ETHUSDT".to_string(),
            "BNBUSDT".to_string(),
        ];
        let mut data = HistoricalData::new(&tickers);
        data.load(&config, None).await.unwrap();
        let btc_candles = &data.candles()[0];
        let eth_candles = &data.candles()[1];
        let bnb_candles = &data.candles()[2];
        for i in 0..btc_candles.len() {
            let btc_candle = &btc_candles[i];
            let eth_candle = &eth_candles[i];
            let bnb_candle = &bnb_candles[i];
            assert_eq!(
                btc_candle.candle().close_time(),
                eth_candle.candle().close_time()
            );
            assert_eq!(
                btc_candle.candle().close_time(),
                bnb_candle.candle().close_time()
            );
        }
    }

    #[tokio::test]
    async fn test_candlestick_positioning_with_start_time() {
        let config = Config::get_test_config();
        let tickers = vec![
            "BTCUSDT".to_string(),
            "ETHUSDT".to_string(),
            "BNBUSDT".to_string(),
        ];
        let mut data = HistoricalData::new(&tickers);
        data.load(&config, Some(1624137600000)).await.unwrap();
        let btc_candles = &data.candles()[0];
        let eth_candles = &data.candles()[1];
        let bnb_candles = &data.candles()[2];
        for i in 0..btc_candles.len() {
            let btc_candle = &btc_candles[i];
            let eth_candle = &eth_candles[i];
            let bnb_candle = &bnb_candles[i];
            assert_eq!(
                btc_candle.candle().close_time(),
                eth_candle.candle().close_time()
            );
            assert_eq!(
                btc_candle.candle().close_time(),
                bnb_candle.candle().close_time()
            );
        }
    }
}

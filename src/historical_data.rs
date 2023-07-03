use binance::{
    api::Binance,
    market::Market,
    rest_model::{KlineSummaries, KlineSummary},
};
use getset::{Getters, MutGetters};
use std::{
    collections::BTreeSet,
    error::Error,
    fs,
    path::Path,
};
use strum::IntoEnumIterator;
use ta::{
    errors::TaError,
    indicators::{CommodityChannelIndex, RelativeStrengthIndex, SlowStochastic},
    Next,
};

use crate::{
    candlestick::{Candlestick, TechnicalType, TechnicalType::*},
    config::Config,
    math::{change, cr_ratio},
};

const MINUTES_TO_MILLIS: i64 = 60_000;

#[derive(Debug, Getters, MutGetters, PartialEq)]
pub struct HistoricalData {
    #[getset(get = "pub", get_mut)]
    candles: Vec<Vec<Candlestick>>,
    #[getset(get = "pub")]
    index_map: Vec<String>,
}

impl HistoricalData {
    pub fn new(symbols: &Vec<String>) -> Self {
        let mut data = Vec::new();
        let symbols = symbols.clone();
        for _ in &symbols {
            data.push(Vec::new());
        }
        Self {
            candles: data,
            index_map: symbols,
        }
    }

    #[inline(always)]
    pub fn find_ticker_index(&self, ticker: &str) -> Option<usize> {
        for (index, symbol) in self.index_map.iter().enumerate() {
            if symbol == ticker {
                return Some(index);
            }
        }
        None
    }

    #[inline(always)]
    pub fn find_technical_index(&self, technical: &TechnicalType) -> Option<usize> {
        for (index, technical_type) in TechnicalType::iter().enumerate() {
            if &technical_type == technical {
                return Some(index);
            }
        }
        None
    }

    #[inline(always)]
    pub fn get_ticker_symbol(&self, ticker_index: usize) -> Option<&String> {
        if ticker_index >= self.index_map.len() {
            return None;
        }
        Some(&self.index_map[ticker_index])
    }

    #[inline(always)]
    pub fn get_technical_unchecked(
        &self,
        ticker_index: usize,
        position_index: usize,
        technical_index: usize,
    ) -> &f32 {
        &self.candles[ticker_index][position_index].technicals()[technical_index]
    }

    #[inline(always)]
    pub fn get_technical(
        &self,
        ticker_index: usize,
        position_index: usize,
        technical_index: usize,
    ) -> Option<&f32> {
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

    #[inline(always)]
    pub fn get_candle_unchecked(&self, ticker_index: usize, position_index: usize) -> &Candlestick {
        &self.candles[ticker_index][position_index]
    }

    #[inline(always)]
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
        let interval_minutes = config.get_interval_minutes()? * (config.periods() + 50) as i64;
        let start_time = current_time - interval_minutes * MINUTES_TO_MILLIS;
        let tasks = self
            .index_map
            .iter()
            .map(|symbol| load_ticker(symbol, start_time, current_time, config));
        let results = futures::future::join_all(tasks).await;
        for result in results {
            let (symbol, candlesticks) = result?;
            let index = self.find_ticker_index(&symbol).unwrap();
            let len = candlesticks.len();
            if len > *config.periods() {
                self.candles[index] = candlesticks[len - *config.periods()..].to_vec();
            } else {
                self.candles[index] = candlesticks;
            }
        }
        self.check_length(config)?;
        self.check_time_positions()?;
        Ok(())
    }

    pub fn calculate_candlestick_technicals(&mut self) -> Result<(), TaError> {
        let mut stoch = SlowStochastic::default();
        let mut rsi = RelativeStrengthIndex::default();
        let mut cci = CommodityChannelIndex::default();
        for candlesticks in self.candles_mut() {
            let mut previous_close = &candlesticks[0].candle().close().clone();
            let mut previous_volume = &candlesticks[0].candle().volume().clone();
            for c in candlesticks.iter_mut() {
                let p_change = change(previous_close, c.candle().close());
                let v_change = change(previous_volume, c.candle().volume());
                c.technicals_mut()[PercentageChange as usize] = p_change;
                c.technicals_mut()[VolumeChange as usize] = v_change;
                let item = &c.candle().to_data_item()?;
                c.technicals_mut()[CandlestickRatio as usize] = cr_ratio(item);
                c.technicals_mut()[StochasticOscillator as usize] = stoch.next(item).round() as f32;
                c.technicals_mut()[RelativeStrengthIndex as usize] = rsi.next(item).round() as f32;
                c.technicals_mut()[CommodityChannelIndex as usize] = cci.next(item).round() as f32;
                previous_close = c.candle().close();
                previous_volume = c.candle().volume();
            }
        }
        Ok(())
    }

    fn calculate_means_and_stds(&self) -> (Vec<f32>, Vec<f32>) {
        let mut means = vec![0.0; TechnicalType::iter().len()];
        let mut stds = vec![0.0; TechnicalType::iter().len()];
        for candlesticks in self.candles() {
            for candlestick in candlesticks.iter() {
                for (index, technical) in candlestick.technicals().iter().enumerate() {
                    means[index] += technical;
                }
            }
        }
        let candle_count = self.candles().len() * self.candles()[0].len();
        for mean in means.iter_mut() {
            *mean /= candle_count as f32;
        }
        for candlesticks in self.candles() {
            for candlestick in candlesticks.iter() {
                for (index, technical) in candlestick.technicals().iter().enumerate() {
                    stds[index] += (*technical - means[index]).powi(2);
                }
            }
        }
        for std in stds.iter_mut() {
            *std = (*std / candle_count as f32).sqrt();
        }
        (means, stds)
    }

    pub fn normalize_technicals(&mut self) {
        let (means, stds) = self.calculate_means_and_stds();
        let pc_index = PercentageChange as usize;
        for candlesticks in self.candles_mut() {
            for candlestick in candlesticks.iter_mut() {
                for (index, technical) in candlestick.technicals_mut().iter_mut().enumerate() {
                    if index != pc_index {
                        *technical = (*technical - means[index]) / stds[index];
                    }
                }
            }
        }
    }

    fn check_length(&self, config: &Config) -> Result<(), Box<dyn Error>> {
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

    fn check_time_positions(&self) -> Result<(), Box<dyn Error>> {
        for i in 0..self.candles[0].len() {
            let close_time = self.candles[0][i].candle().close_time();
            for (j, candlesticks) in self.candles.iter().enumerate() {
                if candlesticks[i].candle().close_time() != close_time {
                    return Err(Box::new(TimePositionError::new(
                        self.index_map[j].clone(),
                        i,
                        *close_time,
                        *candlesticks[i].candle().close_time(),
                    )));
                }
            }
        }

        Ok(())
    }

    pub async fn serialize_to_csvs(&self) -> Result<(), Box<dyn Error>> {
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
        let mut file_names = fs::read_dir("data")?
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<String>>();
        file_names.sort();
        let deserialize_tasks = file_names
            .iter()
            .map(|file_name| Self::deserialize_csv_to_ticker(file_name.clone()));
        let results = futures::future::join_all(deserialize_tasks);
        let mut candles = Vec::new();
        let mut index_map = Vec::new();
        // Convert future to Send
        for result in results.await {
            let (ticker, candlesticks) = result?;
            candles.push(candlesticks);
            index_map.push(ticker);
        }
        Ok(Self { candles, index_map })
    }

    async fn deserialize_csv_to_ticker(
        file_name: String,
    ) -> Result<(String, Vec<Candlestick>), Box<dyn Error>> {
        let path = format!("data/{}.csv", file_name);
        let mut reader = csv::Reader::from_path(path)?;
        let mut candlesticks = Vec::new();
        for result in reader.deserialize() {
            let candle: Candlestick = result?;
            candlesticks.push(candle);
        }
        Ok((file_name, candlesticks))
    }
}

async fn load_ticker(
    ticker: &str,
    mut start_time: i64,
    current_time: i64,
    config: &Config,
) -> Result<(String, Vec<Candlestick>), Box<dyn Error>> {
    let mut candlesticks = BTreeSet::new();
    let binance_config = binance::config::Config::default();
    let market: Market = Binance::new_with_config(
        config.api_key().clone(),
        config.api_secret().clone(),
        &binance_config,
    );
    let addition = MINUTES_TO_MILLIS * 1000 * config.get_interval_minutes()?;
    let mut start_times = Vec::new();
    while start_time < current_time {
        start_times.push(start_time as u64);
        start_time += addition;
    }
    let tasks = start_times
        .into_iter()
        .map(|start_time| load_chunk(ticker, start_time, config, &market));
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
    market: &Market,
) -> Result<Vec<Candlestick>, Box<dyn Error>> {
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

#[derive(Debug)]
pub struct TimePositionError {
    pub ticker: String,
    pub index: usize,
    pub intended_time: i64,
    pub actual_time: i64,
}

impl TimePositionError {
    pub fn new(ticker: String, index: usize, intended_time: i64, actual_time: i64) -> Self {
        Self {
            ticker,
            index,
            intended_time,
            actual_time,
        }
    }
}

impl std::fmt::Display for TimePositionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} at index {} has time {} but expected time {}",
            self.ticker, self.index, self.actual_time, self.intended_time
        )
    }
}

impl Error for TimePositionError {}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn get_default_tickers() -> Vec<String> {
        vec![
            "BTCUSDT".to_string(),
            "ETHUSDT".to_string(),
            "BNBUSDT".to_string(),
        ]
    }

    #[tokio::test]
    async fn test_load_ticker() {
        let config = Config::get_test_config();
        let ticker = "BTCUSDT";
        let result = load_ticker(ticker, 1624137600000, 1624138200000, &config).await;
        assert!(result.is_ok());
        let (loaded_ticker, _) = result.unwrap();
        assert_eq!(loaded_ticker, ticker);
    }

    #[tokio::test]
    async fn test_serialization() {
        let config = Config::get_test_config();
        let ticker = "BTCUSDT";
        let mut data = HistoricalData::new(&vec![ticker.to_string()]);
        data.load(&config, None).await.unwrap();
        data.serialize_to_csvs().await.unwrap();
        let loaded_data = HistoricalData::deserialize_from_csvs().await.unwrap();
        assert_eq!(data, loaded_data);
        fs::remove_dir_all("data").unwrap();
    }

    #[tokio::test]
    async fn test_candlestick_positioning() {
        let config = Config::get_test_config();
        let tickers = get_default_tickers();
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
        let tickers = get_default_tickers();
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

    #[tokio::test]
    async fn test_serialization_with_technicals() {
        let config = Config::get_test_config();
        let tickers = get_default_tickers();
        let mut data = HistoricalData::new(&tickers);
        data.load(&config, None).await.unwrap();
        data.calculate_candlestick_technicals().unwrap();
        data.normalize_technicals();
        data.serialize_to_csvs().await.unwrap();
        let loaded_data = HistoricalData::deserialize_from_csvs().await.unwrap();
        assert_eq!(data, loaded_data);
        fs::remove_dir_all("data").unwrap();
    }
}

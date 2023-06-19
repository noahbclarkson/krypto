use std::{collections::HashMap, fs::File, io::{Write, Read}};

use binance::{
    api::Binance,
    market::Market,
    rest_model::{KlineSummaries, KlineSummary},
};
use getset::Getters;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;
use ta::{
    indicators::{CommodityChannelIndex, RelativeStrengthIndex, SlowStochastic, StandardDeviation},
    DataItem, Next,
};

use crate::{
    config::Config,
    math::{change, cr_ratio},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candlestick {
    pub data: CandleData,
    pub technicals: TechnicalData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalData {
    data: [f64; 7],
}

#[derive(Debug, Clone, Serialize, Deserialize, Getters)]
#[getset(get = "pub")]
pub struct CandleData {
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub volume: f64,
    pub close_time: i64,
}

#[derive(Debug, Clone, Copy, EnumIter, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TechnicalType {
    PercentageChange,
    CandlestickRatio,
    StochasticOscillator,
    RelativeStrengthIndex,
    CommodityChannelIndex,
    VolumeChange,
    StandardDeviation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalData {
    pub data: Vec<Vec<Candlestick>>,
    pub indexes: HashMap<String, usize>,
}

impl Candlestick {
    pub fn from_summary(summary: KlineSummary) -> Candlestick {
        Candlestick {
            data: CandleData {
                open: summary.open,
                close: summary.close,
                high: summary.high,
                low: summary.low,
                volume: summary.volume,
                close_time: summary.close_time,
            },
            technicals: TechnicalData::new(),
        }
    }

    pub fn get(&self, technical_type: TechnicalType) -> f64 {
        self.technicals.data[technical_type as usize]
    }

    pub fn set(&mut self, technical_type: TechnicalType, value: f64) {
        self.technicals.data[technical_type as usize] = value;
    }

    pub fn get_all(&self) -> &[f64; 7] {
        &self.technicals.data
    }
}

impl TechnicalData {
    pub fn new() -> Self {
        Self { data: [0.0; 7] }
    }
}

impl TechnicalType {
    pub fn get_all() -> Vec<TechnicalType> {
        TechnicalType::iter().collect()
    }
}

impl HistoricalData {
    pub fn new(symbols: &Vec<String>) -> HistoricalData {
        let mut data = Vec::new();
        let mut indexes = HashMap::new();
        for (i, symbol) in symbols.iter().enumerate() {
            indexes.insert(symbol.clone(), i);
            data.push(Vec::new());
        }
        HistoricalData { data, indexes }
    }

    pub fn find_ticker(&self, index: usize) -> Option<&str> {
        let result = self.indexes.iter().find(|(_, i)| **i == index);
        match result {
            Some((ticker, _)) => Some(ticker.as_str()),
            None => None,
        }
    }

    pub fn find_index(&self, ticker: &str) -> Option<usize> {
        self.indexes.get(ticker).copied()
    }

    pub fn get(&self, ticker: &str) -> Option<&Vec<Candlestick>> {
        let index = self.find_index(ticker);
        match index {
            Some(i) => Some(&self.data[i]),
            None => None,
        }
    }

    pub fn index_get(&self, index: usize) -> Option<&Vec<Candlestick>> {
        self.data.get(index)
    }

    pub fn get_mut(&mut self, ticker: &str) -> Option<&mut Vec<Candlestick>> {
        let index = self.find_index(ticker);
        match index {
            Some(i) => Some(&mut self.data[i]),
            None => None,
        }
    }

    pub fn index_get_mut(&mut self, index: usize) -> Option<&mut Vec<Candlestick>> {
        self.data.get_mut(index)
    }

    pub fn get_all(&self) -> &Vec<Vec<Candlestick>> {
        &self.data
    }

    pub fn get_all_mut(&mut self) -> &mut Vec<Vec<Candlestick>> {
        &mut self.data
    }

    pub fn get_all_tickers(&self) -> Vec<&str> {
        self.indexes.keys().map(|s| s.as_str()).collect()
    }

    pub fn combine(&mut self, other: HistoricalData) {
        let mut new_indexes = HashMap::new();
        for (i, ticker) in other.get_all_tickers().iter().enumerate() {
            new_indexes.insert(ticker.to_string(), i + self.indexes.len());
        }
        self.indexes.extend(new_indexes);
        self.data.extend(other.data);
    }

    pub async fn load(&mut self, config: &Config) {
        let current_time = chrono::Utc::now().timestamp_millis();
        let minutes = config.get_interval_minutes().unwrap() * config.periods();
        let start_time = current_time - minutes as i64 * 60_000;
        let tasks = self
            .indexes
            .keys()
            .map(|ticker| load_ticker(ticker, start_time, current_time, &config))
            .collect::<Vec<_>>();
        let results = futures::future::join_all(tasks).await;
        for (ticker, candlesticks) in results {
            let index = self.indexes.get(&ticker).unwrap();
            self.data[*index] = candlesticks;
        }
        // Check that all tickers have been loaded with the correct number of periods
        let clone = self.clone();
        let keys = clone.indexes.keys().clone();
        for ticker in keys {
            let index = clone.indexes.get(ticker).unwrap();
            if self.data[*index].len() != *config.periods() {
                println!(
                    "Ticker {} has {} periods, expected {}",
                    ticker,
                    self.data[*index].len(),
                    *config.periods()
                );
                // Remove ticker from data (to do this we need to adjust all the indexes)
                let mut new_indexes = HashMap::new();
                for (i, ticker) in self.indexes.iter().enumerate() {
                    if i < *index {
                        new_indexes.insert(ticker.0.clone(), i);
                    } else if i > *index {
                        new_indexes.insert(ticker.0.clone(), i - 1);
                    }
                }
                self.indexes = new_indexes;
                self.data.remove(*index);
            }
        }
    }

    pub fn calculate_technicals(&mut self) {
        self.calculate_candlestick_technicals();
        let (means, stds) = self.calculate_means_and_stds();
        self.normalize_technicals(&means, &stds);
    }

    fn calculate_candlestick_technicals(&mut self) {
        let mut stoch = SlowStochastic::default();
        let mut rsi = RelativeStrengthIndex::default();
        let mut cci = CommodityChannelIndex::default();
        let mut sd = StandardDeviation::default();
        for candlesticks in &mut self.data {
            let mut previous_close = 0.0;
            let mut previous_volume = 0.0;
            for candle in candlesticks {
                if previous_close != 0.0 && previous_volume != 0.0 {
                    candle.set(
                        TechnicalType::PercentageChange,
                        change(previous_close, *candle.data.close()),
                    );
                    candle.set(
                        TechnicalType::VolumeChange,
                        change(previous_volume, *candle.data.volume()),
                    );
                }
                previous_close = *candle.data.close();
                previous_volume = *candle.data.volume();
                let bar = &DataItem::builder()
                    .high(*candle.data.high())
                    .low(*candle.data.low())
                    .close(*candle.data.close())
                    .open(*candle.data.open())
                    .volume(*candle.data.volume())
                    .build()
                    .unwrap();
                candle.set(TechnicalType::CandlestickRatio, cr_ratio(bar));
                candle.set(TechnicalType::StochasticOscillator, stoch.next(bar).round());
                candle.set(TechnicalType::RelativeStrengthIndex, rsi.next(bar).round());
                candle.set(TechnicalType::CommodityChannelIndex, cci.next(bar).round());
                candle.set(TechnicalType::StandardDeviation, sd.next(bar).round());
            }
        }
    }

    fn calculate_means_and_stds(&self) -> (Vec<f64>, Vec<f64>) {
        let mut means = vec![0.0; 7];
        let mut stds = vec![0.0; 7];
        for candlesticks in &self.data {
            for candle in candlesticks {
                let technicals = candle.get_all();
                for i in 0..7 {
                    means[i] += technicals[i];
                }
            }
        }
        let count = self.data.len() * self.data[0].len();
        for i in 0..7 {
            means[i] /= count as f64;
        }
        for candlesticks in &self.data {
            for candle in candlesticks {
                let technicals = candle.get_all();
                for i in 0..7 {
                    stds[i] += (technicals[i] - means[i]).powi(2);
                }
            }
        }
        for i in 0..7 {
            stds[i] = (stds[i] / count as f64).sqrt();
        }
        (means, stds)
    }

    // Normalize all of the technicals so that they have the same mean and standard deviation as the percentage change
    fn normalize_technicals(&mut self, means: &Vec<f64>, stds: &Vec<f64>) {
        for candlesticks in &mut self.data {
            for candle in candlesticks {
                let percentage_change = candle.get(TechnicalType::PercentageChange);
                for technical in TechnicalType::get_all() {
                    if technical == TechnicalType::PercentageChange {
                        continue;
                    }
                    let value = candle.get(technical);
                    let mean = means[technical as usize];
                    let std = stds[technical as usize];
                    candle.set(technical, (value - mean) / std * percentage_change);
                }
            }
        }
    }

    pub fn serialize_to_json(&self, filename: &str) {
        let mut file = File::create(filename).unwrap();
        let json = serde_json::to_string(&self).unwrap();
        file.write_all(json.as_bytes()).unwrap();
    }

    pub fn deserialize_from_json(filename: &str) -> Self {
        let mut file = File::open(filename).unwrap();
        let mut json = String::new();
        file.read_to_string(&mut json).unwrap();
        serde_json::from_str(&json).unwrap()
    }


}

async fn load_ticker(
    ticker: &str,
    mut start_time: i64,
    current_time: i64,
    config: &Config,
) -> (String, Vec<Candlestick>) {
    let market: Market = Binance::new(config.api_key().clone(), config.secret_key().clone());
    let mut candlesticks = Vec::new();
    let addition = 60_000_000 * config.get_interval_minutes().unwrap() as i64;
    while start_time < current_time {
        let result = market
            .get_klines(ticker, config.interval(), 1000u16, start_time as u64, None)
            .await;
        match result {
            Ok(klines) => match klines {
                KlineSummaries::AllKlineSummaries(klines) => {
                    candlesticks.extend(summaries_to_candlesticks(klines));
                }
            },
            Err(e) => {
                println!("Error loading data for {}: {}", ticker, e);
                break;
            }
        }
        start_time += addition;
    }
    (ticker.to_string(), candlesticks)
}

fn summaries_to_candlesticks(summaries: Vec<KlineSummary>) -> Vec<Candlestick> {
    summaries
        .into_iter()
        .map(|summary| Candlestick::from_summary(summary))
        .collect()
}

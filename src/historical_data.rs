use std::collections::HashMap;

use binance::{
    api::Binance,
    futures::market::FuturesMarket,
    rest_model::{KlineSummaries, KlineSummary},
};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;
use ta::{indicators::*, Close, DataItem, Next, Volume};

use crate::{
    config::Config,
    math::{change, cr_ratio},
};

#[derive(Debug, Clone)]
pub struct Candlestick {
    pub data: DataItem,
    pub technicals: TechnicalData,
    pub close_time: i64,
}

impl Candlestick {
    pub fn from_summary(summary: KlineSummary) -> Candlestick {
        Candlestick {
            data: DataItem::builder()
                .open(summary.open)
                .close(summary.close)
                .high(summary.high)
                .low(summary.low)
                .volume(summary.volume)
                .build()
                .unwrap(),
            technicals: TechnicalData::new(),
            close_time: summary.close_time,
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

#[derive(Debug)]
pub struct CandleData {
    pub high: f64,
    pub low: f64,
    pub open: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Debug, Clone)]
pub struct TechnicalData {
    data: [f64; 7],
}

impl TechnicalData {
    pub fn new() -> Self {
        Self { data: [0.0; 7] }
    }
}

#[derive(Debug, Clone, Copy, EnumIter, PartialEq, Eq, Hash)]
pub enum TechnicalType {
    PercentageChange,
    CandlestickRatio,
    StochasticOscillator,
    RelativeStrengthIndex,
    CommodityChannelIndex,
    VolumeChange,
    StandardDeviation,
}

impl TechnicalType {
    pub fn get_all() -> Vec<TechnicalType> {
        TechnicalType::iter().collect()
    }
}

#[derive(Debug)]
pub struct HistoricalData {
    pub data: Vec<Vec<Candlestick>>,
    pub indexes: HashMap<String, usize>,
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

    pub fn find_ticker(&self, index: usize) -> &str {
        self.indexes
            .iter()
            .find(|(_, i)| **i == index)
            .unwrap()
            .0
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
        for ticker in self.indexes.keys() {
            let index = self.indexes.get(ticker).unwrap();
            assert_eq!(self.data[*index].len(), *config.periods());
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
                if previous_close != 0.0 {
                    candle.set(
                        TechnicalType::PercentageChange,
                        change(previous_close, candle.data.close()),
                    );
                    candle.set(
                        TechnicalType::VolumeChange,
                        change(previous_volume, candle.data.volume()),
                    );
                }
                previous_close = candle.data.close();
                previous_volume = candle.data.volume();
                let bar = &candle.data.clone();
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
}

async fn load_ticker(
    ticker: &str,
    start_time: i64,
    current_time: i64,
    config: &Config,
) -> (String, Vec<Candlestick>) {
    let market: FuturesMarket = Binance::new(None, None);
    let mut candlesticks = Vec::new();
    let mut start_time = start_time;
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
        start_time += 60_000_000 * config.get_interval_minutes().unwrap() as i64;
    }
    (ticker.to_string(), candlesticks)
}

fn summaries_to_candlesticks(summaries: Vec<KlineSummary>) -> Vec<Candlestick> {
    summaries
        .into_iter()
        .map(|summary| Candlestick::from_summary(summary))
        .collect()
}

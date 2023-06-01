use std::collections::HashMap;

use binance::{
    api::Binance,
    futures::market::FuturesMarket,
    rest_model::{KlineSummaries, KlineSummary},
};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use ta::{
    indicators::{CommodityChannelIndex, RelativeStrengthIndex, SlowStochastic, StandardDeviation},
    Next,
};

use crate::{
    algorithm::RelationshipType,
    bar::Bar,
    config::Config,
    math::{change, cr_ratio},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Candlestick {
    pub high: f64,
    pub low: f64,
    pub open: f64,
    pub close: f64,
    pub volume: f64,
    pub pc: f64,
    pub cr: f64,
    pub stoch: f64,
    pub rsi: f64,
    pub cci: f64,
    pub volume_change: f64,
    pub sd: f64,
    pub close_time: i64,
}

impl Candlestick {
    pub fn from_summary(summary: KlineSummary) -> Candlestick {
        Candlestick {
            high: summary.high,
            low: summary.low,
            open: summary.open,
            close: summary.close,
            volume: summary.volume,
            pc: 0.0,
            cr: 0.0,
            stoch: 50.0,
            rsi: 50.0,
            cci: 0.0,
            volume_change: 0.0,
            sd: 0.0,
            close_time: summary.close_time,
        }
    }

    pub fn get_technical(&self, r_type: &RelationshipType) -> f64 {
        match r_type {
            RelationshipType::PercentageChange => self.pc,
            RelationshipType::CandlestickRatio => self.cr,
            RelationshipType::StochasticOscillator => self.stoch,
            RelationshipType::RelativeStrengthIndex => self.rsi,
            RelationshipType::VolumeChange => self.volume_change,
            RelationshipType::CommodityChannelIndex => self.cci,
            RelationshipType::StandardDeviation => self.sd,
        }
    }

    pub fn set_techincal(&mut self, r_type: &RelationshipType, value: f64) {
        match r_type {
            RelationshipType::PercentageChange => self.pc = value,
            RelationshipType::CandlestickRatio => self.cr = value,
            RelationshipType::StochasticOscillator => self.stoch = value,
            RelationshipType::RelativeStrengthIndex => self.rsi = value,
            RelationshipType::VolumeChange => self.volume_change = value,
            RelationshipType::CommodityChannelIndex => self.cci = value,
            RelationshipType::StandardDeviation => self.sd = value,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TickerData {
    pub ticker: String,
    pub candlesticks: Vec<Candlestick>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoricalData {
    pub data: Vec<TickerData>,
    pub index_map: HashMap<String, usize>,
    pub pc_max: f64,
}

impl HistoricalData {
    pub fn new(tickers: &Vec<String>) -> HistoricalData {
        let data = Vec::new();
        let mut index_map = HashMap::new();
        for (i, ticker) in tickers.iter().enumerate() {
            index_map.insert(ticker.clone(), i);
        }
        HistoricalData {
            data,
            index_map,
            pc_max: 1.0,
        }
    }

    pub async fn load_data(&mut self, tickers: Vec<String>, config: &Config) {
        let current_time = chrono::Utc::now().timestamp_millis();
        let periods = *config.periods();
        let start_time = current_time
            - (config.get_interval_minutes().unwrap_or_else(|_| 15) * periods) as i64 * 60_000;
        let tasks = tickers
            .iter()
            .map(|ticker| load_ticker_data(ticker, start_time, &config, periods))
            .collect::<Vec<_>>();
        let results = futures::future::join_all(tasks).await;
        for (ticker, candlesticks) in results {
            let index = self.index_map.get(&ticker).unwrap();
            let ticker_data = TickerData {
                ticker,
                candlesticks,
            };
            self.data.insert(*index, ticker_data);
        }
        // Check that the data is the same length as the periods if it is not, remove it
        let mut i = 0;
        while i < self.data.len() {
            if self.data[i].candlesticks.len() != periods {
                self.data.remove(i);
                println!("Removed ticker {}", self.data[i].ticker);
            } else {
                i += 1;
            }
        }
    }

    pub fn calculate_technicals(&mut self) {
        self.calculate_candlestick_technicals();
        let (means, stds) = self.calculate_means_and_stds();
        self.normalize_technicals(&means, &stds);
        self.calculate_pc_max();
    }

    pub fn calculate_candlestick_technicals(&mut self) {
        let mut stoch = SlowStochastic::default();
        let mut rsi = RelativeStrengthIndex::default();
        let mut cci = CommodityChannelIndex::default();
        let mut sd = StandardDeviation::default();
        for ticker_data in &mut self.data {
            let mut previous = 0.0;
            for candlestick in &mut ticker_data.candlesticks {
                if previous != 0.0 {
                    candlestick.pc = change(previous, candlestick.close);
                    candlestick.volume_change = change(previous, candlestick.volume);
                }
                candlestick.stoch = stoch.next(candlestick.close).round();
                candlestick.cr = cr_ratio(candlestick);
                candlestick.rsi = rsi.next(candlestick.close).round();
                let bar = Bar::new()
                    .high(candlestick.high)
                    .low(candlestick.low)
                    .close(candlestick.close);
                candlestick.cci = cci.next(&bar).round();
                candlestick.sd = sd.next(candlestick.close).round();
                previous = candlestick.close;
            }
        }
    }

    pub fn calculate_means_and_stds(&self) -> (Vec<f64>, Vec<f64>) {
        let mut means = Vec::new();
        let mut stds = Vec::new();
        for r_type in RelationshipType::iter() {
            let (sum, sum_sq, count) = self.calculate_sum_sum_sq_and_count(&r_type);
            let mean = sum / count;
            let std = (sum_sq / count - mean.powi(2)).sqrt();
            means.push(mean);
            stds.push(std);
        }
        (means, stds)
    }

    pub fn calculate_sum_sum_sq_and_count(&self, r_type: &RelationshipType) -> (f64, f64, f64) {
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        let mut count = 0.0;
        for ticker_data in &self.data {
            for candlestick in &ticker_data.candlesticks {
                sum += candlestick.get_technical(r_type);
                sum_sq += candlestick.get_technical(r_type).powi(2);
                count += 1.0;
            }
        }
        (sum, sum_sq, count)
    }

    pub fn normalize_technicals(&mut self, means: &[f64], stds: &[f64]) {
        for (i, r_type) in RelationshipType::iter().enumerate() {
            if r_type == RelationshipType::PercentageChange {
                continue;
            }
            for ticker_data in &mut self.data {
                for candle in &mut ticker_data.candlesticks {
                    candle.set_techincal(
                        &r_type,
                        (candle.get_technical(&r_type) - means[i]) / stds[i] * stds[0],
                    );
                }
            }
        }
    }

    pub fn calculate_pc_max(&mut self) {
        let mut pc_vec = Vec::new();
        for ticker_data in &self.data {
            for candlestick in &ticker_data.candlesticks {
                pc_vec.push(candlestick.pc);
            }
        }
        pc_vec.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let pc_max_index = (pc_vec.len() as f64 * 0.05).round() as usize;
        let mut pc_max_sum = 0.0;
        for i in 0..pc_max_index {
            pc_max_sum += pc_vec[i];
        }
        self.pc_max = pc_max_sum / pc_max_index as f64;
    }

    pub fn get_tickers(&self) -> Vec<String> {
        self.data.iter().map(|data| data.ticker.clone()).collect()
    }
}

async fn load_ticker_data(
    ticker: &str,
    start_time: i64,
    config: &Config,
    periods: usize,
) -> (String, Vec<Candlestick>) {
    let market: FuturesMarket = Binance::new(None, None);
    let end_time = start_time
        + (config.get_interval_minutes().unwrap_or_else(|_| 15) * (periods + 1000)) as i64 * 60_000;
    let mut candlesticks = Vec::new();
    let mut start_time = start_time as u64;
    while start_time < end_time as u64 {
        let result = market
            .get_klines(ticker, config.interval(), 1000u16, start_time, None)
            .await;
        match result {
            Ok(klines) => match klines {
                KlineSummaries::AllKlineSummaries(klines) => {
                    for kline in klines {
                        candlesticks.push(Candlestick::from_summary(kline));
                    }
                }
            },
            Err(e) => {
                println!("Error loading data for {}: {}", ticker, e);
                break;
            }
        }
        start_time += 60_000_000 * config.get_interval_minutes().unwrap_or_else(|_| 15) as u64;
    }
    (ticker.to_string(), candlesticks)
}

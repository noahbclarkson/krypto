use core::f64;
use std::{error::Error, time::Duration};

use chrono::Utc;
use getset::{Getters, Setters};
use strum::IntoEnumIterator;
use ta::Close;

use crate::{
    config::Config,
    historical_data::{Candlestick, HistoricalData, TechnicalType},
    live_data::LiveData,
    math::change,
    testing::TestData,
};

const DEFAULT_MARGIN: f64 = 3.0;
const DEFAULT_DEPTH: usize = 13;
const DEFAULT_MIN_SCORE: f64 = 0.0275;
const STARTING_CASH: f64 = 1000.0;
const TRADE_SIZE: f64 = 0.1;

const WAIT_WINDOW: i64 = 5000;
const MINUTES_TO_MILLIS: usize = 60 * 1000;

#[derive(Debug, Getters)]
#[getset(get = "pub")]
pub struct Algorithm {
    data: HistoricalData,
    pub settings: AlgorithmSettings,
    relationships: Vec<Relationship>,
}

#[derive(Debug, Getters, Setters)]
#[getset(get = "pub", set = "pub")]
pub struct AlgorithmSettings {
    margin: f64,
    depth: usize,
    min_score: f64,
}

impl Default for AlgorithmSettings {
    fn default() -> Self {
        Self {
            margin: DEFAULT_MARGIN,
            depth: DEFAULT_DEPTH,
            min_score: DEFAULT_MIN_SCORE,
        }
    }
}

#[derive(Debug)]
pub struct Relationship {
    correlation: f64,
    depth: usize,
    r_type: TechnicalType,
    target_index: usize,
    predict_index: usize,
}

impl Algorithm {
    pub fn new(data: HistoricalData) -> Self {
        Self {
            data,
            settings: AlgorithmSettings::default(),
            relationships: Vec::new(),
        }
    }

    pub fn compute_relationships(&mut self) {
        self.relationships.clear();
        for (target_index, target_candles) in self.data.data.iter().enumerate() {
            for (predict_index, predict_candles) in self.data.data.iter().enumerate() {
                self.relationships.extend(self.compute_relationship(
                    target_index,
                    predict_index,
                    target_candles,
                    predict_candles,
                ));
            }
        }
    }

    fn compute_relationship(
        &self,
        target_index: usize,
        predict_index: usize,
        target_candles: &[Candlestick],
        predict_candles: &[Candlestick],
    ) -> Vec<Relationship> {
        let depth = self.settings.depth;
        let tech_count = TechnicalType::iter().count();
        let mut results = vec![Vec::new(); tech_count * depth];
        for i in self.settings.depth + 1..predict_candles.len() - 1 {
            let target = &target_candles[i].get(TechnicalType::PercentageChange);
            for d in 0..depth {
                let predict = &predict_candles[i - d - 1];
                let all = predict.get_all();
                for (j, r) in all.iter().enumerate() {
                    results[d * tech_count + j].push((target * r).tanh());
                }
            }
        }
        let mut correlations = results
            .iter()
            .map(|v| v.iter().sum::<f64>() / v.len() as f64)
            .collect::<Vec<f64>>();
        let mut relationships = Vec::new();
        for d in 1..depth + 1 {
            for r_type in TechnicalType::iter() {
                let correlation = correlations.remove(0);
                relationships.push(Relationship {
                    correlation,
                    depth: d,
                    r_type,
                    target_index,
                    predict_index,
                });
            }
        }
        relationships
    }

    #[inline]
    pub fn predict(&self, current_position: usize) -> (usize, f64) {
        let mut scores = vec![0.0; self.data.indexes.len()];
        for relationship in &self.relationships {
            for d in 0..relationship.depth {
                let predict = &self.data.data[relationship.predict_index][current_position - d];
                let tech = predict.get(relationship.r_type);
                scores[relationship.target_index] += tech * relationship.correlation;
            }
        }
        let mut max = 0.0;
        let mut max_index = 0;
        for (i, score) in scores.iter().enumerate() {
            if *score > max {
                max = *score;
                max_index = i;
            }
        }
        (max_index, max)
    }

    pub fn test(&self, config: &Config) -> TestData {
        let mut test = TestData::new(STARTING_CASH);
        let data_len = self.data.data[0].len();
        let depth = self.settings.depth;
        let threshold = data_len - depth - 2;
        let margin = self.settings.margin;
        let fee = config.fee() * margin;
        let min_score = self.settings.min_score;

        let mut i = depth;
        while i < threshold && test.cash() > &0.0 {
            let (index, score) = self.predict(i);
            if score > min_score {
                let current_price = self.data.get_close(index, i + 1);
                let exit_price = self.data.get_close(index, i + depth + 1);
                let change = change(current_price, exit_price);
                let fee_change = test.cash() * fee;

                test.add_cash(-fee_change);
                test.add_cash(test.cash() * change * margin * 0.01);
                match change {
                    x if x > 0.0 => test.add_correct(),
                    x if x < 0.0 => test.add_incorrect(),
                    _ => (),
                }
                i += depth;
            } else {
                i += 1;
            }
        }
        if test.cash() < &0.0 {
            test.set_cash(0.0);
        }
        test
    }

    pub async fn live_test(
        &mut self,
        config: &Config,
        tickers: &Vec<String>,
    ) -> Result<(), Box<dyn Error>> {
        let mut live_data = LiveData::new(STARTING_CASH, config, true);
        let depth = self.settings.depth;
        let margin = self.settings.margin;
        let fee = config.fee();
        let min_score = self.settings.min_score;
        let predict_pos = 15 + depth - 1;

        loop {
            self.data =
                load_new_data(config.clone(), 15 + self.settings.depth, tickers.clone()).await;
            if live_data.enter_price.is_none() || live_data.index.is_none() {
                self.wait(&config, 1).await;
            } else {
                let ep = live_data.enter_price.unwrap();
                let current_price = self.data.get_close(live_data.index.unwrap(), predict_pos);
                let change = change(ep, current_price);
                live_data
                    .test
                    .add_cash(live_data.test.cash() * change * margin * 0.01);
                match change {
                    x if x > 0.0 => live_data.test.add_correct(),
                    x if x < 0.0 => live_data.test.add_incorrect(),
                    _ => (),
                }
                live_data.write(current_price, change > 0.0);
                live_data.print();
            }
            let (prediction_index, score) = self.predict(predict_pos);
            if score > min_score {
                let cash_change = live_data.test.cash() * margin * fee;
                live_data.test.add_cash(-cash_change);
                live_data.index = Some(prediction_index);
                live_data.enter_price =
                    Some(self.data.data[prediction_index][predict_pos].data.close());
                live_data.print_new_trade(score, self.data.find_ticker(prediction_index));
                live_data.last_score = score;
                tokio::time::sleep(Duration::from_secs(45)).await;
                self.data = load_new_data(
                    live_data.original_config.clone(),
                    *config.periods(),
                    tickers.clone(),
                )
                .await;
                self.compute_relationships();
                self.wait(&config, depth).await;
            } else {
                println!("No trade");
                live_data.enter_price = None;
                live_data.index = None;
            }
        }
    }

    async fn wait(&self, config: &Config, periods: usize) {
        for _ in 0..periods {
            loop {
                let now = Utc::now().timestamp_millis();
                let millis = (config.get_interval_minutes().unwrap() * MINUTES_TO_MILLIS) as i64;
                let next_interval = (now / millis) * millis + millis;
                let wait_time = next_interval - now - WAIT_WINDOW;
                if wait_time > WAIT_WINDOW {
                    tokio::time::sleep(Duration::from_millis(wait_time as u64)).await;
                    break;
                } else {
                    tokio::time::sleep(Duration::from_millis(WAIT_WINDOW as u64 + 1)).await;
                }
            }
        }
    }
}

fn round_to_tick_size(quantity: f64, tick_size: f64) -> f64 {
    // Ensure that price % tick_size == 0
    let mut quantity = quantity;
    while quantity % tick_size != 0.0 {
        let remainder = quantity % tick_size;
        if remainder != 0.0 {
            quantity -= remainder;
            if remainder > tick_size / 2.0 {
                quantity += tick_size;
            }
        }
    }
    quantity
}

async fn load_new_data(mut config: Config, periods: usize, tickers: Vec<String>) -> HistoricalData {
    let mut data = HistoricalData::new(&tickers);
    config.set_periods(periods);
    data.load(&config).await;
    data.calculate_technicals();
    data
}

#[cfg(test)]
mod tests {

    use crate::historical_data::TechnicalData;

    use super::*;
    use rand::Rng;
    use ta::DataItem;
    use test::Bencher;

    #[bench]
    fn bench_predict(b: &mut Bencher) {
        let algorithm = get_algorithm();
        b.iter(|| {
            algorithm.predict(algorithm.settings.depth + 1);
        });
    }

    #[bench]
    fn bench_test(b: &mut Bencher) {
        let algorithm = get_algorithm();
        let config = Config::default();
        b.iter(|| {
            algorithm.test(&config);
        });
    }

    fn get_algorithm() -> Algorithm {
        let mut data = HistoricalData::new(&vec!["BTCUSDT".to_string()]);
        let mut rand = rand::thread_rng();
        let candle_data = DataItem::builder()
            .open(rand.gen_range(1.0..2.0))
            .high(rand.gen_range(2.0..3.0))
            .low(rand.gen_range(0.0..1.0))
            .close(rand.gen_range(1.0..2.0))
            .volume(rand.gen_range(0.0..10000.0))
            .build()
            .unwrap();
        let candle = Candlestick {
            data: candle_data,
            technicals: TechnicalData::new(),
            close_time: 0,
        };
        let candles = vec![candle; 1000];
        data.data = vec![candles; 1];
        data.calculate_technicals();
        let mut algorithm = Algorithm::new(data);
        algorithm.compute_relationships();
        algorithm
    }
}

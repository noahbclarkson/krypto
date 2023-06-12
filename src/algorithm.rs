use std::{error::Error, time::Duration};

use chrono::Utc;
use getset::{Getters, Setters};
use strum::IntoEnumIterator;
use ta::Close;

use crate::{
    config::Config,
    historical_data::{Candlestick, HistoricalData, TechnicalType},
    math::{change, format_number},
    testing::TestData,
};

const DEFAULT_MARGIN: f64 = 10.0;
const DEFAULT_DEPTH: usize = 15;
const DEFAULT_MIN_SCORE: f64 = 0.007;
const STARTING_CASH: f64 = 1000.0;

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
        for r_type in TechnicalType::iter() {
            for d in 1..depth + 1 {
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
    pub fn predict(&self, current_position: usize) -> (usize, Vec<f64>) {
        let mut scores = vec![vec![0.0; self.settings.depth]; self.data.indexes.len()];
        for relationship in &self.relationships {
            for d in 0..relationship.depth {
                let predict = &self.data.data[relationship.predict_index][current_position - d];
                let tech = predict.get(relationship.r_type);
                scores[relationship.target_index][relationship.depth - d - 1] +=
                    tech * relationship.correlation;
            }
        }
        // Return the array and index that has the highest absolute average
        let mut best = 0;
        let mut best_score = 0.0;
        for (i, score) in scores.iter().enumerate() {
            let avg = score.iter().sum::<f64>() / score.len() as f64;
            if avg.abs() > best_score {
                best = i;
                best_score = avg.abs();
            }
        }
        (best, scores[best].clone())
    }

    pub fn test(&self, config: &Config) -> TestData {
        let mut test = TestData::new(STARTING_CASH);
        let data_len = self.data.data[0].len();
        let depth = self.settings.depth;
        let threshold = data_len - depth - 2;
        let margin = self.settings.margin;
        let fee = config.fee();
        let min_score = self.settings.min_score;

        let mut i = depth;
        while i < threshold {
            let (prediction_index, scores) = self.predict(i);
            let score = scores.iter().sum::<f64>() / scores.len() as f64;
            if score.abs() >= min_score {
                let current_price = self.data.data[prediction_index][i + 1].data.close();
                let exit_price = self.data.data[prediction_index][i + depth + 1].data.close();
                let change = change(current_price, exit_price);
                let cash_change = test.cash() * margin * fee;

                test.add_cash(-cash_change);
                let correct = change * score;
                if correct > 0.0 {
                    test.add_cash(test.cash() * change.abs() * margin * 0.01);
                    test.add_correct();
                } else if correct < 0.0 {
                    test.add_cash(-test.cash() * change.abs() * margin * 0.01);
                    test.add_incorrect();
                }
                i += depth;
            } else {
                i += 1;
            }
        }
        test
    }

    pub async fn live_test(
        &mut self,
        config: &Config,
        tickers: &Vec<String>,
    ) -> Result<(), Box<dyn Error>> {
        let mut test = TestData::new(1000.0);
        let mut csv_file = csv::Writer::from_path("live_test.csv")?;
        csv_file.write_record(&[
            "Cash ($)",
            "Accuracy",
            "Trade Direction",
            "Correct/Incorrect",
            "Current Price",
            "Enter Price",
            "Score",
        ])?;
        let original_config = config.clone();
        let depth = self.settings.depth;
        let margin = self.settings.margin;
        let fee = config.fee();
        let min_score = self.settings.min_score;
        let predict_pos = 15 + depth - 1;
        let mut enter_price: Option<f64> = None;
        let mut index: Option<usize> = None;
        let mut direction = 0.0;
        let mut last_score = 0.0;

        loop {
            let mut data = HistoricalData::new(tickers);
            let mut config = config.clone();
            config.set_periods(15 + self.settings.depth);
            data.load(&config).await;
            data.calculate_technicals();
            self.data = data;
            if enter_price.is_none() || index.is_none() || direction == 0.0 {
                self.wait(&config, 1).await;
            } else {
                let ep = enter_price.unwrap();
                let change = change(ep, self.data.data[index.unwrap()][predict_pos].data.close());
                let correct = change * direction;
                if correct > 0.0 {
                    test.add_cash(test.cash() * change.abs() * margin * 0.01);
                    test.add_correct();
                } else if correct < 0.0 {
                    test.add_cash(-test.cash() * change.abs() * margin * 0.01);
                    test.add_incorrect();
                }
                csv_file.write_record(&[
                    test.cash().to_string(),
                    test.get_accuracy().to_string(),
                    direction.to_string(),
                    correct.signum().to_string(),
                    self.data.data[index.unwrap()][predict_pos]
                        .data
                        .close()
                        .to_string(),
                    ep.to_string(),
                    last_score.to_string(),
                ])?;
                csv_file.flush()?;
                println!(
                    "Cash: ${}, Accuracy: {:.2}%, Direction: {}, Correct/Incorrect: {}/{}, Score {:.5}",
                    format_number(*test.cash()),
                    test.get_accuracy() * 100.0,
                    match direction as i32 {
                        1 => "Long",
                        -1 => "Short",
                        _ => "None",
                    },
                    test.correct(),
                    test.incorrect(),
                    last_score
                );
            }
            let (prediction_index, scores) = self.predict(predict_pos);
            let score = scores.iter().sum::<f64>() / scores.len() as f64;
            if score.abs() >= min_score {
                let cash_change = test.cash() * margin * fee;
                test.add_cash(-cash_change);
                index = Some(prediction_index);
                enter_price = Some(self.data.data[prediction_index][predict_pos].data.close());
                direction = score.signum();
                if score > 0.0 {
                    println!(
                        "Enter Long: for {} at ${:.5}",
                        self.data.find_ticker(prediction_index),
                        enter_price.unwrap()
                    );
                } else {
                    println!(
                        "Enter Short: for {} at ${:.5}",
                        self.data.find_ticker(prediction_index),
                        enter_price.unwrap()
                    );
                }
                last_score = score;
                tokio::time::sleep(Duration::from_secs(45)).await;
                let mut new_data = HistoricalData::new(tickers);
                let config = original_config.clone();
                new_data.load(&config).await;
                new_data.calculate_technicals();
                self.data = new_data;
                self.compute_relationships();
                self.wait(&config, depth).await;
            } else {
                println!("No trade");
                enter_price = None;
                index = None;
                direction = 0.0;
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

#[cfg(test)]
mod tests {

    use crate::historical_data::{CandleData, TechnicalData};

    use super::*;
    use ta::DataItem;
    use test::Bencher;

    #[bench]
    fn bench_predict(b: &mut Bencher) {
        let mut data = HistoricalData::new(&vec!["BTCUSDT".to_string()]);
        let mut config = Config::default();
        data.data.push(vec![Candlestick { data: DataItem::builder().close(1.0).high(1.1).open(0.9).low(0.8).build().unwrap(),
            technicals: TechnicalData::default(),
            close_time: 0,
        }]);
    }
}

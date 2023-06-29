use std::{error::Error, sync::Arc, time::Duration};

use chrono::{DateTime, Local, Utc};
use getset::{Getters, MutGetters, Setters};
use rand::Rng;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use crate::{
    candlestick::{Candlestick, TechnicalType},
    config::Config,
    historical_data::HistoricalData,
    math::change,
    testing::TestData,
};

// The margin to use when buying in tests (3x for cross margin)
const MARGIN: f32 = 0.03;
// The default name of the algorithm file (must be a json file)
const DEFAULT_FILE_NAME: &str = "algorithm.json";
// The default starting cash for tests
const STARTING_CASH: f32 = 1000.0;

#[derive(Debug, Getters, Setters, Clone, Serialize, Deserialize)]
#[getset(get = "pub", set = "pub")]
pub struct AlgorithmSettings {
    // The maximum depth of the algorithm (the number of candles to look back and generate relationships for)
    depth: usize,
    // The fee to use when buying or selling in tests (decimal value, e.g. 0.01 = 1%)
    fee: f32,
    // The minimum score to use when choosing whether to make a trade (this value is usually found through parameter optimization)
    min_score: Option<f32>,
}

impl AlgorithmSettings {
    pub fn default(config: &Config) -> Self {
        Self {
            depth: *config.depth(),
            fee: *config.fee(),
            min_score: *config.min_score(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Getters, Serialize, Deserialize)]
#[getset(get = "pub")]
pub struct Relationship {
    // The correlation between the percentage change of the target at t and the technical at t-depth
    correlation: f32,
    // The depth of the relationship (the number of candles between the target and the technical)
    depth: usize,
    // The type of the technical
    r_type: usize,
    // The index of the target ticker that the relationship is for
    target_index: usize,
    // The index of the technical ticker that the relationship is for
    predict_index: usize,
}

#[derive(Debug, Clone, Getters, Setters, MutGetters, Serialize, Deserialize)]
pub struct Algorithm {
    #[getset(get = "pub", get_mut = "pub", set = "pub")]
    // The settings of the algorithm
    settings: AlgorithmSettings,
    #[getset(get = "pub")]
    // The relationships between the target and the technicals
    relationships: Vec<Relationship>,
    #[getset(get)]
    // The indexes of the candles to use for testing
    test_indexes: Vec<usize>,
    #[getset(get = "pub", set = "pub")]
    #[serde(skip)]
    // The test data for the algorithm (for access by the GUI)
    test_data: Option<TestData>,
}

impl Algorithm {
    /// Creates a new Algorithm with the given config.
    ///
    /// # Arguments
    ///
    /// * `config` - The config to use for the algorithm
    ///
    /// # Returns
    ///
    /// * The new algorithm
    ///
    /// # Examples
    ///
    /// ```
    /// use krypto::algorithm::Algorithm;
    /// use krypto::config::Config;
    ///
    /// let config = Config::get_test_config();
    /// let algorithm = Algorithm::new(&config);
    /// ```
    pub fn new(config: &Config) -> Self {
        let settings = AlgorithmSettings::default(config);
        let relationships = Vec::new();
        Self {
            settings,
            relationships,
            test_indexes: Self::load_random_test_indexes(config),
            test_data: None,
        }
    }

    fn load_random_test_indexes(config: &Config) -> Vec<usize> {
        if config.test_ratio().is_none()
            || config.test_ratio().unwrap() <= 0.0
            || config.test_ratio().unwrap() >= 1.0
        {
            let mut indexes = Vec::new();
            for i in 15..config.periods() - 15 {
                indexes.push(i);
            }
            return indexes;
        }
        let mut indexes = Vec::new();
        let rand = &mut rand::thread_rng();
        let test_ratio = config.test_ratio().unwrap();
        for i in 15..config.periods() - 15 {
            if rand.gen::<f32>() < test_ratio {
                indexes.push(i);
            }
        }
        indexes
    }

    pub async fn compute_relationships(&mut self, candles: &Vec<Vec<Candlestick>>) {
        self.relationships.clear();
        for (target_index, target_candles) in candles.iter().enumerate() {
            let target_candles = Arc::new(target_candles);
            let tasks = candles
                .iter()
                .enumerate()
                .map(|(predict_index, predict_candles)| {
                    Self::compute_relationship(
                        target_index,
                        predict_index,
                        &target_candles,
                        predict_candles,
                        self.settings.depth,
                    )
                });
            futures::future::join_all(tasks)
                .await
                .into_iter()
                .for_each(|mut relationships| self.relationships.append(&mut relationships));
        }
    }

    async fn compute_relationship(
        target_index: usize,
        predict_index: usize,
        target_candles: &Arc<&Vec<Candlestick>>,
        predict_candles: &[Candlestick],
        depth: usize,
    ) -> Vec<Relationship> {
        let tech_count = TechnicalType::iter().count();
        let mut results = vec![Vec::new(); tech_count * depth];
        let pc_index = TechnicalType::PercentageChange as usize;
        for i in depth + 1..predict_candles.len() - 1 {
            let target = &target_candles[i + 1].technicals()[pc_index];
            for d in 0..depth {
                for (j, technical) in target_candles[i - d].technicals().iter().enumerate() {
                    results[d * tech_count + j].push((technical * target).tanh());
                }
            }
        }
        let mut correlations = results
            .iter()
            .map(|v| v.iter().sum::<f32>() / v.len() as f32)
            .collect::<Vec<f32>>();
        let mut relationships = Vec::new();
        for d in 1..depth + 1 {
            for j in 0..tech_count {
                let correlation = correlations.remove(0);
                relationships.push(Relationship {
                    correlation,
                    depth: d,
                    r_type: j,
                    target_index,
                    predict_index,
                });
            }
        }
        relationships
    }

    #[inline(always)]
    pub fn predict(
        &self,
        current_position: usize,
        candles: &Vec<Vec<Candlestick>>,
    ) -> (usize, f32) {
        let mut scores = vec![0.0; candles.len()];
        for relationship in self.relationships() {
            for d in 0..relationship.depth {
                let predict = candles[relationship.predict_index][current_position - d]
                    .technicals()[relationship.r_type];
                scores[relationship.target_index] += predict * relationship.correlation;
            }
        }
        let mut max_index = 0;
        let mut max = scores[0];
        for i in 1..scores.len() {
            if scores[i] > max {
                max_index = i;
                max = scores[i];
            }
        }
        (max_index, max)
    }

    pub fn test(&mut self, candles: &Vec<Vec<Candlestick>>) -> TestData {
        let mut test = TestData::new(STARTING_CASH, self.test_indexes().clone().len());
        let settings = self.settings().clone();
        let min_score = settings.min_score().unwrap_or(0.0);
        let depth = settings.depth();
        let fee = settings.fee();

        for i in self.test_indexes().clone() {
            let (index, score) = self.predict(i, candles);
            if score > min_score {
                let current_price = candles[index][i].candle().close();
                let exit_price = candles[index][i + depth].candle().close();
                let change = change(current_price, exit_price);
                let fee_change = test.cash() * fee * MARGIN * 100.0;

                test.add_cash(-fee_change);
                test.add_cash(test.cash() * MARGIN * change);

                match change {
                    x if x > 0.0 => test.add_correct(),
                    x if x < 0.0 => test.add_incorrect(),
                    _ => (),
                }

                if *test.cash() <= 0.0 {
                    test.set_cash(0.0);
                    break;
                }

                self.set_test_data(Some(test.clone()));
            }
        }
        test
    }

    pub async fn live_test(
        &mut self,
        config: &Config,
        tickers: &Vec<String>,
    ) -> Result<TestData, Box<dyn Error>> {
        let original_config = config.clone();
        let mut test = TestData::new(STARTING_CASH, self.test_indexes().clone().len());
        let settings = self.settings().clone();
        let min_score = settings.min_score().unwrap_or(0.0);
        let depth = settings.depth();
        let fee = settings.fee();
        let data_length = 15 + depth;

        let mut csv = csv::Writer::from_path("live_test.csv")?;
        csv.write_record(&[
            "Enter Date",
            "Enter Time",
            "Exit Time",
            "Index",
            "Ticker",
            "Change (%)",
            "Cash ($)",
            "Accuracy (%)",
        ])?;

        let mut enter_price: Option<f32> = None;
        let mut enter_time: Option<DateTime<Local>> = None;
        let mut enter_index: Option<usize> = None;

        wait(&config, 1).await;

        loop {
            let candles = load_new_data(&config, data_length, &tickers).await;
            let candles = match candles {
                Ok(candles) => candles,
                Err(e) => {
                    println!("Error: {}", e);
                    println!("Retrying...");
                    match load_new_data(&config, data_length, &tickers).await {
                        Ok(candles) => candles,
                        Err(e) => {
                            println!("Error: {}", e);
                            println!("Waiting and Retrying...");
                            wait(&config, 1).await;
                            continue;
                        }
                    }
                }
            };

            if enter_index.is_some() && enter_price.is_some() && enter_time.is_some() {
                let ep = enter_price.unwrap();
                let ei = enter_index.unwrap();
                let et = enter_time.unwrap();
                let candle = candles[ei][data_length - 1].candle();
                let current_price = candle.close();
                let change = change(&ep, current_price);
                let fee_change = test.cash() * fee * MARGIN * 100.0;

                test.add_cash(-fee_change);
                test.add_cash(test.cash() * MARGIN * change);

                match change {
                    x if x > 0.0 => test.add_correct(),
                    x if x < 0.0 => test.add_incorrect(),
                    _ => (),
                }

                if *test.cash() <= 0.0 {
                    test.set_cash(0.0);
                    break;
                }

                self.set_test_data(Some(test.clone()));

                println!(
                    "{}: {:.5} -> {:.5} ({:.3}%)",
                    tickers[ei], ep, current_price, change
                );
                println!("{}", test);

                let enter_date = et.format("%Y-%m-%d").to_string();
                let enter_time = et.format("%H:%M:%S").to_string();
                let exit_time = Local::now().format("%H:%M:%S").to_string();

                let csv_result_1 = csv.write_record(&[
                    &enter_date,
                    &enter_time,
                    &exit_time,
                    &ei.to_string(),
                    &tickers[ei],
                    &change.to_string(),
                    &test.cash().to_string(),
                    &(test.get_accuracy() * 100.0).to_string(),
                ]);
                let csv_result_2 = csv.flush();

                match csv_result_1 {
                    Ok(_) => (),
                    Err(e) => {
                        println!("Error: {}", e);
                        println!("This is not a fatal error, but the csv file may be corrupted or opened.");
                    }
                }

                match csv_result_2 {
                    Ok(_) => (),
                    Err(e) => {
                        println!("Error: {}", e);
                        println!("This is not a fatal error, but the csv file may be corrupted or opened.");
                    }
                }
            }
            let (index, score) = self.predict(data_length - 1, &candles.clone());
            if score > min_score {
                let ep = *candles[index][data_length - 1].candle().close();
                enter_price = Some(ep);
                enter_index = Some(index);
                enter_time = Some(Local::now());
                println!(
                    "Entered long on {} at ${:.5} with score {:.5} ",
                    tickers[index], ep, score
                );

                wait(&config, depth - 1).await;
                let new_candles =
                    load_new_data(&original_config, *original_config.periods(), &tickers).await;
                match new_candles {
                    Ok(new_candles) => {
                        self.compute_relationships(&new_candles).await;
                        println!("Relationships updated");
                    }
                    Err(e) => {
                        println!("Error: {}", e);
                    }
                };
                wait(&config, 1).await;
            } else {
                enter_price = None;
                enter_index = None;
                println!("No trade (score: {:.5})", score);
                wait(&config, 1).await;
            }
        }
        Ok(test)
    }
    pub fn serialize_to_json(&self) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self)?;
        std::fs::write(DEFAULT_FILE_NAME, json)?;
        Ok(())
    }

    pub fn deserialize_from_json() -> Result<Self, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(DEFAULT_FILE_NAME)?;
        let algorithm = serde_json::from_str(&json)?;
        Ok(algorithm)
    }
}

const WAIT_WINDOW: i64 = 4100;
const MINUTES_TO_MILLIS: i64 = 60 * 1000;

async fn wait(config: &Config, periods: usize) {
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

async fn load_new_data(
    config: &Config,
    periods: usize,
    tickers: &Vec<String>,
) -> Result<Vec<Vec<Candlestick>>, Box<dyn std::error::Error>> {
    let mut data = HistoricalData::new(&tickers);
    let mut config = config.clone();
    let config = config.set_periods(periods);
    data.load(&config, None).await?;
    if data.calculate_candlestick_technicals().is_err() {
        return Err(Box::new(AlgorithmError::new(
            "Failed to calculate candlestick technicals",
        )));
    }
    data.normalize_technicals();
    Ok(data.candles().clone())
}

#[derive(Debug)]
pub struct AlgorithmError {
    message: String,
}

impl AlgorithmError {
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

impl std::fmt::Display for AlgorithmError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "AlgorithmError: {}", self.message)
    }
}

impl Error for AlgorithmError {}

#[cfg(test)]
pub mod tests {

    use crate::historical_data::{tests::get_default_tickers, HistoricalData};

    use super::*;

    #[tokio::test]
    async fn test_compute_relationships() {
        let config = Config::get_test_config();
        let tickers = get_default_tickers();
        let mut data = HistoricalData::new(&tickers);
        data.load(&config, None).await.unwrap();
        data.calculate_candlestick_technicals().unwrap();
        data.normalize_technicals();
        let candles = data.candles();
        let mut algorithm = Algorithm::new(&config);
        algorithm.compute_relationships(candles).await;
        let relationships = algorithm.relationships();
        assert_eq!(
            relationships.len(),
            tickers.len().pow(2) * TechnicalType::iter().count() * config.depth()
        );
    }

    #[tokio::test]
    async fn test_serialize_to_json() {
        let config = Config::get_test_config();
        let tickers = get_default_tickers();
        let mut data = HistoricalData::new(&tickers);
        data.load(&config, None).await.unwrap();
        data.calculate_candlestick_technicals().unwrap();
        data.normalize_technicals();
        let candles = data.candles();
        let mut algorithm = Algorithm::new(&config);
        algorithm.compute_relationships(candles).await;
        algorithm.serialize_to_json().unwrap();
        std::fs::remove_file(DEFAULT_FILE_NAME).unwrap();
    }
}

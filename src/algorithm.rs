use std::{thread::sleep, time::Duration};

use chrono::Utc;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

use crate::{
    config::Config,
    historical_data::{Candlestick, HistoricalData, TickerData},
    math::change,
    testing::TestData,
};

#[derive(Debug, Clone)]
pub struct Algorithm {
    // The historical data attached to the algorithm
    pub data: HistoricalData,
    // The relationships between the tickers
    pub relationships: Vec<Relationship>,
}

#[derive(Debug, Clone)]
pub struct Relationship {
    // The ticker that is being used to make the prediction
    predictor: String,
    // The ticker that is being predicted
    target: String,
    // The correlation between the predictor and the target
    correlation: f64,
    // The type of relationship
    relationship_type: RelationshipType,
    // The weight of the relationship
    weight: f64,
}

#[derive(Debug, Clone, EnumIter, PartialEq)]
pub enum RelationshipType {
    PercentageChange,
    CandlestickRatio,
    StochasticOscillator,
    RelativeStrengthIndex,
    CommodityChannelIndex,
    VolumeChange,
    StandardDeviation,
}

impl Algorithm {
    pub fn new(data: HistoricalData) -> Algorithm {
        Algorithm {
            data,
            relationships: Vec::new(),
        }
    }

    pub fn calculate_relationships(&mut self) {
        for ticker_data in self.data.data.clone() {
            // Target ticker
            let ticker = &ticker_data.ticker;
            let candlesticks = &ticker_data.candlesticks;
            for other_ticker_data in self.data.data.clone() {
                // Calculate relationship between ticker (target) and other_ticker (predictor)
                self.calculate_relationship(ticker, candlesticks, other_ticker_data)
            }
        }
    }

    fn calculate_relationship(
        &mut self,
        ticker: &String,
        candlesticks: &[Candlestick],
        other_ticker_data: TickerData,
    ) {
        // Loop throuh enum and create a Vec for each relationship type
        let mut results = Vec::new();
        for _ in RelationshipType::iter() {
            results.push(Vec::new());
        }
        let other_ticker = &other_ticker_data.ticker;
        let other_candlesticks = &other_ticker_data.candlesticks;
        for i in 1..candlesticks.len() {
            let candlestick = &candlesticks[i];
            let other_candlestick = &other_candlesticks[i - 1];
            let target = candlestick.pc;
            for (i, r_type) in RelationshipType::iter().enumerate() {
                let n = other_candlestick.get_technical(&r_type);
                results[i].push((target * n).tanh());
            }
        }
        let correlations = results
            .iter()
            .map(|x| x.iter().sum::<f64>() / x.len() as f64)
            .collect::<Vec<_>>();
        for (i, r_type) in RelationshipType::iter().enumerate() {
            let correlation = correlations[i];
            let relationship = Relationship {
                predictor: other_ticker.clone(),
                target: ticker.clone(),
                correlation,
                relationship_type: r_type,
                weight: 1.0,
            };
            self.relationships.push(relationship);
        }
    }

    pub fn predict(&self, ticker: &str, target_pos: usize) -> f64 {
        let mut score = 0.0;
        let predict_pos = target_pos - 1;
        for ticker_data in &self.data.data {
            let other_candlesticks = &ticker_data.candlesticks;
            for relationship in &self.relationships {
                if relationship.target == ticker && relationship.predictor == ticker_data.ticker {
                    score += other_candlesticks[predict_pos]
                        .get_technical(&relationship.relationship_type)
                        * relationship.correlation
                        * relationship.weight;
                }
            }
        }
        score
    }

    pub fn test(&self, ticker: &str, config: &Config) -> TestData {
        let mut test = TestData::new(1000.0);
        let target_data = self.data.data.iter().find(|x| x.ticker == ticker).unwrap();
        let target_candles = &target_data.candlesticks;
        let mut last_trade_direction = 0.0;
        let fee = config.fee();
        let margin = config.margin() / 100.0;
        // Remove the first target candle because it is used to calculate the first prediction
        for (i, target) in target_candles.iter().skip(1).enumerate() {
            let prediction = self.predict(&ticker, i + 1);
            let actual = &target.pc;
            let ps = prediction.signum();
            if last_trade_direction != ps {
                test.add_cash(-test.cash() * fee * margin);
                last_trade_direction = ps;
            }
            if ps * actual > 0.0 {
                test.add_cash(test.cash() * actual.abs() * margin);
                test.add_correct();
            } else {
                test.add_cash(-test.cash() * actual.abs() * margin);
                test.add_incorrect();
            }
        }
        test
    }

    pub async fn live_test(&mut self, ticker: &str, config: &Config) {
        // This function gets data for the ticker and then runs the algorithm on it
        let mut last_trade_direction = 0.0;
        let mut test = TestData::new(1000.0);
        let tickers = self.data.get_tickers();
        let index = self.data.index_map.get(ticker).unwrap();
        let mut enter_price = None;
        let margin = config.margin() / 100.0;
        loop {
            self.wait(&config);
            let mut data = HistoricalData::new(&tickers);
            let mut config = config.clone();
            config.set_periods(15);
            data.load_data(tickers.clone(), &config).await;
            data.calculate_technicals();
            let mut algorithm = self.clone();
            algorithm.data = data.clone();
            let prediction = algorithm.predict(ticker, 15);
            if last_trade_direction != prediction.signum() {
                test.add_cash(-test.cash() * config.fee() * margin);
                let candle = data.data[*index].candlesticks.last().unwrap();
                let current_price = candle.close;
                let change = change(enter_price.unwrap_or(current_price), current_price);
                if last_trade_direction != 0.0 {
                    if last_trade_direction * change <= 0.0 {
                        test.add_cash(-test.cash() * change.abs() * margin);
                        test.add_incorrect();
                    } else {
                        test.add_cash(test.cash() * change.abs() * margin);
                        test.add_correct();
                    }
                    println!("{}", test);
                }
                enter_price = Some(current_price);
                if prediction > 0.0 {
                    println!("Buy {}", ticker);
                }
                if prediction < 0.0 {
                    println!("Sell {}", ticker);
                }
            } else {
                println!("Hold {}", ticker);
            }
            last_trade_direction = prediction.signum();
        }
    }

    fn wait(&self, config: &Config) {
        loop {
            let now = Utc::now().timestamp_millis();
            let millis = (config.get_interval_minutes().unwrap_or_else(|_| 15) * 60 * 1000) as i64;
            let next_interval = (now / millis) * millis + millis;
            let wait_time = next_interval - now - 2500;
            if wait_time >= 2501 {
                sleep(Duration::from_millis(wait_time as u64));
                break;
            } else {
                sleep(Duration::from_millis(2502));
            }
        }
    }

    pub fn optimize_weights(&mut self, ticker: &str, config: &Config, iterations: usize) {
        let initial_test = self.test(ticker, config);
        let mut highest_cash = *initial_test.cash();
        println!("Initial test:");
        println!("{}", initial_test);
        let mut best_relationships = self.relationships.clone();
        for i in 0..iterations {
            let mut initial_relationships = self.relationships.clone();
            // Find all the relationships which have the target set to the ticker and randomize their weights
            for relationship in &mut initial_relationships {
                if relationship.target == ticker {
                    relationship.weight = rand::random::<f64>();
                }
            }
            // Test the algorithm with the new weights
            self.relationships = initial_relationships;
            let test = self.test(ticker, config);
            if *test.cash() > highest_cash {
                highest_cash = *test.cash();
                best_relationships = self.relationships.clone();
                println!("New Highest cash! (Iteration: {}):", i);
                println!("{}", test);
            }
        }
        self.relationships = best_relationships;
    }
}

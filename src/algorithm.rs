use std::{error::Error, time::Duration, collections::HashMap};

use chrono::Utc;
use getset::{Getters, Setters};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

use crate::{
    config::Config,
    historical_data::{Candlestick, HistoricalData, TickerData},
    math::change,
    testing::TestData,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Algorithm {
    // The historical data attached to the algorithm
    pub data: HistoricalData,
    // The ticker that has been chosen to trade
    pub ticker: Option<String>,
    // The margin that has been chosen to be traded with
    pub margin: Option<f64>,
    // The map of relationship pairs to relationships
    #[serde(skip)]
    pub relationships: HashMap<TickerPair, Relationship>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Getters, Setters)]
#[getset(get = "pub")]
pub struct Relationship {
    correlation: f64,
    weight: f64,
}

#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct TickerPair {
    target: String,
    predictor: String,
    relationship_type: RelationshipType,
}

#[derive(Debug, Clone, EnumIter, PartialEq, Serialize, Deserialize, Hash, Eq)]
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
            relationships: HashMap::new(),
            ticker: None,
            margin: None,
        }
    }

    pub fn calculate_relationships(&mut self) {
        let data = self.data.data.clone();
        for ticker_data in &data {
            // Target ticker
            let ticker = &ticker_data.ticker;
            let candlesticks = &ticker_data.candlesticks;
            for other_ticker_data in &data {
                // Calculate relationship between ticker (target) and other_ticker (predictor)
                self.calculate_relationship(ticker, candlesticks, other_ticker_data);
            }
        }
    }

    fn calculate_relationship(
        &mut self,
        ticker: &String,
        candlesticks: &[Candlestick],
        other_ticker_data: &TickerData,
    ) {
        let mut results = vec![Vec::new(); RelationshipType::iter().count()];
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
                correlation,
                weight: 1.0,
            };
            let pair = TickerPair {
                predictor: other_ticker.clone(),
                target: ticker.clone(),
                relationship_type: r_type,
            };
            self.relationships.insert(pair, relationship);
        }
    }

    pub fn predict(&self, target_pos: usize, ticker_pairs: &[&TickerPair]) -> f64 {
        let predict_pos = target_pos - 1;
        self.data.data.iter().flat_map(|ticker_data| {
            let other_candlestick = &ticker_data.candlesticks[predict_pos];
            ticker_pairs.iter().filter_map(|ticker_pair| {
                self.relationships.get(ticker_pair).map(|relationship| {
                    other_candlestick.get_technical(&ticker_pair.relationship_type) 
                    * relationship.correlation 
                    * relationship.weight
                })
            })
        }).sum()
    }

    pub fn test(&self, ticker: &str, config: &Config, print_cash: bool) -> TestData {
        let mut test = TestData::new(1000.0);
        let target_data = self.data.data.iter().find(|x| x.ticker == ticker).unwrap();
        let target_candles = &target_data.candlesticks;
        let mut last_trade_direction = 0.0;
        let fee = config.fee();
        let margin = self.margin.unwrap_or(*config.margin()) / 100.0;
        // We need to get a Vec of possible ticker pairs (ones with the target ticker as the target)
        let algo = self.clone();
        let ticker_pairs = algo.get_ticker_pairs(ticker).clone();
        for (i, target) in target_candles.iter().skip(1).enumerate() {
            let prediction = self.predict(i + 1, &ticker_pairs);
            let actual = &target.pc;
            let ps = prediction.signum();
            if last_trade_direction != ps {
                test.add_cash(-test.cash() * fee * margin);
                last_trade_direction = ps;
            }
            if ps * actual >= 0.0 {
                test.add_cash(test.cash() * actual.abs() * margin);
                test.add_correct();
            } else {
                test.add_cash(-test.cash() * actual.abs() * margin);
                test.add_incorrect();
            }
            if print_cash {
                println!("{}", test.cash());
            }
        }
        test
    }

    pub async fn live_test(
        &mut self,
        ticker: &str,
        config: &Config,
        tickers: &Vec<String>,
    ) -> Result<(), Box<dyn Error>> {
        let mut last_trade_direction = None;
        let mut test = TestData::new(1000.0);
        let index = self.data.index_map.get(ticker).ok_or_else(|| {
            format!(
                "Ticker {} not found in index map: {:?}",
                ticker, self.data.index_map
            )
        })?;
        let margin = self.margin.unwrap_or(*config.margin()) / 100.0;
        let mut csv_file = csv::Writer::from_path("live_test.csv")?;
        csv_file.write_record(&["Time", "Price", "Cash", "Accuracy"])?;
        let mut enter_price = None;
        let ticker_pairs = self.get_ticker_pairs(ticker);
        loop {
            self.wait(&config).await;
            let mut data = HistoricalData::new(tickers);
            let mut config = config.clone();
            config.set_periods(15);
            data.load_data(tickers.clone(), &config).await;
            data.calculate_technicals();
            let mut algorithm = self.clone();
            algorithm.data = data.clone();
            let prediction = algorithm.predict(data.data[*index].candlesticks.len(), &ticker_pairs);
            let prediction_sign = prediction.signum();
            let candle = data.data[*index]
                .candlesticks
                .last()
                .ok_or("No last candle available")?;
            let current_price = candle.close;

            if last_trade_direction.is_none() || last_trade_direction.unwrap() != prediction_sign {
                // Apply the fee
                test.add_cash(-test.cash() * config.fee() * margin);

                // Calculate the price change since the last trade direction change, if there was one
                if let Some(ep) = enter_price {
                    let change = change(ep, current_price);

                    if last_trade_direction.unwrap() * change >= 0.0 {
                        test.add_cash(test.cash() * change.abs() * margin);
                        test.add_correct();
                    } else {
                        test.add_cash(-test.cash() * change.abs() * margin);
                        test.add_incorrect();
                    }
                }

                // Track the entry price
                enter_price = Some(current_price);

                if prediction > 0.0 {
                    println!("Buy {}", ticker);
                } else if prediction < 0.0 {
                    println!("Sell {}", ticker);
                }

                // Update the last trade direction
                last_trade_direction = Some(prediction_sign);
            } else {
                println!("Hold {}", ticker);
            }

            csv_file.write_record(&[
                &Utc::now().to_string(),
                &current_price.to_string(),
                &test.cash().to_string(),
                &test.get_accuracy().to_string(),
            ])?;

            csv_file.flush()?;
        }
    }

    fn get_ticker_pairs(&self, ticker: &str) -> Vec<&TickerPair> {
        let mut ticker_pairs = Vec::new();
        for (pair, _) in &self.relationships {
            if pair.target == ticker {
                ticker_pairs.push(pair);
            }
        }
        ticker_pairs
    }

    async fn wait(&self, config: &Config) {
        loop {
            let now = Utc::now().timestamp_millis();
            let millis = (config.get_interval_minutes().unwrap_or_else(|_| 15) * 60 * 1000) as i64;
            let next_interval = (now / millis) * millis + millis;
            let wait_time = next_interval - now - 5000;
            if wait_time > 5000 {
                tokio::time::sleep(Duration::from_millis(wait_time as u64)).await;
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(5001)).await;
            }
        }
    }

    pub fn optimize_weights(&mut self, config: &Config, iterations: usize) {
        let ticker = &self.clone().ticker.unwrap();
        let initial_test = self.test(ticker, config, false);
        let mut highest_cash = *initial_test.cash();
        println!("Initial test:");
        println!("{}", initial_test);
        let mut best_relationships = self.relationships.clone();
        let algo = self.clone();
        let ticker_pairs = algo.get_ticker_pairs(ticker);
        for i in 0..iterations {
            for ticker_pair in &ticker_pairs {
                self.relationships.get_mut(*ticker_pair).unwrap().weight = rand::random::<f64>();
            }
            let test = self.test(ticker, config, false);
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
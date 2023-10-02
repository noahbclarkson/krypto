use std::{collections::HashMap, fs::File};

use csv::Writer;
use getset::{Getters, Setters};

use crate::{
    candle::close_time_to_date,
    config::Config,
    historical_data::HistoricalData,
    math::percentage_change,
    relationship::{compute_relationships, predict_multiple, Prediction, Relationship},
    test_result::{test_headers, TestResult},
};

pub const STARTING_CASH: f64 = 1000.0;

#[derive(Getters, Setters)]
#[getset(get = "pub")]
pub struct Algorithm {
    predictions: Vec<Prediction>,
    relationships: Vec<Box<[Relationship]>>,
    #[getset(skip)]
    writer: Option<AlgorithmWriter>,
    #[getset(set = "pub")]
    write_to_file: bool,
}

#[derive(Getters, Setters, Clone)]
#[getset(get = "pub", set = "pub")]
pub struct Ratio {
    pub start: f64,
    pub end: f64,
}

impl Ratio {
    pub fn new(start: f64, end: f64) -> Self {
        Self { start, end }
    }

    pub fn get_periods(&self, periods: usize) -> usize {
        let start = (periods as f64 * self.start) as usize;
        let end = (periods as f64 * self.end) as usize;
        end - start
    }
}

impl Algorithm {
    pub async fn new(candles: &[HistoricalData], config: &Config, ratio: Ratio) -> Self {
        let mut relationships = Vec::new();
        for data in candles {
            let relationship = compute_relationships(data.tickers(), config, ratio.clone()).await;
            relationships.push(relationship);
        }
        Self {
            predictions: Vec::new(),
            relationships,
            writer: None,
            write_to_file: false,
        }
    }

    pub fn backtest(
        &mut self,
        data: &[HistoricalData],
        config: &Config,
        ratio: Ratio,
        print: bool,
    ) -> TestResult {
        let mut test = TestResult::new(STARTING_CASH);
        self.initialize_csv_writer(config);

        let update_predictions = self.predictions.is_empty();

        let start = (config.depth() + 1) + (*config.periods() as f64 * ratio.start) as usize;
        let end = (*config.periods() as f64 * ratio.end) as usize - *config.depth();
        let candles = data[0].tickers();

        for (iteration, pos) in (start..end).enumerate() {
            let prediction = if update_predictions {
                let current_time = data[0].tickers()[0].candles()[pos].close_time();
                let positions = data
                    .iter()
                    .map(|d| d.find_matching_close_time_index(*current_time))
                    .collect::<Vec<_>>();
                self.update_predictions(positions, data)
            } else {
                self.predictions[iteration].clone()
            };
            if prediction.score() > &config.min_score().unwrap_or_default() {
                let current_price = candles[*prediction.target_index()].candles()[pos].close();
                let exit_price =
                    candles[*prediction.target_index()].candles()[pos + *config.depth()].close();

                let change = percentage_change(*current_price, *exit_price) / 100.0;
                let fee_change =
                    test.cash() * config.fee().unwrap_or_default() * config.leverage() / 100.0;

                test.add_cash(-fee_change);
                test.add_cash(test.cash() * config.leverage() * change);

                match change {
                    x if x > 0.0 => test.add_correct(),
                    x if x < 0.0 => test.add_incorrect(),
                    _ => (),
                }
                if print {
                    println!("{}", test);
                }

                if self.write_to_file {
                    let record = RecordBuilder::new()
                        .cash(*test.cash())
                        .accuracy(test.get_accuracy())
                        .ticker(candles[*prediction.target_index()].ticker().to_string())
                        .score(*prediction.score())
                        .change_result(change)
                        .current_price(*current_price)
                        .exit_price(*exit_price)
                        .change_percent(change)
                        .close_time_start(close_time_to_date(
                            *candles[*prediction.target_index()].candles()[pos].close_time(),
                        ))
                        .close_time_end(close_time_to_date(
                            *candles[*prediction.target_index()].candles()[pos + *config.depth()]
                                .close_time(),
                        ))
                        .fee_change(fee_change)
                        .build();

                    self.writer.as_mut().unwrap().write_record(&record);
                }

                if *test.cash() <= 0.0 {
                    test = test.set_cash(0.0).clone();
                    if update_predictions {
                        continue;
                    }
                    break;
                }
            }
        }
        test
    }

    // pub fn simple_backtest(&mut self, candles: &[TickerData], config: &Config, ratio: Ratio) -> TestResult {
    //     let mut test = TestResult::new(STARTING_CASH);
    //     self.initialize_csv_writer(config);

    //     let update_predictions = self.predictions.is_empty();
    //     let mut p_index = 0;

    //     let start = (config.depth() + 1) + (*config.periods() as f64 * ratio.start) as usize;
    //     let end = (*config.periods() as f64 * ratio.end) as usize - *config.depth();

    //     for i in start..end {
    //         let (index, score) = if update_predictions {
    //             self.update_predictions(i, candles)
    //         } else {
    //             self.get_cached_prediction(&mut p_index)
    //         };
    //         if score > config.min_score().unwrap_or_default() {
    //             let current_price = candles[index].candles()[i].close();
    //             let exit_price = candles[index].candles()[i + *config.depth()].close();

    //             let change = percentage_change(*current_price, *exit_price) / 100.0;
    //             let fee_change =
    //                 test.cash() * config.fee().unwrap_or_default() * config.leverage() / 100.0;

    //             test.add_cash(-fee_change);
    //             test.add_cash(test.cash() * config.leverage() * change);

    //             match change {
    //                 x if x > 0.0 => test.add_correct(),
    //                 x if x < 0.0 => test.add_incorrect(),
    //                 _ => (),
    //             }

    //             if self.write_to_file {
    //                 let record = RecordBuilder::new()
    //                     .cash(*test.cash())
    //                     .accuracy(test.get_accuracy())
    //                     .ticker(candles[index].ticker().to_string())
    //                     .score(score)
    //                     .change_result(change)
    //                     .current_price(*current_price)
    //                     .exit_price(*exit_price)
    //                     .change_percent(change)
    //                     .close_time_start(close_time_to_date(
    //                         *candles[index].candles()[i].close_time(),
    //                     ))
    //                     .close_time_end(close_time_to_date(
    //                         *candles[index].candles()[i + *config.depth()].close_time(),
    //                     ))
    //                     .fee_change(fee_change)
    //                     .build();

    //                 self.writer.as_mut().unwrap().write_record(&record);
    //             }

    //             if *test.cash() <= 0.0 {
    //                 test = test.set_cash(0.0).clone();
    //                 if update_predictions {
    //                     continue;
    //                 }
    //                 break;
    //             }
    //         }
    //     }
    //     test
    // }

    fn initialize_csv_writer(&mut self, config: &Config) {
        if self.write_to_file {
            let mut writer = AlgorithmWriter::new(config);
            writer.write_headers();
            self.writer = Some(writer);
        }
    }

    #[inline]
    fn update_predictions(&mut self, positions: Vec<usize>, data: &[HistoricalData]) -> Prediction {
        let mut predictions = Vec::new();
        for ((relationships, position), d) in self.relationships.iter().zip(positions).zip(data) {
            let prediction = predict_multiple(&relationships, position, d.tickers());
            predictions.push(prediction);
        }
        // Add all predictions with the same index together
        let mut prediction_map = HashMap::new();
        for prediction in predictions.iter().flat_map(|p| p.iter()) {
            let entry = prediction_map
                .entry(prediction.target_index())
                .or_insert(0.0);
            *entry += prediction.score();
        }
        // Find the best prediction
        let (target_index, score) = prediction_map
            .into_iter()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .unwrap();
        let prediction = Prediction::new(*target_index, score);
        self.predictions.push(prediction);
        Prediction::new(*target_index, score)
    }

    #[inline]
    pub fn reset(&mut self) {
        self.predictions.clear();
    }
}

struct AlgorithmWriter {
    csv_writer: Writer<File>,
}

impl AlgorithmWriter {
    fn new(config: &Config) -> Self {
        let csv_writer = Writer::from_path(format!(
            "{}-{}-{}-{}.csv",
            config.intervals()[0],
            config.depth(),
            config.min_score().unwrap_or_default(),
            config.leverage()
        ))
        .unwrap();
        Self { csv_writer }
    }

    fn write_headers(&mut self) {
        self.csv_writer.write_record(&test_headers()).unwrap();
    }

    fn write_record(&mut self, record: &[String]) {
        self.csv_writer.write_record(record).unwrap();
    }
}

pub struct RecordBuilder {
    cash: f64,
    accuracy: f64,
    ticker: String,
    score: f64,
    change_result: String,
    current_price: f64,
    exit_price: f64,
    change_percent: f64,
    close_time_start: String,
    close_time_end: String,
    fee_change: f64,
}

impl RecordBuilder {
    pub fn new() -> Self {
        Self {
            cash: 0.0,
            accuracy: 0.0,
            ticker: String::new(),
            score: 0.0,
            change_result: String::new(),
            current_price: 0.0,
            exit_price: 0.0,
            change_percent: 0.0,
            close_time_start: String::new(),
            close_time_end: String::new(),
            fee_change: 0.0,
        }
    }

    pub fn cash(mut self, cash: f64) -> Self {
        self.cash = cash;
        self
    }

    pub fn accuracy(mut self, accuracy: f64) -> Self {
        self.accuracy = accuracy;
        self
    }

    pub fn ticker(mut self, ticker: String) -> Self {
        self.ticker = ticker;
        self
    }

    pub fn score(mut self, score: f64) -> Self {
        self.score = score;
        self
    }

    pub fn change_result(mut self, change: f64) -> Self {
        self.change_result = match change {
            x if x > 0.0 => "Correct".to_string(),
            x if x < 0.0 => "Incorrect".to_string(),
            _ => "None".to_string(),
        };
        self
    }

    pub fn current_price(mut self, current_price: f64) -> Self {
        self.current_price = current_price;
        self
    }

    pub fn exit_price(mut self, exit_price: f64) -> Self {
        self.exit_price = exit_price;
        self
    }

    pub fn change_percent(mut self, change_percent: f64) -> Self {
        self.change_percent = change_percent;
        self
    }

    pub fn close_time_start(mut self, close_time_start: chrono::NaiveDateTime) -> Self {
        self.close_time_start = format_date(close_time_start);
        self
    }

    pub fn close_time_end(mut self, close_time_end: chrono::NaiveDateTime) -> Self {
        self.close_time_end = format_date(close_time_end);
        self
    }

    pub fn fee_change(mut self, fee_change: f64) -> Self {
        self.fee_change = fee_change;
        self
    }

    pub fn build(self) -> Vec<String> {
        vec![
            format!("${:.2}", self.cash),
            format!("{:.2}%", self.accuracy * 100.0),
            self.ticker,
            format!("{:.5}", self.score),
            self.change_result,
            format!("${:.5}", self.current_price),
            format!("${:.5}", self.exit_price),
            format!("{:.2}%", self.change_percent * 100.0),
            self.close_time_start,
            self.close_time_end,
            format!("$-{:.2}", self.fee_change),
        ]
    }
}

fn format_date(date: chrono::NaiveDateTime) -> String {
    date.format("%Y-%m-%d %H:%M:%S").to_string()
}

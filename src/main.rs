use std::{fs::File, error::Error};

use binance_r_matrix::{error::BinanceDataError, config::HistoricalDataConfigBuilder, interval::Interval, historical_data::HistoricalData};
use r_matrix::{
    dataset::DatasetBuilder,
    r_matrix::{
        cmaes::{CMAESOptimize, RMatrixCMAESSettingsBuilder},
        matrix::RMatrixBuilder,
    },
    NormalizationFunctionType,
};

const TICKERS: [&str; 4] = ["BTCUSDT", "ETHUSDT", "BNBUSDT", "ADAUSDT"];

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    let tickers = TICKERS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let config = HistoricalDataConfigBuilder::default()
        .tickers(tickers)
        .periods(3000)
        .interval(Interval::FifteenMinutes)
        .build()
        .unwrap();
    let mut historical_data = HistoricalData::new(config);
    historical_data.load().await?;
    historical_data.calculate_technicals()?;
    let dataset = historical_data.to_dataset();
    let mut r_matrix = RMatrixBuilder::default()
        .depth(10)
        .max_forward_depth(10)
        .function(NormalizationFunctionType::Softsign)
        .build()
        .unwrap();
    let data = Box::new(dataset);
    r_matrix.train(&data).unwrap();
    let cmaes_settings = RMatrixCMAESSettingsBuilder::default()
        .optimize(CMAESOptimize::Cash)
        .build()
        .unwrap();
    r_matrix.optimize(&data, cmaes_settings);
    r_matrix.plot_cash(&data).unwrap();
    let test_data = r_matrix
        .clone()
        .test(&data);
    
    Ok(())
}

// pub fn main() {
//     // Load dataset from dataset.csv file
//     // Date,Close/Last,Volume,Open,High,Low
//     let appl = get_percentage_changes("dataset.csv");
//     let sbux = get_percentage_changes("dataset2.csv");
//     let msft = get_percentage_changes("dataset3.csv");
//     // Make both the same length by removing the first few days if needed
//     println!("appl: {}", appl.len());
//     println!("sbux: {}", sbux.len());
//     println!("msft: {}", msft.len());
//     let mut dataset = DatasetBuilder::default();
//     dataset.set_feature_names(vec![
//         "appl".to_string(),
//         "sbux".to_string(),
//         "msft".to_string(),
//     ]);
//     dataset.set_label_names(vec![
//         "appl".to_string(),
//         "sbux".to_string(),
//         "msft".to_string(),
//     ]);
//     for i in 0..appl.len() {
//         dataset.add_data_point(
//             i,
//             vec![appl[i], sbux[i], msft[i]],
//             vec![appl[i], sbux[i], msft[i]],
//         );
//     }
//     let dataset = dataset.build().unwrap();
//     let mut r_matrix = RMatrixBuilder::default()
//         .depth(10)
//         .max_forward_depth(10)
//         .function(NormalizationFunctionType::Softsign)
//         .build()
//         .unwrap();
//     let data = Box::new(dataset);
//     r_matrix.train(&data).unwrap();
//     let cmaes_settings = RMatrixCMAESSettingsBuilder::default()
//         .optimize(CMAESOptimize::Cash)
//         .build()
//         .unwrap();
//     r_matrix.optimize(&data, cmaes_settings);
//     r_matrix.plot_cash(&data).unwrap();
//     let (accuracy, error, cash) = r_matrix
//         .clone()
//         .accuracy_error_cash(r_matrix.weights(), &data)
//         .unwrap();
//     // Print in a pretty way
//     println!("Accuracy: {:.2}%", accuracy * 100.0);
//     println!("Error: {:.6}", error);
//     println!("Cash: ${:.2}", cash);
// }

// fn get_percentage_changes(filepath: &str) -> Vec<f64> {
//     let file = File::open(filepath).unwrap();
//     let mut rdr = csv::Reader::from_reader(file);
//     // Get the percentage change for each day
//     let mut last_close = 0.0;
//     let mut percentage_change = Vec::new();
//     for result in rdr.records() {
//         let record = result.unwrap();
//         let close = record[1].replace('$', "").parse::<f64>().unwrap();
//         if last_close != 0.0 {
//             let change = (close - last_close) / last_close;
//             percentage_change.push(change);
//         }
//         last_close = close;
//     }
//     percentage_change
// }

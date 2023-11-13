use std::panic::catch_unwind;

use binance_r_matrix::matrix::BinanceRMatrix;
use binance_r_matrix::{
    HistoricalData, HistoricalDataConfig, HistoricalDataConfigBuilder, Interval, BinanceDataId,
};
use krypto::config::Config;
use r_matrix::data::RMatrixId;
use r_matrix::matricies::{ForestConfigBuilder, ForestRMatrix, RMatrix};
use r_matrix::rtest::{RTest, RTestConfigBuilder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::read_config(None).await.unwrap();
    let mut data = HistoricalData::new(config.clone().into());
    data.load().await?;
    data.calculate_technicals()?;
    let rdata = data.to_rdata()?;
    let mut matricies: Vec<Option<Box<dyn RMatrix<BinanceDataId>>>> = Vec::new();

    for (individual, ticker) in rdata.iter().zip(config.tickers().iter()) {
        println!("Creating matrix for {}", ticker);
        
        let config = config.clone().into();
        
        let result = std::panic::catch_unwind(|| {
            Box::new(ForestRMatrix::new(&individual, config).unwrap())
        });
    
        match result {
            Ok(r_matrix) => matricies.push(Some(r_matrix)),
            Err(_) => {
                println!("Matrix for {} could not be created due to a panic!", ticker);
                matricies.push(None);
            }
        }
    }
    let rmatrix = BinanceRMatrix::new(matricies, rdata);
    let mut cash = 1000.0;
    let mut correct = 0;
    let mut incorrect = 0;
    let mut last_ticker = "".to_string();
    let mut last_prediction = 0.0;
    for i in *config.training_periods()..(config.training_periods() + config.testing_periods() - 1) {
        let predictions = rmatrix.predict(i);
        // Get the index of the prediction with the higest value
        let mut max_index = 0;
        let mut max_value = 0.0f64;
        for (index, prediction) in predictions.iter().enumerate() {
            if prediction.abs() > max_value.abs() {
                max_index = index;
                max_value = *prediction;
            }
        }
        let real = rmatrix.data()[max_index].target().data()[i];
        let prediction = predictions[max_index];
        if prediction.abs() < config.min_score().unwrap_or(0.0) {
            last_ticker = "".to_string();
            last_prediction = prediction;
            continue;
        }
        if prediction * real > 0.0 {
            cash += cash * real.abs() * config.margin();
            correct += 1;
        } else if prediction * real < 0.0 {
            cash -= cash * real.abs() * config.margin();
            incorrect += 1;
        }
        if last_ticker != config.tickers()[max_index] || last_prediction * prediction < 0.0 {
            cash -= cash * config.fee().unwrap_or(0.0) * 0.01 * config.margin();
        }
        last_ticker = config.tickers()[max_index].clone();
        last_prediction = prediction;
        println!(
            "Prediction: {:.3}%, Real: {:.3}%, Cash: {:.3}, Ticker: {}",
            prediction * 100.0, real * 100.0, cash, config.tickers()[max_index]
        );
        // println!("{}", cash);
    }
    println!(
        "Correct: {}, Incorrect: {}, Accuracy: {:.3}%",
        correct,
        incorrect,
        correct as f64 / (correct + incorrect) as f64 * 100.0
    );
    Ok(())
}

fn test(predictions: Vec<f64>) {

}

#[derive(Debug, Clone, Copy)]
pub enum DataType {
    PercentageChangePredict,
    PercentageChangeReal,
}

impl RMatrixId for DataType {
    fn get_id(&self) -> &str {
        match self {
            DataType::PercentageChangePredict => "percentage_change_predict",
            DataType::PercentageChangeReal => "percentage_change_real",
        }
    }

    fn is_target(&self) -> bool {
        match self {
            DataType::PercentageChangePredict => true,
            DataType::PercentageChangeReal => false,
        }
    }
}

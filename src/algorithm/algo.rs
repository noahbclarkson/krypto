use std::fmt;

use linfa::traits::Predict as _;
use linfa_pls::PlsRegression;
use ndarray::Array2;
use tracing::{debug, info, instrument};

use crate::{
    algorithm::{pls::get_pls, test_data::TestData},
    config::KryptoConfig,
    data::{candlestick::Candlestick, dataset::IntervalData},
    error::KryptoError,
    util::matrix_utils::normalize_by_columns,
};

pub struct Algorithm {
    pub pls: PlsRegression<f64>,
    pub n_components: usize,
    pub depth: usize,
    pub ticker: String,
    pub monthly_return: f64,
    pub accuracy: f64,
}

impl Algorithm {
    #[instrument(skip(dataset, config))]
    pub fn load(
        dataset: &IntervalData,
        n: usize,
        depth: usize,
        ticker: &str,
        config: &KryptoConfig,
    ) -> Result<Self, KryptoError> {
        let (monthly_return, accuracy) = backtest(dataset, n, depth, ticker, config)?;
        let (features, labels, _) = get_overall_dataset(dataset, depth, ticker);
        let pls = get_pls(features, labels, n)?;
        Ok(Self {
            pls,
            n_components: n,
            depth,
            ticker: ticker.to_string(),
            monthly_return,
            accuracy,
        })
    }

    pub fn get_ticker(&self) -> &str {
        &self.ticker
    }

    pub fn get_monthly_return(&self) -> &f64 {
        &self.monthly_return
    }

    pub fn get_accuracy(&self) -> f64 {
        self.accuracy
    }

    pub fn get_n_components(&self) -> usize {
        self.n_components
    }

    pub fn get_depth(&self) -> usize {
        self.depth
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Ticker: {} | Monthly Return: {:.2} | Accuracy: {:.2}% | Depth: {} | N Components: {}",
            self.ticker,
            self.monthly_return * 100.0,
            self.accuracy * 100.0,
            self.depth,
            self.n_components
        )
    }
}

#[instrument(skip(dataset, config, n, depth, ticker))]
fn backtest(
    dataset: &IntervalData,
    n: usize,
    depth: usize,
    ticker: &str,
    config: &KryptoConfig,
) -> Result<(f64, f64), KryptoError> {
    info!("Running backtest");
    let (features, labels, candles) = get_overall_dataset(dataset, depth, ticker);
    let count = config.cross_validations;
    let total_size = candles.len();
    let test_data_size = (total_size as f64 / count as f64).floor() as usize - 1;
    let mut test_datas = Vec::new();
    for i in 0..count {
        let start = i * test_data_size;
        let end = if i == count - 1 {
            total_size
        } else {
            (i + 1) * test_data_size
        };
        debug!("Start: {} | End: {}", start, end);
        let mut train_features = features.clone();
        let test_features: Vec<Vec<f64>> = train_features.drain(start..end).collect();
        let mut train_labels = labels.clone();
        train_labels.drain(start..end);
        let test_candles = candles.clone().drain(start..end).collect();
        let pls = get_pls(train_features, train_labels, n)?;
        let predictions = get_predictions(pls, test_features);
        debug!("Running cross validation: {}/{}", i + 1, count);
        let test_data = TestData::new(predictions, test_candles, config);
        debug!("Cross Validation {}: {}", i + 1, test_data);
        test_datas.push(test_data);
    }
    let returns = test_datas
        .iter()
        .map(|d| d.monthly_return)
        .collect::<Vec<f64>>();
    let accuracies = test_datas.iter().map(|d| d.accuracy).collect::<Vec<f64>>();
    let median_return = returns[returns.len() / 2];
    let median_accuracy = accuracies[accuracies.len() / 2];
    info!(
        "Median Monthly Return: {:.2} | Median Accuracy: {:.2}%",
        median_return * 100.0,
        median_accuracy * 100.0
    );
    Ok((median_return, median_accuracy))
}

fn get_predictions(pls: PlsRegression<f64>, features: Vec<Vec<f64>>) -> Vec<f64> {
    let features = Array2::from_shape_vec(
        (features.len(), features[0].len()),
        features.iter().flatten().cloned().collect(),
    )
    .unwrap();
    pls.predict(&features).as_slice().unwrap().to_vec()
}

#[instrument(skip(dataset))]
fn get_overall_dataset(
    dataset: &IntervalData,
    depth: usize,
    ticker: &str,
) -> (Vec<Vec<f64>>, Vec<f64>, Vec<Candlestick>) {
    let records = dataset.get_records();
    let predictors = normalize_by_columns(records);
    // Set all NaN values to 0
    let predictors: Vec<Vec<f64>> = predictors
        .iter()
        .map(|r| {
            r.iter()
                .map(|v| {
                    if v.is_nan() {
                        debug!("Found NaN value");
                        0.0
                    } else {
                        *v
                    }
                })
                .collect()
        })
        .collect();
    let features: Vec<Vec<f64>> = predictors
        .windows(depth)
        .map(|w| w.iter().flatten().copied().collect::<Vec<f64>>())
        .collect();
    // Remove the last features row to match the labels length
    let features: Vec<Vec<f64>> = features.iter().take(features.len() - 1).cloned().collect();
    let labels: Vec<f64> = dataset
        .get(ticker)
        .unwrap()
        .get_labels()
        .iter()
        .skip(depth)
        .cloned()
        .collect();
    // Set NaN values to 1
    let labels: Vec<f64> = labels
        .iter()
        .map(|v| if v.is_nan() { 1.0 } else { *v })
        .collect();
    let candles: Vec<Candlestick> = dataset
        .get(ticker)
        .unwrap()
        .get_candles()
        .iter()
        .skip(depth)
        .cloned()
        .collect();
    debug!("Features Shape: {}x{}", features.len(), features[0].len());
    debug!("Labels Shape: {}", labels.len());
    debug!("Candles Shape: {}", candles.len());
    (features, labels, candles)
}

use std::{cmp::Ordering, collections::HashMap};

use linfa::prelude::*;
use linfa_pls::PlsRegression;
use ndarray::{Array, Array2};
use smartcore::{
    ensemble::random_forest_classifier::RandomForestClassifier, linalg::basic::matrix::DenseMatrix,
};
use tracing::{debug, instrument};

use crate::{
    algorithm_type::AlgorithmType,
    dataset::{DataArray, Dataset, Key},
    interval::Interval,
    util::{annualized_return, calmar_ratio, format_number},
    KryptoConfig, KryptoError,
};

#[instrument(skip(config, data))]
pub async fn load_algorithm(
    config: &KryptoConfig,
    data: &Dataset,
) -> Result<Vec<AlgorithmResult>, KryptoError> {
    let (train, test) = data.split(config.split);
    let mut results = Vec::new();
    for algorithm_type in config.algorithms.iter() {
        let predictions = predictions(&train, &test, algorithm_type, config).await?;
        for ((ticker, interval), prediction) in predictions.iter() {
            let d = test.data.get(&(ticker.clone(), *interval)).unwrap();
            let labels = d
                .data
                .iter()
                .skip(1)
                .map(|c| c.percentage_change.unwrap().signum())
                .collect::<Vec<_>>();
            let mut cash = 1000.0;
            let mut correct = 0;
            let mut incorrect = 0;
            let mut cash_history = vec![cash];
            let mut last_predictions = None;
            for (label, prediction) in labels.iter().zip(prediction.iter()) {
                let prediction = *prediction as f64;
                if prediction == 1.0 {
                    cash *= 1.0 + label * config.margin;
                } else {
                    cash *= 1.0 - label * config.margin;
                }
                match prediction.signum() == label.signum() {
                    true => correct += 1,
                    false => incorrect += 1,
                }
                if let Some(last_predictions) = last_predictions {
                    if last_predictions != prediction.signum() {
                        cash *= 1.0 - config.fee * config.margin;
                    }
                }
                if cash <= 0.0 {
                    break;
                }
                last_predictions = Some(prediction.signum());
                cash_history.push(cash);
            }
            let start_date = d.data[0].open_time;
            let end_date = d.data.last().unwrap().open_time;
            let calmar = calmar_ratio(&cash_history, start_date, end_date);
            let accuracy = correct as f64 / (correct + incorrect) as f64 * 100.0;
            let annualized_return = annualized_return(&cash_history, start_date, end_date) * 100.0;
            let result = AlgorithmResult::new(
                cash,
                calmar,
                accuracy,
                annualized_return,
                cash_history.len(),
                ticker.to_string(),
                *interval,
            );
            results.push(result);
        }
    }
    Ok(results)
}

async fn predictions(
    train: &Dataset,
    test: &Dataset,
    algorithm: &AlgorithmType,
    config: &KryptoConfig,
) -> Result<HashMap<Key, Vec<usize>>, KryptoError> {
    let mut predictions = HashMap::new();
    debug!("Training Classifiers");
    let mut tasks = Vec::new();
    for (key, train_data) in train.data.iter() {
        let test_data = test.data.get(key).unwrap();
        let train_data = train_data.clone();
        let test_data = test_data.clone();
        let algorithm = algorithm.clone();
        let config = config.clone();
        tasks.push(tokio::spawn(algorithm_task(
            key.clone(),
            train_data,
            test_data,
            algorithm,
            config,
        )));
    }
    for task in futures::future::join_all(tasks).await {
        let (key, output) = task??;
        predictions.insert(key, output);
    }
    debug!("Classifiers have been trained.");
    Ok(predictions)
}

#[instrument(skip(train_data, test_data, config))]
async fn algorithm_task(
    key: Key,
    train_data: DataArray,
    test_data: DataArray,
    algorithm: AlgorithmType,
    config: KryptoConfig,
) -> Result<(Key, Vec<usize>), KryptoError> {
    debug!("Training {} for {} {}", algorithm, key.0, key.1);
    let (features, labels) = features_and_labels(&train_data);

    debug!(
        "Features and labels have been loaded with {}x{} features and {} labels.",
        features.len(),
        features[0].len(),
        labels.len()
    );
    let output = match algorithm {
        AlgorithmType::RandomForest => {
            let x = DenseMatrix::from_2d_array(&features)?;
            let labels = labels
                .iter()
                .map(|&x| x.signum() as usize)
                .collect::<Vec<_>>();
            let classifier = RandomForestClassifier::fit(&x, &labels, Default::default()).unwrap();
            let test_features = test_data
                .data
                .iter()
                .map(|c| c.features.as_ref().unwrap().as_slice())
                .collect::<Vec<_>>();
            let test_x = DenseMatrix::from_2d_array(&test_features)?;
            classifier.predict(&test_x).unwrap()
        }
        AlgorithmType::PartialLeastSquares => {
            let num_features = features[0].len();
            let flattened_features: Vec<f64> = features.into_iter().flatten().copied().collect();
            let records: Array2<f64> =
                Array2::from_shape_vec((labels.len(), num_features), flattened_features).unwrap();
            let targets: Array2<f64> =
                Array::from_shape_vec((labels.len(), 1), labels.to_vec()).unwrap();
            let ds = linfa::dataset::Dataset::new(records, targets);
            let pls = PlsRegression::params(config.pls_components)
                .scale(true)
                .max_iterations(250)
                .fit(&ds)
                .unwrap();
            let test_features = test_data
                .data
                .iter()
                .map(|c| c.features.as_ref().unwrap().as_slice())
                .collect::<Vec<_>>();
            let flattened_test_features: Vec<f64> =
                test_features.into_iter().flatten().copied().collect();
            let test_records: Array2<f64> = Array2::from_shape_vec(
                (test_data.data.len(), num_features),
                flattened_test_features,
            )
            .unwrap();
            let predictions = pls.predict(&test_records);
            predictions
                .into_iter()
                .map(|x| x.signum() as usize)
                .collect()
        }
        _ => {
            unimplemented!()
        }
    };
    debug!("Predictions have been loaded.");
    Ok((key, output))
}

pub fn get_pls(data: &DataArray, config: &KryptoConfig) -> PlsRegression<f64> {
    let (features, labels) = features_and_labels(data);
    let num_features = features[0].len();
    let flattened_features: Vec<f64> = features.into_iter().flatten().copied().collect();
    let records: Array2<f64> =
        Array2::from_shape_vec((labels.len(), num_features), flattened_features).unwrap();
    let targets: Array2<f64> = Array::from_shape_vec((labels.len(), 1), labels.to_vec()).unwrap();
    let ds = linfa::dataset::Dataset::new(records, targets);
    PlsRegression::params(config.pls_components)
        .scale(true)
        .max_iterations(250)
        .fit(&ds)
        .unwrap()
}

fn features_and_labels(data: &DataArray) -> (Vec<&[f64]>, Vec<f64>) {
    let features = data
        .data
        .iter()
        .map(|c| c.features.as_ref().unwrap().as_slice())
        .collect::<Vec<_>>();
    let labels = data
        .data
        .iter()
        .skip(1)
        .map(|c| c.percentage_change.unwrap().signum())
        .collect();
    // Remove the last row from features
    let features = features.iter().take(features.len() - 1).cloned().collect();
    (features, labels)
}

#[derive(Debug, Clone)]
pub struct AlgorithmResult {
    pub cash: f64,
    pub calmar: f64,
    pub accuracy: f64,
    pub annualized_return: f64,
    pub trades: usize,
    pub ticker: String,
    pub interval: Interval,
}

impl AlgorithmResult {
    pub fn new(
        cash: f64,
        calmar: f64,
        accuracy: f64,
        annualized_return: f64,
        trades: usize,
        ticker: String,
        interval: Interval,
    ) -> Self {
        AlgorithmResult {
            cash,
            calmar,
            accuracy,
            annualized_return,
            trades,
            ticker,
            interval,
        }
    }
}

impl std::fmt::Display for AlgorithmResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Ticker: {}, Interval: {}, Cash: ${}, Calmar: {:.3}, Accuracy: {:.2}%, Annualized Return: {:.2}%",
            self.ticker,
            self.interval,
            format_number(self.cash as f32),
            self.calmar,
            self.accuracy,
            self.annualized_return

        )
    }
}

impl PartialOrd for AlgorithmResult {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AlgorithmResult {
    fn cmp(&self, other: &Self) -> Ordering {
        self.calmar.partial_cmp(&other.calmar).unwrap()
    }
}

impl PartialEq for AlgorithmResult {
    fn eq(&self, other: &Self) -> bool {
        self.calmar == other.calmar
    }
}

impl Eq for AlgorithmResult {}

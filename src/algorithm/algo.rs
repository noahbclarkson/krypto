use std::fmt;

use linfa_pls::PlsRegression;
use tracing::{debug, info, instrument};

use crate::{
    algorithm::{
        pls::{get_pls, predict},
        test_data::TestData,
    },
    config::KryptoConfig,
    data::{candlestick::Candlestick, dataset::IntervalData},
    error::KryptoError,
    util::{math_utils::median, matrix_utils::normalize_by_columns},
};

pub struct Algorithm {
    pub pls: PlsRegression<f64>,
    settings: AlgorithmSettings,
    result: AlgorithmResult,
}

impl Algorithm {
    #[instrument(skip(dataset, config))]
    pub fn load(
        dataset: &IntervalData,
        settings: AlgorithmSettings,
        config: &KryptoConfig,
    ) -> Result<Self, KryptoError> {
        let result = Self::backtest(dataset, &settings, config)?;
        let (features, labels, _) = Self::prepare_dataset(dataset, &settings);
        let pls = get_pls(features, labels, settings.n)?;
        Ok(Self {
            pls,
            settings,
            result,
        })
    }

    fn backtest(
        dataset: &IntervalData,
        settings: &AlgorithmSettings,
        config: &KryptoConfig,
    ) -> Result<AlgorithmResult, KryptoError> {
        debug!("Running backtest");

        let (features, labels, candles) = Self::prepare_dataset(dataset, settings);
        let count = config.cross_validations;
        let total_size = candles.len();
        let test_data_size = total_size / count;

        let mut test_results = Vec::with_capacity(count);

        for i in 0..count {
            let start = i * test_data_size;
            let end = match i == count - 1 {
                true => total_size,
                false => (i + 1) * test_data_size,
            };

            let test_features = &features[start..end];
            let test_candles = &candles[start..end];
            let train_features = [&features[..start], &features[end..]].concat();
            let train_labels = [&labels[..start], &labels[end..]].concat();

            let pls = get_pls(train_features, train_labels, settings.n)?;
            let predictions = predict(&pls, test_features);

            let test_data = TestData::new(predictions, test_candles.to_vec(), config)?;
            debug!(
                "Cross-validation {} ({}-{}): {}",
                i + 1,
                start,
                end,
                test_data
            );
            test_results.push(test_data);
        }

        let median_return = median(
            &test_results
                .iter()
                .map(|d| d.monthly_return)
                .collect::<Vec<_>>(),
        );
        let median_accuracy = median(&test_results.iter().map(|d| d.accuracy).collect::<Vec<_>>());
        let result = AlgorithmResult::new(median_return, median_accuracy);
        info!("Backtest result: {}", result);
        Ok(result)
    }

    #[instrument(skip(dataset))]
    fn prepare_dataset(
        dataset: &IntervalData,
        settings: &AlgorithmSettings,
    ) -> (Vec<Vec<f64>>, Vec<f64>, Vec<Candlestick>) {
        let records = dataset.get_records();
        let normalized_predictors = normalize_by_columns(records)
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|v| if v.is_nan() { 0.0 } else { v })
                    .collect()
            })
            .collect::<Vec<Vec<f64>>>();

        let features = normalized_predictors
            .windows(settings.depth)
            .map(|window| window.iter().flatten().cloned().collect())
            .collect::<Vec<Vec<f64>>>();
        let features = features[..features.len() - 1].to_vec();

        let symbol_data = dataset
            .get(&settings.symbol)
            .expect("Symbol not found in dataset");

        let labels: Vec<f64> = symbol_data
            .get_labels()
            .iter()
            .skip(settings.depth)
            .map(|&v| if v.is_nan() { 1.0 } else { v })
            .collect();

        let candles: Vec<Candlestick> = symbol_data
            .get_candles()
            .iter()
            .skip(settings.depth)
            .cloned()
            .collect();

        debug!("Features shape: {}x{}", features.len(), features[0].len());
        debug!("Labels count: {}", labels.len());
        debug!("Candles count: {}", candles.len());

        (features, labels, candles)
    }

    pub fn get_symbol(&self) -> &str {
        &self.settings.symbol
    }

    pub fn get_monthly_return(&self) -> f64 {
        self.result.monthly_return
    }

    pub fn get_accuracy(&self) -> f64 {
        self.result.accuracy
    }

    pub fn get_n_components(&self) -> usize {
        self.settings.n
    }

    pub fn get_depth(&self) -> usize {
        self.settings.depth
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Algorithm: ({}) | Result: ({})",
            self.settings, self.result
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlgorithmSettings {
    pub n: usize,
    pub depth: usize,
    pub symbol: String,
}

impl AlgorithmSettings {
    pub fn new(n: usize, depth: usize, symbol: &str) -> Self {
        Self {
            n,
            depth,
            symbol: symbol.to_string(),
        }
    }
}

impl fmt::Display for AlgorithmSettings {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Symbol: {} | Depth: {} | Components: {}",
            self.symbol, self.depth, self.n
        )
    }
}

pub struct AlgorithmResult {
    pub monthly_return: f64,
    pub accuracy: f64,
}

impl AlgorithmResult {
    pub fn new(monthly_return: f64, accuracy: f64) -> Self {
        Self {
            monthly_return,
            accuracy,
        }
    }
}

impl fmt::Display for AlgorithmResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Median Monthly Return: {:.2}% | Median Accuracy: {:.2}%",
            self.monthly_return * 100.0,
            self.accuracy * 100.0
        )
    }
}

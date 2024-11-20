use std::fmt;

use linfa_pls::PlsRegression;
use rayon::prelude::*;
use tracing::{debug, info, instrument};

use crate::{
    algorithm::{
        pls::{get_pls, predict},
        test_data::TestData,
    },
    config::KryptoConfig,
    data::dataset::IntervalData,
    error::KryptoError,
    util::math_utils::median,
};

pub struct Algorithm {
    pub pls: PlsRegression<f64>,
    settings: AlgorithmSettings,
    result: AlgorithmResult,
}

impl Algorithm {
    #[instrument(skip(interval_dataset, config))]
    pub fn load(
        interval_dataset: &IntervalData,
        settings: AlgorithmSettings,
        config: &KryptoConfig,
    ) -> Result<Self, KryptoError> {
        let result = Self::backtest(interval_dataset, &settings, config)?;
        let ds = interval_dataset.get_symbol_dataset(&settings);
        let pls = get_pls(
            ds.get_features().clone(),
            ds.get_labels().clone(),
            settings.n,
        )?;
        Ok(Self {
            pls,
            settings,
            result,
        })
    }

    fn backtest(
        interval_dataset: &IntervalData,
        settings: &AlgorithmSettings,
        config: &KryptoConfig,
    ) -> Result<AlgorithmResult, KryptoError> {
        debug!("Running backtest");

        let ds = interval_dataset.get_symbol_dataset(settings);
        let count = config.cross_validations;
        let total_size = ds.len()?;
        let test_data_size = total_size / count;

        let test_results: Vec<TestData> = (0..count)
            .into_par_iter()
            .map(|i| -> Result<TestData, KryptoError> {
                let start = i * test_data_size;
                let end = match i == count - 1 {
                    true => total_size,
                    false => (i + 1) * test_data_size,
                };
                let features = ds.get_features();
                let labels = ds.get_labels();
                let candles = ds.get_candles();
                let test_features = &features[start..end];
                let test_candles = &candles[start..end];
                let train_features = [&features[..start], &features[end..]].concat();
                let train_labels = [&labels[..start], &labels[end..]].concat();

                let pls = get_pls(train_features, train_labels, settings.n)?;
                let predictions = predict(&pls, test_features)?;

                let test_data = TestData::new(predictions, test_candles, config)?;
                debug!(
                    "Cross-validation {} ({}-{}): {}",
                    i + 1,
                    start,
                    end,
                    test_data
                );
                Ok(test_data)
            })
            .collect::<Result<Vec<_>, KryptoError>>()?;

        let median_return = median(
            &test_results
                .iter()
                .map(|d| d.monthly_return)
                .filter(|&v| v.is_finite())
                .collect::<Vec<_>>(),
        );
        let median_accuracy = median(
            &test_results
                .iter()
                .map(|d| d.accuracy)
                .filter(|&v| v.is_finite())
                .collect::<Vec<_>>(),
        );
        let result = AlgorithmResult::new(median_return, median_accuracy);
        info!("Backtest result: {}", result);
        Ok(result)
    }

    #[instrument(skip(interval_dataset, config, self))]
    pub fn backtest_on_all_seen_data(
        &self,
        interval_dataset: &IntervalData,
        config: &KryptoConfig,
    ) -> Result<AlgorithmResult, KryptoError> {
        debug!("Running backtest on all seen data");
        let ds = interval_dataset.get_symbol_dataset(&self.settings);
        let predictions = predict(&self.pls, ds.get_features())?;
        let test_data = TestData::new(predictions, ds.get_candles(), config)?;

        // Evaluate performance metrics
        let monthly_return = test_data.monthly_return;
        let accuracy = test_data.accuracy;

        let result = AlgorithmResult::new(monthly_return, accuracy);
        info!("Backtest on all seen data result: {}", result);
        Ok(result)
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

    pub fn all(symbols: Vec<String>, max_n: usize, max_depth: usize) -> Vec<Self> {
        symbols
            .iter()
            .flat_map(|symbol| {
                (1..=max_n)
                    .flat_map(|n| (1..=max_depth).map(move |depth| Self::new(n, depth, symbol)))
                    .collect::<Vec<_>>()
            })
            .collect()
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

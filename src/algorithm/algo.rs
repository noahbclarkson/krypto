use std::fmt;

use linfa_pls::PlsRegression;
use tracing::{debug, error, info, instrument, warn};

use crate::{
    algorithm::{
        pls::{get_pls, predict},
        // Import TestData for aggregation helpers, SimulationOutput is used internally in backtest funcs
        test_data::{SimulationOutput, TestData},
    },
    config::KryptoConfig,
    data::dataset::interval_data::IntervalData,
    error::KryptoError,
    util::math_utils::median, // Keep median for aggregation
};

pub struct Algorithm {
    pub pls: PlsRegression<f64>,
    pub settings: AlgorithmSettings,
    pub result: AlgorithmResult, // Result now contains more metrics
}

impl fmt::Debug for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Algorithm {{ settings: {:?}, result: {:?} }}",
            self.settings, self.result
        )
    }
}

impl Algorithm {
    /**
    Load the algorithm by training on the full dataset and performing walk-forward validation.

    ## Arguments
    * `interval_dataset` - The dataset for a specific interval.
    * `settings` - The algorithm parameters (n, depth, symbol).
    * `config` - The application configuration.

    ## Returns
    A trained `Algorithm` instance with its validation result, or a `KryptoError`.
    */
    #[instrument(skip(interval_dataset, config), fields(symbol=%settings.symbol, depth=settings.depth, n=settings.n, interval=%interval_dataset.interval()))]
    pub fn load(
        interval_dataset: &IntervalData,
        settings: AlgorithmSettings,
        config: &KryptoConfig,
    ) -> Result<Self, KryptoError> {
        info!("Loading algorithm and running walk-forward validation...");

        // 1. Perform Walk-Forward Validation
        let validation_result = Self::backtest_walk_forward(interval_dataset, &settings, config)?;
        info!("Walk-forward validation result: {}", validation_result);

        // 2. Train final model on the entire dataset for potential future use (e.g., trading or log generation)
        let full_dataset = interval_dataset.get_symbol_dataset(&settings)?;
        if full_dataset.is_empty() {
            return Err(KryptoError::InsufficientData {
                got: 0,
                required: 1,
                context: format!(
                    "Full dataset empty for final model training (Symbol: {}, Interval: {})",
                    settings.symbol,
                    interval_dataset.interval()
                ),
            });
        }

        // Validate data before final fit
        Self::validate_data(full_dataset.get_features(), full_dataset.get_labels())?;

        debug!(
            "Training final PLS model on full dataset ({} samples)...",
            full_dataset.len()?
        );
        let final_pls = get_pls(
            full_dataset.get_features(),
            full_dataset.get_labels(), // Use raw labels for PLS training
            settings.n,
        )?;
        debug!("Final PLS model trained successfully.");

        Ok(Self {
            pls: final_pls,
            settings,
            result: validation_result, // Store the result from walk-forward validation
        })
    }

    /// Validates predictor and target data before feeding to PLS.
    fn validate_data(predictors: &[Vec<f64>], target: &[f64]) -> Result<(), KryptoError> {
        if predictors
            .iter()
            .flatten()
            .any(|&x| x.is_nan() || x.is_infinite())
        {
            return Err(KryptoError::PlsInternalError(
                "NaN or Infinity detected in predictor data.".to_string(),
            ));
        }
        if target.iter().any(|&x| x.is_nan() || x.is_infinite()) {
            return Err(KryptoError::PlsInternalError(
                "NaN or Infinity detected in target data.".to_string(),
            ));
        }
        Ok(())
    }

    /**
    Perform walk-forward validation using an expanding window.

    ## Arguments
    * `interval_dataset` - The dataset for a specific interval.
    * `settings` - Algorithm parameters.
    * `config` - Application configuration.

    ## Returns
    Aggregated `AlgorithmResult` from the walk-forward validation.
    */
    fn backtest_walk_forward(
        interval_dataset: &IntervalData,
        settings: &AlgorithmSettings,
        config: &KryptoConfig,
    ) -> Result<AlgorithmResult, KryptoError> {
        debug!("Running Walk-Forward Validation...");
        let full_symbol_dataset = interval_dataset.get_symbol_dataset(settings)?;
        let total_samples = full_symbol_dataset.len()?;
        let num_splits = config.cross_validations;

        if num_splits == 0 || total_samples <= num_splits {
            return Err(KryptoError::InsufficientData {
                got: total_samples,
                required: num_splits + 1,
                context: format!(
                    "Not enough samples ({}) for {} walk-forward splits.",
                    total_samples, num_splits
                ),
            });
        }

        let initial_train_size =
            (total_samples as f64 * config.walk_forward_train_ratio).floor() as usize;
        let remaining_samples = total_samples - initial_train_size;
        if remaining_samples < num_splits {
            return Err(KryptoError::InsufficientData {
                got: remaining_samples,
                required: num_splits, // Need at least num_splits samples for testing
                context: format!(
                    "Not enough remaining samples ({}) for {} walk-forward test splits.",
                    remaining_samples, num_splits
                ),
            });
        }
        let test_size_per_split = remaining_samples / num_splits;

        if initial_train_size == 0 || test_size_per_split == 0 {
            return Err(KryptoError::InsufficientData {
                got: initial_train_size.min(test_size_per_split), // Report the zero value
                required: 1,
                context: format!(
                    "Initial training size ({}) or test size per split ({}) is zero.",
                    initial_train_size, test_size_per_split
                ),
            });
        }

        let mut split_outputs: Vec<SimulationOutput> = Vec::with_capacity(num_splits); // Store full output

        for i in 0..num_splits {
            let train_end = initial_train_size + i * test_size_per_split;
            let test_start = train_end;
            // Ensure the last split includes all remaining data
            let test_end = if i == num_splits - 1 {
                total_samples
            } else {
                test_start + test_size_per_split
            };

            // Additional check for valid range
            if test_start >= test_end || train_end == 0 {
                warn!(
                    "Skipping invalid walk-forward split {}: Train End {}, Test Start {}, Test End {}",
                    i + 1, train_end, test_start, test_end
                );
                continue;
            }

            debug!(
                "Walk-Forward Split {}/{}: Train [0..{}], Test [{}..{}]",
                i + 1, num_splits, train_end, test_start, test_end
            );

            let train_features = &full_symbol_dataset.get_features()[0..train_end];
            let train_labels = &full_symbol_dataset.get_labels()[0..train_end];
            let test_features = &full_symbol_dataset.get_features()[test_start..test_end];
            let test_candles = &full_symbol_dataset.get_candles()[test_start..test_end];

            if train_features.is_empty() || test_features.is_empty() {
                warn!(
                    "Skipping split {} due to empty train ({}) or test ({}) features.",
                    i + 1,
                    train_features.len(),
                    test_features.len()
                );
                continue;
            }

            Self::validate_data(train_features, train_labels)?;

            let pls = match get_pls(train_features, train_labels, settings.n) {
                Ok(model) => model,
                Err(e) => {
                    error!(
                        "Failed to train PLS for split {}: {}. Skipping split.",
                        i + 1,
                        e
                    );
                    // Consider returning error vs. skipping split based on desired robustness
                    return Err(KryptoError::WalkForwardError(format!(
                        "PLS training failed in split {}: {}",
                        i + 1,
                        e
                    )));
                }
            };

            let predictions = match predict(&pls, test_features) {
                Ok(preds) => preds,
                Err(e) => {
                    error!(
                        "Failed to predict PLS for split {}: {}. Skipping split.",
                        i + 1,
                        e
                    );
                    return Err(KryptoError::WalkForwardError(format!(
                        "PLS prediction failed in split {}: {}",
                        i + 1,
                        e
                    )));
                }
            };

            // Run backtest simulation on the test portion
            let simulation_output = match TestData::run_simulation(
                &settings.symbol, // Pass symbol
                &predictions,
                test_candles,
                config,
            ) {
                Ok(output) => output, // Get the full SimulationOutput
                Err(e) => {
                    error!(
                        "Failed to run backtest simulation for split {}: {}. Skipping split.",
                        i + 1,
                        e
                    );
                    return Err(KryptoError::WalkForwardError(format!(
                        "Backtest simulation failed in split {}: {}",
                        i + 1,
                        e
                    )));
                }
            };

            debug!("Split {} Result: {}", i + 1, simulation_output.metrics); // Log metrics
            split_outputs.push(simulation_output); // Store full output
        }

        if split_outputs.is_empty() {
            return Err(KryptoError::WalkForwardError(
                "No valid walk-forward splits completed.".to_string(),
            ));
        }

        // Aggregate results (use median for robustness, average for counts)
        let all_metrics: Vec<&TestData> = split_outputs.iter().map(|o| &o.metrics).collect();

        let median_monthly_return = median(&TestData::get_monthly_returns(all_metrics.as_slice()));
        let median_accuracy = median(&TestData::get_accuracies(all_metrics.as_slice()));
        let median_sharpe = median(&TestData::get_sharpe_ratios(all_metrics.as_slice()));
        let avg_max_drawdown =
            all_metrics.iter().map(|d| d.max_drawdown).sum::<f64>() / all_metrics.len() as f64;
        let avg_total_trades = (all_metrics
            .iter()
            .map(|d| d.total_trades as f64)
            .sum::<f64>()
            / all_metrics.len() as f64)
            .round() as u32;
        // Calculate median win rate correctly
        let win_rates: Vec<f64> = all_metrics
            .iter()
            .map(|d| d.win_rate)
            .filter(|v| v.is_finite())
            .collect();
        let median_win_rate = median(&win_rates);

        let final_result = AlgorithmResult::new(
            median_monthly_return,
            median_accuracy,
            median_sharpe,
            avg_max_drawdown,
            avg_total_trades, // Add aggregated metrics
            median_win_rate,  // Add aggregated metrics
        );
        debug!("Aggregated Walk-Forward Result: {}", final_result);
        Ok(final_result)
    }

    /**
    Run a backtest using the *final* trained model on all available data.
    NOTE: This result is likely optimistic as the model has seen all the data during training.
          Use walk-forward validation results for more realistic performance estimates.

    ## Arguments
    * `interval_dataset` - The interval dataset.
    * `config` - The configuration.

    ## Returns
    The result of the backtest on all data.
    */
    #[instrument(skip(self, interval_dataset, config))]
    pub fn backtest_on_all_seen_data(
        &self,
        interval_dataset: &IntervalData,
        config: &KryptoConfig,
    ) -> Result<AlgorithmResult, KryptoError> {
        info!("Running backtest on all seen data (using final trained model)...");
        let ds = interval_dataset.get_symbol_dataset(&self.settings)?;

        if ds.is_empty() {
            return Err(KryptoError::InsufficientData {
                got: 0,
                required: 1,
                context: format!(
                    "Dataset empty for full backtest (Symbol: {}, Interval: {})",
                    self.settings.symbol,
                    interval_dataset.interval()
                ),
            });
        }

        Self::validate_data(ds.get_features(), ds.get_labels())?;

        let predictions = predict(&self.pls, ds.get_features())?;
        // Use run_simulation to get metrics
        let simulation_output =
            TestData::run_simulation(&self.settings.symbol, &predictions, ds.get_candles(), config)?;

        // Create AlgorithmResult using the metrics from SimulationOutput
        let result = AlgorithmResult::new(
            simulation_output.metrics.monthly_return,
            simulation_output.metrics.accuracy,
            simulation_output.metrics.sharpe_ratio,
            simulation_output.metrics.max_drawdown,
            simulation_output.metrics.total_trades, // Add metric
            simulation_output.metrics.win_rate,     // Add metric
        );
        info!(
            "Backtest on all seen data result: {} | Final Cash: ${:.2}",
            result, simulation_output.metrics.final_cash
        );
        Ok(result)
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Algorithm: ({}) | Validation Result: ({})", // Display validation result
            self.settings, self.result
        )
    }
}

// --- Algorithm Settings ---
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

    pub fn all(symbols: &[String], max_n: usize, max_depth: usize) -> Vec<Self> {
        if max_n == 0 || max_depth == 0 {
            return vec![];
        }
        symbols
            .iter()
            .flat_map(|symbol| {
                (1..=max_n).flat_map(move |n| {
                    (1..=max_depth).map(move |depth| Self::new(n, depth, symbol))
                })
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

// --- Algorithm Result (Expanded) ---
#[derive(Debug, Clone, PartialEq)]
pub struct AlgorithmResult {
    pub monthly_return: f64,
    pub accuracy: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub total_trades: u32, // Added
    pub win_rate: f64,     // Added
                           // Add others as needed (e.g., sortino, profit factor)
}

impl AlgorithmResult {
    pub fn new(
        monthly_return: f64,
        accuracy: f64,
        sharpe_ratio: f64,
        max_drawdown: f64,
        total_trades: u32, // Added
        win_rate: f64,     // Added
    ) -> Self {
        Self {
            // Sanitize results
            monthly_return: if monthly_return.is_finite() {
                monthly_return
            } else {
                -1.0
            },
            accuracy: if accuracy.is_finite() { accuracy } else { 0.0 },
            sharpe_ratio: if sharpe_ratio.is_finite() {
                sharpe_ratio
            } else {
                -999.0
            }, // Use a distinct low value
            // Apply clippy suggestion: use clamp
            max_drawdown: if max_drawdown.is_finite() {
                max_drawdown.clamp(0.0, 1.0)
            } else {
                1.0
            },
            total_trades,
            win_rate: if win_rate.is_finite() { win_rate } else { 0.0 },
        }
    }
}

impl fmt::Display for AlgorithmResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "M_Ret: {:.2}% | Acc: {:.1}% | Sharpe: {:.2} | MaxDD: {:.1}% | Trades: {} | WinRate: {:.1}%",
            self.monthly_return * 100.0,
            self.accuracy * 100.0,
            self.sharpe_ratio,
            self.max_drawdown * 100.0,
            self.total_trades,
            self.win_rate * 100.0,
        )
    }
}
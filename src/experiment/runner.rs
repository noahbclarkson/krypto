//! Experiment runner - orchestrates the backtest loop.
//!
//! The runner coordinates:
//! 1. Loading configuration
//! 2. Fetching/preparing data
//! 3. Running validation splits
//! 4. Optimizing strategy parameters (if configured)
//! 5. Evaluating and comparing results
//! 6. Persisting outputs and manifest

use anyhow::{bail, Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use polars::prelude::*;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::algo::optimization::{OptimizableStrategy, StrategyParams};
use crate::algo::strategies::{
    AdaptiveMaCrossover, BollingerReversion, DynamicTrend, LeadLagStrategy, MacdTrend, ObvTrend,
    PriceMomentum, RelativeStrengthStrat, RsiMeanReversion, VolatilitySqueeze,
};
use crate::algo::SignalGenerator;
use crate::backtest::engine::{BacktestResult, Backtester};
use crate::config::{DataSplit, ExperimentConfig, RuntimeConfig};
use crate::data::loader::DataLoader;
use crate::experiment::manifest::{OutputFile, OutputFileType, SplitResult};
use crate::experiment::{BacktestMetrics, ResultsSummary, RunManifest};
use crate::features::indicators::FeatureEngine;

/// Internal struct to track split information during experiment runs.
#[derive(Debug, Clone)]
struct SplitInfo {
    split_index: usize,
    symbol: String,
    train_range: (usize, usize),
    test_range: (usize, usize),
}

/// Experiment runner orchestrates the full backtest workflow.
pub struct ExperimentRunner {
    config: ExperimentConfig,
    runtime: Option<RuntimeConfig>,
    manifest: RunManifest,
    output_dir: PathBuf,
    /// Loaded data for backtesting (populated after load_data() or run())
    data: Option<DataFrame>,
}

impl ExperimentRunner {
    /// Create a new experiment runner from configuration.
    pub fn new(config: ExperimentConfig) -> Result<Self> {
        config.validate()?;

        let run_id = config.run_id();
        let output_dir = config.output.base_dir.join(&run_id);

        // Create output directory
        std::fs::create_dir_all(&output_dir)
            .with_context(|| format!("Failed to create output directory: {:?}", output_dir))?;

        // Serialize config for manifest
        let config_json = serde_json::to_value(&config)?;

        let manifest = RunManifest::new(run_id, config_json);

        Ok(Self {
            config,
            runtime: None,
            manifest,
            output_dir,
            data: None,
        })
    }

    /// Run the full experiment pipeline.
    pub fn run(&mut self) -> Result<ResultsSummary> {
        info!("Starting experiment: {}", self.config.name);
        info!("Output directory: {:?}", self.output_dir);

        let symbols = self.config.data.symbols.clone();
        info!(
            "Running experiment for {} symbol(s): {:?}",
            symbols.len(),
            symbols
        );

        // Collect results across all symbols
        let mut all_train_results = Vec::new();
        let mut all_test_results = Vec::new();
        let mut split_infos = Vec::new();

        // Iterate over ALL configured symbols
        for symbol in &symbols {
            info!("Processing symbol: {}", symbol);

            // Phase 1: Load and prepare data for this symbol
            let raw_data = self
                .load_data_for_symbol(symbol)
                .with_context(|| format!("Failed to load data for symbol {}", symbol))?;

            // Phase 2: Compute features
            info!("Computing technical features for {}", symbol);
            let data = FeatureEngine::add_technicals(&raw_data, None)
                .with_context(|| format!("Failed to compute features for symbol {}", symbol))?;

            // Phase 3: Compute runtime config (splits) based on this symbol's data
            let total_candles = data.height();
            let runtime = RuntimeConfig::from_experiment(self.config.clone(), total_candles)?;

            info!(
                "Loaded {} candles for {}, {} validation splits",
                total_candles,
                symbol,
                runtime.splits.len()
            );

            // Phase 4: Run backtests across all splits for this symbol
            for split in &runtime.splits {
                let (train_result, test_result) = self.run_split(&data, split)?;
                all_train_results.push(train_result);
                all_test_results.push(test_result);
                split_infos.push(SplitInfo {
                    split_index: split.index,
                    symbol: symbol.clone(),
                    train_range: split.train_range,
                    test_range: split.test_range,
                });
            }
        }

        // Store runtime for later use (use last symbol's runtime for reference)
        self.runtime = Some(RuntimeConfig::from_experiment(
            self.config.clone(),
            all_train_results.len(),
        )?);

        // Phase 5: Aggregate and evaluate results across all symbols
        let summary = self.aggregate_results(&all_train_results, &all_test_results, &split_infos)?;

        // Phase 6: Save outputs
        self.save_outputs(&summary)?;

        // Phase 7: Complete manifest
        self.manifest.complete(summary.clone());
        self.save_manifest()?;

        info!(
            "Experiment completed successfully across {} symbol(s)",
            symbols.len()
        );
        Ok(summary)
    }

    /// Run a backtest on already-loaded data.
    ///
    /// This method allows running backtests without going through the full
    /// experiment pipeline (data loading, feature computation, etc.).
    /// The input DataFrame should already have technical features computed.
    ///
    /// # Arguments
    /// * `data` - DataFrame with OHLCV data and technical features
    /// * `train_range` - (start, end) indices for training period
    /// * `test_range` - (start, end) indices for testing period
    ///
    /// # Returns
    /// A tuple of (train_result, test_result) BacktestResult structs
    pub fn run_backtest(
        &self,
        data: &DataFrame,
        train_range: (usize, usize),
        test_range: (usize, usize),
    ) -> Result<(BacktestResult, BacktestResult)> {
        // Create backtester with config
        let backtester = Backtester::new(
            self.config.sizing.initial_capital,
            self.config.costs.fee_pct,
            self.config.costs.slippage_bps,
        );

        // Slice data for train and test
        let train_df = data.slice(train_range.0 as i64, train_range.1 - train_range.0);
        let test_df = data.slice(test_range.0 as i64, test_range.1 - test_range.0);

        // Get trailing stop and take profit from config
        let trailing_stop = self.config.sizing.trailing_stop_pct;
        let take_profit = self.config.sizing.take_profit_pct;

        // Run strategy-specific backtest
        self.run_strategy_backtest(&backtester, &train_df, &test_df, trailing_stop, take_profit)
    }

    /// Run a backtest on a single DataFrame using the configured strategy.
    ///
    /// This is a convenience method for running a single backtest without
    /// train/test splits.
    ///
    /// # Arguments
    /// * `data` - DataFrame with OHLCV data and technical features
    ///
    /// # Returns
    /// BacktestResult with full metrics
    pub fn run_single_backtest(&self, data: &DataFrame) -> Result<BacktestResult> {
        // Create backtester with config
        let backtester = Backtester::new(
            self.config.sizing.initial_capital,
            self.config.costs.fee_pct,
            self.config.costs.slippage_bps,
        );

        let trailing_stop = self.config.sizing.trailing_stop_pct;
        let take_profit = self.config.sizing.take_profit_pct;

        // Get strategy and generate signals
        let strategy_type = self.config.strategy.strategy_type.to_lowercase();
        let params = self.parse_strategy_params()?;

        // Generate signals based on strategy type
        let signals = self.generate_signals(&strategy_type, &params, data)?;

        // Run backtest
        backtester
            .run(data, &signals, trailing_stop, take_profit)
            .with_context(|| "Backtest failed")
    }

    /// Run a backtest using stored data with a specified strategy.
    ///
    /// This is a convenience method that runs a backtest on previously loaded data
    /// (via `load_data()` or after `run()`) using the specified strategy name.
    ///
    /// # Arguments
    /// * `strategy_name` - Name of the strategy to use (e.g., "dynamic_trend", "rsi_reversion")
    ///
    /// # Returns
    /// BacktestResult with full metrics
    pub fn run_backtest_with_strategy(&mut self, strategy_name: &str) -> Result<BacktestResult> {
        // Get loaded data or return error
        let data = self.data.as_ref()
            .context("No data loaded. Call load_data() or run() first.")?;

        // Create backtester with default settings from config
        let backtester = Backtester::with_defaults(self.config.sizing.initial_capital);

        let trailing_stop = self.config.sizing.trailing_stop_pct;
        let take_profit = self.config.sizing.take_profit_pct;

        // Parse strategy params from config
        let params = self.parse_strategy_params()?;

        // Generate signals for the specified strategy
        let signals = self.generate_signals(strategy_name, &params, data)?;

        // Run and return the backtest result
        backtester
            .run(data, &signals, trailing_stop, take_profit)
            .with_context(|| format!("Backtest failed for strategy '{}'", strategy_name))
    }

    /// Load data for backtesting without running the full experiment pipeline.
    ///
    /// This method loads and prepares data (computes features) for subsequent
    /// backtests via `run_backtest()`.
    ///
    /// # Arguments
    /// * `symbol` - Trading symbol to load (e.g., "BTCUSDT")
    ///
    /// # Returns
    /// Reference to the loaded DataFrame
    pub fn load_data(&mut self, symbol: &str) -> Result<&DataFrame> {
        info!("Loading data for symbol: {}", symbol);

        // Load raw data
        let raw_data = self.load_data_for_symbol(symbol)?;

        // Compute features
        info!("Computing technical features for {}", symbol);
        let data = FeatureEngine::add_technicals(&raw_data, None)
            .with_context(|| format!("Failed to compute features for symbol {}", symbol))?;

        // Store for later use
        self.data = Some(data);

        Ok(self.data.as_ref().unwrap())
    }

    /// Generate signals for a given strategy and data.
    fn generate_signals(
        &self,
        strategy_type: &str,
        params: &StrategyParams,
        data: &DataFrame,
    ) -> Result<Series> {
        match strategy_type {
            "dynamic_trend" | "dynamictrend" => {
                let mut strategy = DynamicTrend::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            "relative_strength" | "relativestrength" => {
                let mut strategy = RelativeStrengthStrat::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            "bollinger_reversion" | "bollingerreversion" => {
                let mut strategy = BollingerReversion::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            "atr_breakout" | "atrbreakout" => {
                let mut strategy = crate::algo::strategies::AtrBreakout::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            "volatility_squeeze" | "volatilitysqueeze" => {
                let mut strategy = VolatilitySqueeze::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            "lead_lag" | "leadlag" | "lead_lag_arb" => {
                let mut strategy = LeadLagStrategy::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            "obv_trend" | "obvtrend" => {
                let mut strategy = ObvTrend::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            "macd_trend" | "macdtrend" => {
                let mut strategy = MacdTrend::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            "rsi_reversion" | "rsireversion" => {
                let mut strategy = RsiMeanReversion::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            "price_momentum" | "pricemomentum" => {
                let mut strategy = PriceMomentum::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            "adaptive_ma_cross" | "adaptivemacross" | "adaptive_ma_crossover" => {
                let mut strategy = AdaptiveMaCrossover::new();
                if !params.params.is_empty() {
                    strategy.set_params(params);
                }
                strategy.predict(data)
            }
            _ => bail!(
                "Unknown strategy type '{}'. Supported: dynamic_trend, relative_strength, bollinger_reversion, atr_breakout, volatility_squeeze, lead_lag, obv_trend, macd_trend, rsi_reversion, price_momentum, adaptive_ma_cross",
                strategy_type
            ),
        }
        .with_context(|| format!("Failed to generate signals for strategy {}", strategy_type))
    }

    /// Load data for a specific symbol.
    fn load_data_for_symbol(&self, symbol: &str) -> Result<DataFrame> {
        info!(
            "Loading data from {} for symbol: {}",
            self.config.data.source, symbol
        );

        let source = self.config.data.source.to_lowercase();
        if source != "binance" {
            bail!(
                "Unsupported data source '{}'. Currently supported: binance",
                self.config.data.source
            );
        }

        let total_candles = self.resolve_lookback_candles()?;
        info!(
            "Fetching {} {} candles for {} from Binance",
            total_candles, self.config.data.interval, symbol
        );

        let df = self.fetch_binance_data(symbol, &self.config.data.interval, total_candles)?;

        if df.height() == 0 {
            bail!("Data loader returned no data for {}", symbol);
        }

        Ok(df)
    }

    fn resolve_lookback_candles(&self) -> Result<u32> {
        if let Some(lookback) = self.config.data.lookback_candles {
            let bounded = lookback.min(u32::MAX as usize) as u32;
            if lookback > u32::MAX as usize {
                warn!(
                    "lookback_candles={} exceeds u32::MAX; clamped to {}",
                    lookback,
                    u32::MAX
                );
            }
            return Ok(bounded.max(1));
        }

        let start = DateTime::parse_from_rfc3339(&self.config.data.start_date)
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|_| {
                NaiveDate::parse_from_str(&self.config.data.start_date, "%Y-%m-%d")
                    .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
            })
            .with_context(|| {
                format!(
                    "Invalid start_date '{}'. Use RFC3339 or YYYY-MM-DD",
                    self.config.data.start_date
                )
            })?;

        let end = match &self.config.data.end_date {
            Some(end_str) => DateTime::parse_from_rfc3339(end_str)
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|_| {
                    NaiveDate::parse_from_str(end_str, "%Y-%m-%d")
                        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                })
                .with_context(|| {
                    format!("Invalid end_date '{}'. Use RFC3339 or YYYY-MM-DD", end_str)
                })?,
            None => Utc::now(),
        };

        if end <= start {
            bail!("end_date must be after start_date");
        }

        let interval_secs = interval_to_seconds(&self.config.data.interval).ok_or_else(|| {
            anyhow::anyhow!("Unsupported interval '{}'.", self.config.data.interval)
        })?;

        let secs = (end - start).num_seconds().max(interval_secs as i64);
        let estimated = ((secs as f64 / interval_secs as f64).ceil() as usize).max(1);
        let bounded = estimated.min(u32::MAX as usize) as u32;

        if estimated > u32::MAX as usize {
            warn!(
                "Resolved candle count {} exceeds u32::MAX; clamped to {}",
                estimated,
                u32::MAX
            );
        }

        Ok(bounded)
    }

    fn fetch_binance_data(
        &self,
        symbol: &str,
        interval: &str,
        total_candles: u32,
    ) -> Result<DataFrame> {
        let loader = DataLoader::new(None, None);

        let fut = async move { loader.fetch_data(symbol, interval, total_candles).await };

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(fut)
            }
        }
    }

    /// Run backtest for a single train/test split.
    fn run_split(
        &self,
        data: &DataFrame,
        split: &DataSplit,
    ) -> Result<(BacktestResult, BacktestResult)> {
        info!(
            "Running split {}: train [{}, {}), test [{}, {})",
            split.index,
            split.train_range.0,
            split.train_range.1,
            split.test_range.0,
            split.test_range.1
        );

        // Create backtester with config
        let backtester = Backtester::new(
            self.config.sizing.initial_capital,
            self.config.costs.fee_pct,
            self.config.costs.slippage_bps,
        );

        // Slice data for train and test
        let train_df = data.slice(split.train_range.0 as i64, split.train_range.1 - split.train_range.0);
        let test_df = data.slice(split.test_range.0 as i64, split.test_range.1 - split.test_range.0);

        // Get trailing stop and take profit from config
        let trailing_stop = self.config.sizing.trailing_stop_pct;
        let take_profit = self.config.sizing.take_profit_pct;

        // Run strategy-specific backtest
        let (train_result, test_result) = self.run_strategy_backtest(
            &backtester,
            &train_df,
            &test_df,
            trailing_stop,
            take_profit,
        )?;

        info!(
            "Split {} results - Train: {} trades, {:.2}% return, {:.2} Sharpe",
            split.index,
            train_result.total_trades,
            train_result.total_return_pct,
            train_result.sharpe_ratio
        );

        info!(
            "Split {} results - Test: {} trades, {:.2}% return, {:.2} Sharpe",
            split.index,
            test_result.total_trades,
            test_result.total_return_pct,
            test_result.sharpe_ratio
        );

        Ok((train_result, test_result))
    }

    /// Run backtest with the configured strategy.
    fn run_strategy_backtest(
        &self,
        backtester: &Backtester,
        train_df: &DataFrame,
        test_df: &DataFrame,
        trailing_stop: f64,
        take_profit: f64,
    ) -> Result<(BacktestResult, BacktestResult)> {
        let strategy_type = self.config.strategy.strategy_type.to_lowercase();
        let params = self.parse_strategy_params()?;

        info!("Running strategy: {} with {} params", strategy_type, params.params.len());

        // Match on strategy type and create/configure/run strategy
        // Using concrete types to allow for OptimizableStrategy trait usage
        match strategy_type.as_str() {
            "dynamic_trend" | "dynamictrend" => {
                let mut strategy = DynamicTrend::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            "relative_strength" | "relativestrength" => {
                let mut strategy = RelativeStrengthStrat::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            "bollinger_reversion" | "bollingerreversion" => {
                let mut strategy = BollingerReversion::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            "atr_breakout" | "atrbreakout" => {
                let mut strategy = crate::algo::strategies::AtrBreakout::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            "volatility_squeeze" | "volatilitysqueeze" => {
                let mut strategy = VolatilitySqueeze::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            "lead_lag" | "leadlag" | "lead_lag_arb" => {
                let mut strategy = LeadLagStrategy::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            "obv_trend" | "obvtrend" => {
                let mut strategy = ObvTrend::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            "macd_trend" | "macdtrend" => {
                let mut strategy = MacdTrend::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            "rsi_reversion" | "rsireversion" => {
                let mut strategy = RsiMeanReversion::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            "price_momentum" | "pricemomentum" => {
                let mut strategy = PriceMomentum::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            "adaptive_ma_cross" | "adaptivemacross" | "adaptive_ma_crossover" => {
                let mut strategy = AdaptiveMaCrossover::new();
                if !params.params.is_empty() {
                    strategy.set_params(&params);
                }
                self.execute_backtest(backtester, train_df, test_df, trailing_stop, take_profit, &strategy)
            }
            _ => bail!(
                "Unknown strategy type '{}'. Supported: dynamic_trend, relative_strength, bollinger_reversion, atr_breakout, volatility_squeeze, lead_lag, obv_trend, macd_trend, rsi_reversion, price_momentum, adaptive_ma_cross",
                self.config.strategy.strategy_type
            ),
        }
    }

    /// Execute backtest with a strategy that implements SignalGenerator.
    fn execute_backtest<S: SignalGenerator>(
        &self,
        backtester: &Backtester,
        train_df: &DataFrame,
        test_df: &DataFrame,
        trailing_stop: f64,
        take_profit: f64,
        strategy: &S,
    ) -> Result<(BacktestResult, BacktestResult)> {
        // Validate features
        self.validate_features(train_df, strategy.name())?;
        self.validate_features(test_df, strategy.name())?;

        // Generate signals
        let train_signals = strategy.predict(train_df)
            .with_context(|| format!("Failed to generate signals for strategy {} on train data", strategy.name()))?;

        let test_signals = strategy.predict(test_df)
            .with_context(|| format!("Failed to generate signals for strategy {} on test data", strategy.name()))?;

        // Run backtests
        let train_result = backtester.run(train_df, &train_signals, trailing_stop, take_profit)
            .with_context(|| "Train backtest failed")?;

        let test_result = backtester.run(test_df, &test_signals, trailing_stop, take_profit)
            .with_context(|| "Test backtest failed")?;

        Ok((train_result, test_result))
    }

    /// Parse strategy params from config JSON into StrategyParams.
    fn parse_strategy_params(&self) -> Result<StrategyParams> {
        let params_json = &self.config.strategy.params;
        let mut params = StrategyParams::new();

        if params_json.is_null() {
            return Ok(params);
        }

        if let Some(obj) = params_json.as_object() {
            for (key, value) in obj {
                let param_value = if let Some(n) = value.as_f64() {
                    n
                } else if let Some(n) = value.as_i64() {
                    n as f64
                } else if let Some(n) = value.as_u64() {
                    n as f64
                } else {
                    warn!(
                        "Skipping param '{}' with non-numeric value: {:?}",
                        key, value
                    );
                    continue;
                };
                params.params.insert(key.clone(), param_value);
            }
        }

        Ok(params)
    }

    /// Validate that required features exist in the DataFrame.
    fn validate_features(&self, df: &DataFrame, strategy_name: &str) -> Result<()> {
        // Common required columns
        let required = vec!["open", "high", "low", "close", "volume"];

        for col in &required {
            if df.column(col).is_err() {
                bail!(
                    "Required column '{}' missing from DataFrame for strategy '{}'",
                    col,
                    strategy_name
                );
            }
        }

        // Check for commonly-used technical indicators
        let optional_indicators = vec!["rsi", "atr", "macd", "ema_20", "ema_50"];

        for col in &optional_indicators {
            if df.column(col).is_err() {
                warn!(
                    "Optional indicator '{}' not available for strategy '{}'. Some strategies may not function optimally.",
                    col, strategy_name
                );
            }
        }

        Ok(())
    }

    /// Aggregate results across all splits.
    fn aggregate_results(
        &self,
        train_results: &[BacktestResult],
        test_results: &[BacktestResult],
        split_infos: &[SplitInfo],
    ) -> Result<ResultsSummary> {
        if train_results.is_empty() {
            bail!("No results to aggregate");
        }

        // Find best test result
        let best_idx = test_results
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.sharpe_ratio.partial_cmp(&b.sharpe_ratio).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        let best = BacktestMetrics::from(test_results[best_idx].clone());

        // Compute averages
        let avg_trades: f64 = test_results
            .iter()
            .map(|r| r.total_trades as f64)
            .sum::<f64>()
            / test_results.len() as f64;
        let avg_win_rate: f64 =
            test_results.iter().map(|r| r.win_rate).sum::<f64>() / test_results.len() as f64;
        let avg_sharpe: f64 =
            test_results.iter().map(|r| r.sharpe_ratio).sum::<f64>() / test_results.len() as f64;
        let avg_return: f64 = test_results.iter().map(|r| r.total_return_pct).sum::<f64>()
            / test_results.len() as f64;
        let avg_dd: f64 = test_results.iter().map(|r| r.max_drawdown_pct).sum::<f64>()
            / test_results.len() as f64;

        let average = BacktestMetrics {
            total_trades: avg_trades as usize,
            win_rate: avg_win_rate,
            profit_factor: 0.0,
            total_return_pct: avg_return,
            max_drawdown_pct: avg_dd,
            sharpe_ratio: avg_sharpe,
            kelly_fraction: 0.0,
            total_fees_paid: 0.0,
            // V3 extended metrics - compute averages
            sortino_ratio: test_results.iter().map(|r| r.sortino_ratio).sum::<f64>() / test_results.len() as f64,
            calmar_ratio: test_results.iter().map(|r| r.calmar_ratio).sum::<f64>() / test_results.len() as f64,
            avg_trade_duration_bars: test_results.iter().map(|r| r.avg_trade_duration_bars).sum::<f64>() / test_results.len() as f64,
            max_consecutive_wins: test_results.iter().map(|r| r.max_consecutive_wins).max().unwrap_or(0),
            max_consecutive_losses: test_results.iter().map(|r| r.max_consecutive_losses).max().unwrap_or(0),
            avg_win_pct: test_results.iter().map(|r| r.avg_win_pct).sum::<f64>() / test_results.len() as f64,
            avg_loss_pct: test_results.iter().map(|r| r.avg_loss_pct).sum::<f64>() / test_results.len() as f64,
            largest_win_pct: test_results.iter().map(|r| r.largest_win_pct).sum::<f64>() / test_results.len() as f64,
            largest_loss_pct: test_results.iter().map(|r| r.largest_loss_pct).sum::<f64>() / test_results.len() as f64,
        };

        // Find worst (by drawdown)
        let worst_idx = test_results
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.max_drawdown_pct.partial_cmp(&b.max_drawdown_pct).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        let worst = BacktestMetrics::from(test_results[worst_idx].clone());

        // Compute robustness
        let train_sharpe = train_results
            .get(best_idx)
            .map(|r: &BacktestResult| r.sharpe_ratio)
            .unwrap_or(0.0);
        let test_sharpe = test_results
            .get(best_idx)
            .map(|r: &BacktestResult| r.sharpe_ratio)
            .unwrap_or(0.0);
        let robustness = if train_sharpe > 0.0 {
            Some(test_sharpe / train_sharpe)
        } else {
            None
        };

        // Build split results
        let split_results: Vec<SplitResult> = train_results
            .iter()
            .zip(test_results.iter())
            .zip(split_infos.iter())
            .map(|((train, test), info)| SplitResult {
                split_index: info.split_index,
                symbol: info.symbol.clone(),
                train_metrics: BacktestMetrics::from(train.clone()),
                test_metrics: BacktestMetrics::from(test.clone()),
                train_range: info.train_range,
                test_range: info.test_range,
                equity_curve: Some(test.equity_curve.clone()),
            })
            .collect();

        Ok(ResultsSummary {
            best,
            average,
            worst,
            combinations_tested: train_results.len(),
            combinations_passed: train_results
                .iter()
                .filter(|r| r.sharpe_ratio > 0.0)
                .count(),
            train_metrics: train_results
                .get(best_idx)
                .map(|r: &BacktestResult| BacktestMetrics::from(r.clone())),
            test_metrics: test_results
                .get(best_idx)
                .map(|r: &BacktestResult| BacktestMetrics::from(r.clone())),
            robustness,
            split_results,
        })
    }

    /// Save experiment outputs.
    fn save_outputs(&mut self, summary: &ResultsSummary) -> Result<()> {
        // Save metrics JSON
        let metrics_path = self.output_dir.join("metrics.json");
        let metrics_json = serde_json::to_string_pretty(summary)?;
        std::fs::write(&metrics_path, &metrics_json)?;

        self.manifest.add_output(OutputFile {
            file_type: OutputFileType::Metrics,
            path: PathBuf::from("metrics.json"),
            size_bytes: metrics_json.len() as u64,
            description: "Aggregated backtest metrics".to_string(),
        });

        // Save equity curve if requested
        if self.config.output.save_equity_curve {
            // Find the best split's equity curve
            if let Some(best_split) = summary.split_results.iter().find(|s| {
                summary.best.sharpe_ratio > 0.0 &&
                (s.test_metrics.sharpe_ratio - summary.best.sharpe_ratio).abs() < 0.001
            }) {
                if let Some(curve) = &best_split.equity_curve {
                    let curve_path = self.output_dir.join("equity_curve.csv");
                    let mut csv = String::from("bar,equity\n");
                    for (i, eq) in curve.iter().enumerate() {
                        csv.push_str(&format!("{},{}\n", i, eq));
                    }
                    let bytes = csv.len() as u64;
                    std::fs::write(&curve_path, &csv)?;

                    self.manifest.add_output(OutputFile {
                        file_type: OutputFileType::EquityCurve,
                        path: PathBuf::from("equity_curve.csv"),
                        size_bytes: bytes,
                        description: format!(
                            "Equity curve for {} (split {})",
                            best_split.symbol, best_split.split_index
                        ),
                    });
                }
            }
        }

        // Save config snapshot
        let config_path = self.output_dir.join("config.json");
        let config_json = serde_json::to_string_pretty(&self.config)?;
        std::fs::write(&config_path, &config_json)?;

        self.manifest.add_output(OutputFile {
            file_type: OutputFileType::Custom,
            path: PathBuf::from("config.json"),
            size_bytes: config_json.len() as u64,
            description: "Configuration snapshot".to_string(),
        });

        Ok(())
    }

    /// Save the run manifest.
    fn save_manifest(&self) -> Result<()> {
        let manifest_path = self.output_dir.join("manifest.json");
        self.manifest.save(&manifest_path)?;
        Ok(())
    }

    /// Get the output directory for this run.
    pub fn output_dir(&self) -> &PathBuf {
        &self.output_dir
    }

    /// Get the run manifest.
    pub fn manifest(&self) -> &RunManifest {
        &self.manifest
    }
}

fn interval_to_seconds(interval: &str) -> Option<u64> {
    match interval {
        "1m" => Some(60),
        "3m" => Some(3 * 60),
        "5m" => Some(5 * 60),
        "15m" => Some(15 * 60),
        "30m" => Some(30 * 60),
        "1h" => Some(60 * 60),
        "2h" => Some(2 * 60 * 60),
        "4h" => Some(4 * 60 * 60),
        "6h" => Some(6 * 60 * 60),
        "8h" => Some(8 * 60 * 60),
        "12h" => Some(12 * 60 * 60),
        "1d" => Some(24 * 60 * 60),
        "3d" => Some(3 * 24 * 60 * 60),
        "1w" => Some(7 * 24 * 60 * 60),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CLI Entry Point Support
// ─────────────────────────────────────────────────────────────────────────────

/// Run an experiment from a config file.
pub fn run_from_config(path: &PathBuf) -> Result<ResultsSummary> {
    // Note: YAML support not yet implemented - use JSON
    let config = ExperimentConfig::from_json(path)?;

    let mut runner = ExperimentRunner::new(config)?;
    runner.run()
}

/// List all experiment runs in the output directory.
pub fn list_runs(base_dir: &PathBuf) -> Result<Vec<RunManifest>> {
    let mut manifests = Vec::new();

    if !base_dir.exists() {
        return Ok(manifests);
    }

    for entry in std::fs::read_dir(base_dir)? {
        let entry = entry?;
        let manifest_path = entry.path().join("manifest.json");

        if manifest_path.exists() {
            match RunManifest::load(&manifest_path) {
                Ok(m) => manifests.push(m),
                Err(e) => warn!("Failed to load manifest {:?}: {}", manifest_path, e),
            }
        }
    }

    // Sort by start time, newest first
    manifests.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    Ok(manifests)
}

/// Compare two experiment runs.
pub fn compare_runs(manifest_a: &RunManifest, manifest_b: &RunManifest) -> String {
    let comparison = manifest_a.compare(manifest_b);

    let mut report = format!("Comparison: {} vs {}\n", comparison.run_a, comparison.run_b);
    report.push_str(&"=".repeat(60));
    report.push('\n');

    if let Some(sharpe) = comparison.sharpe_diff {
        report.push_str(&format!("Sharpe ratio diff: {:.4}\n", sharpe));
    }

    if let Some(ret) = comparison.return_diff {
        report.push_str(&format!("Return diff: {:.2}%\n", ret));
    }

    if let Some(dd) = comparison.drawdown_diff {
        report.push_str(&format!("Drawdown diff: {:.2}%\n", dd));
    }

    if !comparison.config_diff.is_empty() {
        report.push_str("\nConfig differences:\n");
        for diff in comparison.config_diff {
            report.push_str(&format!("  - {}\n", diff));
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ExperimentConfig;

    #[test]
    fn interval_mapping_supports_common_binance_intervals() {
        assert_eq!(interval_to_seconds("1h"), Some(3600));
        assert_eq!(interval_to_seconds("4h"), Some(14_400));
        assert_eq!(interval_to_seconds("1d"), Some(86_400));
        assert_eq!(interval_to_seconds("bogus"), None);
    }

    #[test]
    fn resolves_lookback_from_config_or_dates() {
        let mut config = ExperimentConfig::example();
        config.data.lookback_candles = Some(1234);
        let runner = ExperimentRunner::new(config).expect("runner should build");
        assert_eq!(runner.resolve_lookback_candles().unwrap(), 1234);

        let mut config2 = ExperimentConfig::example();
        config2.data.lookback_candles = None;
        config2.data.interval = "1h".to_string();
        config2.data.start_date = "2024-01-01".to_string();
        config2.data.end_date = Some("2024-01-02".to_string());
        let runner2 = ExperimentRunner::new(config2).expect("runner should build");
        assert_eq!(runner2.resolve_lookback_candles().unwrap(), 24);
    }

    /// Integration test: verify data flow from DataFrame -> features -> strategy -> backtest
    #[test]
    fn test_data_flow_integration() {
        // Create a mock OHLCV DataFrame with 100 candles
        let n = 100;
        let base_price = 100.0;
        let mut closes = Vec::with_capacity(n);
        let mut opens = Vec::with_capacity(n);
        let mut highs = Vec::with_capacity(n);
        let mut lows = Vec::with_capacity(n);
        let mut volumes = Vec::with_capacity(n);
        let mut times = Vec::with_capacity(n);

        for i in 0..n {
            let price = base_price + (i as f64 * 0.5);
            let variation = (i as f64 % 10.0) * 0.1;
            closes.push(price);
            opens.push(price - variation);
            highs.push(price + variation + 0.5);
            lows.push(price - variation - 0.5);
            volumes.push(1000.0 + (i as f64 * 10.0));
            times.push(chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap() + chrono::Duration::hours(i as i64));
        }

        let raw_df = df!(
            "time" => times,
            "open" => opens.clone(),
            "high" => highs.clone(),
            "low" => lows.clone(),
            "close" => closes.clone(),
            "volume" => volumes.clone()
        ).expect("Failed to create test DataFrame");

        // Step 1: Add technical features
        let df_with_features = FeatureEngine::add_technicals(&raw_df, None)
            .expect("Failed to compute features");

        // Verify features were added
        assert!(df_with_features.column("rsi").is_ok(), "RSI should be computed");
        assert!(df_with_features.column("atr").is_ok(), "ATR should be computed");
        assert!(df_with_features.column("macd").is_ok(), "MACD should be computed");

        // Step 2: Create a simple strategy
        let strategy = DynamicTrend::new();

        // Step 3: Generate signals
        let signals = strategy.predict(&df_with_features)
            .expect("Failed to generate signals");

        // Verify signals were generated
        assert_eq!(signals.len(), n, "Should have signal for each candle");

        // Step 4: Run backtest
        let backtester = Backtester::new(10_000.0, 0.001, 5.0);
        let result = backtester.run(&df_with_features, &signals, 0.05, 0.0)
            .expect("Backtest should run successfully");

        // Verify backtest result structure
        assert!(result.final_equity > 0.0, "Final equity should be positive");
        assert!(result.equity_curve.len() == n, "Equity curve should match data length");
    }

    /// Test that strategy params are correctly parsed from config
    #[test]
    fn test_strategy_params_parsing() {
        let mut config = ExperimentConfig::example();
        config.strategy.strategy_type = "dynamic_trend".to_string();
        config.strategy.params = serde_json::json!({
            "ema_fast": 30,
            "ema_slow": 100,
            "rsi_filter": 45.0
        });

        let runner = ExperimentRunner::new(config).expect("Runner should build");
        let params = runner.parse_strategy_params().expect("Should parse params");

        assert_eq!(params.get("ema_fast", 0.0), 30.0);
        assert_eq!(params.get("ema_slow", 0.0), 100.0);
        assert_eq!(params.get("rsi_filter", 0.0), 45.0);
    }

    /// Test that feature validation catches missing columns
    #[test]
    fn test_feature_validation_missing_columns() {
        // Create DataFrame missing required columns
        let df = df!(
            "time" => &[chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap()],
            "close" => &[100.0_f64]
        ).expect("Failed to create test DataFrame");

        let config = ExperimentConfig::example();
        let runner = ExperimentRunner::new(config).expect("Runner should build");

        let result = runner.validate_features(&df, "test_strategy");
        assert!(result.is_err(), "Should fail with missing columns");

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("open"), "Error should mention missing 'open' column");
    }

    /// Test that split data slicing works correctly
    #[test]
    fn test_data_split_slicing() {
        let n = 100;
        let closes: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let times: Vec<chrono::NaiveDateTime> = (0..n)
            .map(|i| {
                chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    + chrono::Duration::hours(i as i64)
            })
            .collect();

        let df = df!(
            "time" => times,
            "close" => closes
        ).expect("Failed to create test DataFrame");

        let split = DataSplit {
            index: 0,
            train_range: (0, 60),
            test_range: (60, 100),
            purge_range: None,
        };

        let train_df = df.slice(split.train_range.0 as i64, split.train_range.1 - split.train_range.0);
        let test_df = df.slice(split.test_range.0 as i64, split.test_range.1 - split.test_range.0);

        assert_eq!(train_df.height(), 60, "Train split should have 60 rows");
        assert_eq!(test_df.height(), 40, "Test split should have 40 rows");

        let train_close = train_df.column("close").unwrap().f64().unwrap();
        assert_eq!(train_close.get(0).unwrap(), 0.0, "Train should start at index 0");
        assert_eq!(train_close.get(59).unwrap(), 59.0, "Train should end at index 59");
    }

    /// Integration test: verify run_backtest method works with loaded data
    #[test]
    fn test_run_backtest_on_loaded_data() {
        // Create a mock OHLCV DataFrame with 100 candles
        let n = 100;
        let base_price = 100.0;
        let mut closes = Vec::with_capacity(n);
        let mut opens = Vec::with_capacity(n);
        let mut highs = Vec::with_capacity(n);
        let mut lows = Vec::with_capacity(n);
        let mut volumes = Vec::with_capacity(n);
        let mut times = Vec::with_capacity(n);

        for i in 0..n {
            let price = base_price + (i as f64 * 0.5);
            let variation = (i as f64 % 10.0) * 0.1;
            closes.push(price);
            opens.push(price - variation);
            highs.push(price + variation + 0.5);
            lows.push(price - variation - 0.5);
            volumes.push(1000.0 + (i as f64 * 10.0));
            times.push(chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap() + chrono::Duration::hours(i as i64));
        }

        let raw_df = df!(
            "time" => times,
            "open" => opens,
            "high" => highs,
            "low" => lows,
            "close" => closes,
            "volume" => volumes
        ).expect("Failed to create test DataFrame");

        // Add technical features
        let df_with_features = FeatureEngine::add_technicals(&raw_df, None)
            .expect("Failed to compute features");

        // Create experiment runner
        let mut config = ExperimentConfig::example();
        config.strategy.strategy_type = "dynamic_trend".to_string();
        config.sizing.initial_capital = 10_000.0;
        config.sizing.trailing_stop_pct = 0.05;
        config.sizing.take_profit_pct = 0.10;

        let runner = ExperimentRunner::new(config).expect("Runner should build");

        // Run backtest using the new method
        let (train_result, test_result) = runner
            .run_backtest(&df_with_features, (0, 70), (70, 100))
            .expect("Backtest should run successfully");

        // Verify train result
        assert!(train_result.final_equity > 0.0, "Train equity should be positive");
        assert!(train_result.equity_curve.len() == 70, "Train equity curve length should match");
        assert!(train_result.sharpe_ratio.is_finite(), "Sharpe ratio should be finite");

        // Verify test result
        assert!(test_result.final_equity > 0.0, "Test equity should be positive");
        assert!(test_result.equity_curve.len() == 30, "Test equity curve length should match");

        // Verify V3 extended metrics are populated
        assert!(train_result.sortino_ratio.is_finite(), "Sortino ratio should be finite");
        assert!(train_result.calmar_ratio.is_finite(), "Calmar ratio should be finite");
        assert!(train_result.avg_trade_duration_bars >= 0.0, "Avg duration should be non-negative");

        println!("Train result: {} trades, {:.2}% return, {:.2} Sharpe, {:.2} Sortino",
                 train_result.total_trades, train_result.total_return_pct, 
                 train_result.sharpe_ratio, train_result.sortino_ratio);
        println!("Test result: {} trades, {:.2}% return, {:.2} Sharpe, {:.2} Sortino",
                 test_result.total_trades, test_result.total_return_pct,
                 test_result.sharpe_ratio, test_result.sortino_ratio);
    }

    /// Integration test: verify run_single_backtest method works
    #[test]
    fn test_run_single_backtest() {
        // Create a mock OHLCV DataFrame
        let n = 50;
        let closes: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let opens: Vec<f64> = closes.iter().map(|&c| c - 0.5).collect();
        let highs: Vec<f64> = closes.iter().map(|&c| c + 0.5).collect();
        let lows: Vec<f64> = closes.iter().map(|&c| c - 1.0).collect();
        let volumes: Vec<f64> = vec![1000.0; n];
        let times: Vec<chrono::NaiveDateTime> = (0..n)
            .map(|i| {
                chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    + chrono::Duration::hours(i as i64)
            })
            .collect();

        let raw_df = df!(
            "time" => times,
            "open" => opens,
            "high" => highs,
            "low" => lows,
            "close" => closes,
            "volume" => volumes
        ).expect("Failed to create test DataFrame");

        // Add features
        let df_with_features = FeatureEngine::add_technicals(&raw_df, None)
            .expect("Failed to compute features");

        // Create runner
        let mut config = ExperimentConfig::example();
        config.strategy.strategy_type = "price_momentum".to_string();
        config.sizing.initial_capital = 5_000.0;

        let runner = ExperimentRunner::new(config).expect("Runner should build");

        // Run single backtest
        let result = runner
            .run_single_backtest(&df_with_features)
            .expect("Single backtest should run");

        // Verify result
        assert!(result.final_equity > 0.0, "Final equity should be positive");
        assert_eq!(result.equity_curve.len(), n, "Equity curve should match data length");

        // Verify V3 metrics are present
        assert!(result.sortino_ratio.is_finite() || result.sortino_ratio == 0.0);
        assert!(result.calmar_ratio.is_finite() || result.calmar_ratio == 0.0);
        // max_consecutive_wins and max_consecutive_losses are u32, always >= 0

        println!("Single backtest: {} trades, {:.2}% return, equity ${:.2}",
                 result.total_trades, result.total_return_pct, result.final_equity);
    }

    /// Test that BacktestMetrics correctly captures V3 extended metrics
    #[test]
    fn test_backtest_metrics_v3_capture() {
        // Create a mock BacktestResult with V3 metrics
        let result = BacktestResult {
            total_trades: 10,
            win_rate: 60.0,
            profit_factor: 1.5,
            final_equity: 11_000.0,
            total_return_pct: 10.0,
            max_drawdown_pct: 5.0,
            sharpe_ratio: 1.2,
            kelly_fraction: 0.08,
            equity_curve: vec![10_000.0, 10_500.0, 11_000.0],
            total_fees_paid: 50.0,
            average_position_size: 1.0,
            // V3 extended metrics
            sortino_ratio: 1.8,
            calmar_ratio: 2.0,
            avg_trade_duration_bars: 5.5,
            max_consecutive_wins: 4,
            max_consecutive_losses: 2,
            avg_win_pct: 2.0,
            avg_loss_pct: 1.0,
            largest_win_pct: 3.5,
            largest_loss_pct: 1.5,
            trades: vec![],
        };

        // Convert to BacktestMetrics
        let metrics = BacktestMetrics::from(result);

        // Verify core metrics
        assert_eq!(metrics.total_trades, 10);
        assert_eq!(metrics.win_rate, 60.0);
        assert_eq!(metrics.total_return_pct, 10.0);
        assert_eq!(metrics.sharpe_ratio, 1.2);

        // Verify V3 extended metrics are captured
        assert!((metrics.sortino_ratio - 1.8).abs() < 0.001);
        assert!((metrics.calmar_ratio - 2.0).abs() < 0.001);
        assert!((metrics.avg_trade_duration_bars - 5.5).abs() < 0.001);
        assert_eq!(metrics.max_consecutive_wins, 4);
        assert_eq!(metrics.max_consecutive_losses, 2);
        assert!((metrics.avg_win_pct - 2.0).abs() < 0.001);
        assert!((metrics.avg_loss_pct - 1.0).abs() < 0.001);
        assert!((metrics.largest_win_pct - 3.5).abs() < 0.001);
        assert!((metrics.largest_loss_pct - 1.5).abs() < 0.001);
    }
}

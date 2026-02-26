//! Experiment configuration schema.
//!
//! Defines all parameters needed for reproducible backtesting.
//! Configuration is loaded from JSON files and validated at startup.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Root configuration for a backtest experiment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentConfig {
    /// Human-readable experiment name (used for output directory)
    pub name: String,

    /// Experiment description/purpose
    #[serde(default)]
    pub description: Option<String>,

    /// Data configuration
    pub data: DataConfig,

    /// Transaction cost configuration
    #[serde(default)]
    pub costs: CostConfig,

    /// Position sizing configuration
    #[serde(default)]
    pub sizing: SizingConfig,

    /// Validation configuration (walk-forward windows)
    pub validation: ValidationConfig,

    /// Evaluation metrics configuration
    #[serde(default)]
    pub metrics: MetricsConfig,

    /// Strategy configuration
    pub strategy: StrategyConfig,

    /// Output configuration
    #[serde(default)]
    pub output: OutputConfig,
}

impl ExperimentConfig {
    /// Load configuration from a JSON file.
    pub fn from_json(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_json::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Save configuration to a JSON file.
    pub fn to_json(&self, path: &PathBuf) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Validate configuration for logical consistency.
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            bail!("Experiment name cannot be empty");
        }

        if self.data.symbols.is_empty() {
            bail!("At least one symbol must be specified");
        }

        if self.validation.train_ratio + self.validation.test_ratio > 1.0 {
            bail!("train_ratio + test_ratio cannot exceed 1.0");
        }

        if self.sizing.kelly_fraction > 0.25 {
            bail!("Kelly fraction > 25% is excessively risky. Max recommended: 0.25");
        }

        Ok(())
    }

    /// Generate a unique run ID for this experiment.
    pub fn run_id(&self) -> String {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        format!("{}__{}", self.name, timestamp)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Data Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Data source configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataConfig {
    /// Exchange or data provider (e.g., "binance", "csv")
    pub source: String,

    /// Trading symbols (e.g., ["BTCUSDT", "ETHUSDT"])
    pub symbols: Vec<String>,

    /// Time interval (e.g., "1h", "4h", "1d")
    pub interval: String,

    /// Start date (ISO 8601 format)
    pub start_date: String,

    /// End date (ISO 8601 format), defaults to "now" if not specified
    #[serde(default)]
    pub end_date: Option<String>,

    /// Number of candles to fetch (alternative to date range)
    #[serde(default)]
    pub lookback_candles: Option<usize>,

    /// Cache directory for downloaded data
    #[serde(default = "default_cache_dir")]
    pub cache_dir: PathBuf,
}

fn default_cache_dir() -> PathBuf {
    PathBuf::from("./data/cache")
}

// ─────────────────────────────────────────────────────────────────────────────
// Transaction Costs
// ─────────────────────────────────────────────────────────────────────────────

/// Transaction cost configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostConfig {
    /// Trading fee percentage (default: 0.1% = 0.001)
    #[serde(default = "default_fee_pct")]
    pub fee_pct: f64,

    /// Slippage in basis points (default: 5 bps = 0.05%)
    #[serde(default = "default_slippage_bps")]
    pub slippage_bps: f64,

    /// Include funding rates for margin/leverage (future enhancement)
    #[serde(default)]
    pub include_funding: bool,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            fee_pct: default_fee_pct(),
            slippage_bps: default_slippage_bps(),
            include_funding: false,
        }
    }
}

fn default_fee_pct() -> f64 {
    0.001
} // 0.1%
fn default_slippage_bps() -> f64 {
    5.0
} // 5 bps

// ─────────────────────────────────────────────────────────────────────────────
// Position Sizing
// ─────────────────────────────────────────────────────────────────────────────

/// Position sizing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizingConfig {
    /// Initial capital for backtest
    #[serde(default = "default_initial_capital")]
    pub initial_capital: f64,

    /// Fraction of Kelly criterion to use (0.0 = fixed size, 1.0 = full Kelly)
    #[serde(default)]
    pub kelly_fraction: f64,

    /// Fixed position size as fraction of equity (if kelly_fraction = 0)
    #[serde(default = "default_position_fraction")]
    pub position_fraction: f64,

    /// Maximum position size as fraction of equity (cap)
    #[serde(default = "default_max_position")]
    pub max_position_fraction: f64,

    /// Trailing stop percentage (e.g., 0.05 = 5%)
    #[serde(default = "default_trailing_stop")]
    pub trailing_stop_pct: f64,
}

impl Default for SizingConfig {
    fn default() -> Self {
        Self {
            initial_capital: default_initial_capital(),
            kelly_fraction: 0.0,
            position_fraction: default_position_fraction(),
            max_position_fraction: default_max_position(),
            trailing_stop_pct: default_trailing_stop(),
        }
    }
}

fn default_initial_capital() -> f64 {
    10_000.0
}
fn default_position_fraction() -> f64 {
    1.0
} // 100% of equity per trade
fn default_max_position() -> f64 {
    1.0
}
fn default_trailing_stop() -> f64 {
    0.05
} // 5%

// ─────────────────────────────────────────────────────────────────────────────
// Validation (Walk-Forward)
// ─────────────────────────────────────────────────────────────────────────────

/// Validation configuration for out-of-sample testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// Method: "simple_split", "walk_forward", "cpcv"
    #[serde(default = "default_validation_method")]
    pub method: String,

    /// Fraction of data for training (default: 0.6)
    #[serde(default = "default_train_ratio")]
    pub train_ratio: f64,

    /// Fraction of data for testing (default: 0.4)
    #[serde(default = "default_test_ratio")]
    pub test_ratio: f64,

    /// Number of walk-forward windows (if method = "walk_forward")
    #[serde(default = "default_n_windows")]
    pub n_windows: usize,

    /// Purge gap between train and test (in candles) to prevent leakage
    #[serde(default)]
    pub purge_gap: usize,

    /// Embargo period after test set (in candles)
    #[serde(default)]
    pub embargo: usize,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            method: default_validation_method(),
            train_ratio: default_train_ratio(),
            test_ratio: default_test_ratio(),
            n_windows: default_n_windows(),
            purge_gap: 0,
            embargo: 0,
        }
    }
}

fn default_validation_method() -> String {
    "simple_split".to_string()
}
fn default_train_ratio() -> f64 {
    0.6
}
fn default_test_ratio() -> f64 {
    0.4
}
fn default_n_windows() -> usize {
    5
}

// ─────────────────────────────────────────────────────────────────────────────
// Evaluation Metrics
// ─────────────────────────────────────────────────────────────────────────────

/// Evaluation metrics configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Primary metric for strategy selection
    #[serde(default = "default_primary_metric")]
    pub primary_metric: String,

    /// Minimum thresholds for consideration
    #[serde(default)]
    pub thresholds: MetricThresholds,

    /// Additional metrics to compute and report
    #[serde(default = "default_additional_metrics")]
    pub additional: Vec<String>,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            primary_metric: default_primary_metric(),
            thresholds: MetricThresholds::default(),
            additional: default_additional_metrics(),
        }
    }
}

fn default_primary_metric() -> String {
    "sharpe_ratio".to_string()
}
fn default_additional_metrics() -> Vec<String> {
    vec![
        "win_rate".to_string(),
        "profit_factor".to_string(),
        "max_drawdown".to_string(),
        "total_return".to_string(),
        "kelly_fraction".to_string(),
    ]
}

/// Minimum thresholds for strategy consideration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricThresholds {
    /// Minimum Sharpe ratio
    #[serde(default)]
    pub min_sharpe: Option<f64>,

    /// Minimum profit factor
    #[serde(default)]
    pub min_profit_factor: Option<f64>,

    /// Minimum win rate (percentage)
    #[serde(default)]
    pub min_win_rate: Option<f64>,

    /// Maximum drawdown (percentage)
    #[serde(default)]
    pub max_drawdown: Option<f64>,

    /// Minimum number of trades
    #[serde(default)]
    pub min_trades: Option<usize>,

    /// Robustness ratio (test_sharpe / train_sharpe)
    #[serde(default)]
    pub min_robustness: Option<f64>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Strategy Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Strategy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Strategy type (e.g., "dynamic_trend", "atr_breakout", "ensemble")
    pub strategy_type: String,

    /// Strategy-specific parameters (flexible key-value map)
    #[serde(default)]
    pub params: serde_json::Value,

    /// Optimization configuration (if strategy is optimizable)
    #[serde(default)]
    pub optimization: Option<OptimizationConfig>,
}

/// Optimization configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationConfig {
    /// Number of parameter combinations to try
    #[serde(default = "default_n_iterations")]
    pub n_iterations: usize,

    /// Optimization method: "random", "grid", "bayesian"
    #[serde(default = "default_opt_method")]
    pub method: String,

    /// Parameter ranges for optimization
    #[serde(default)]
    pub param_ranges: std::collections::HashMap<String, ParamRange>,
}

fn default_n_iterations() -> usize {
    100
}
fn default_opt_method() -> String {
    "random".to_string()
}

/// Parameter range for optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamRange {
    /// Parameter type: "float", "int", "categorical"
    #[serde(rename = "type")]
    pub param_type: String,

    /// Minimum value (for numeric types)
    #[serde(default)]
    pub min: Option<f64>,

    /// Maximum value (for numeric types)
    #[serde(default)]
    pub max: Option<f64>,

    /// Possible values (for categorical type)
    #[serde(default)]
    pub values: Option<Vec<String>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Output Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Output configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Base output directory for experiment results
    #[serde(default = "default_output_dir")]
    pub base_dir: PathBuf,

    /// Save equity curves
    #[serde(default = "default_true")]
    pub save_equity_curve: bool,

    /// Save trade log
    #[serde(default = "default_true")]
    pub save_trades: bool,

    /// Generate plots
    #[serde(default)]
    pub generate_plots: bool,

    /// Verbose output
    #[serde(default)]
    pub verbose: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            base_dir: default_output_dir(),
            save_equity_curve: true,
            save_trades: true,
            generate_plots: false,
            verbose: false,
        }
    }
}

fn default_output_dir() -> PathBuf {
    PathBuf::from("./experiments")
}
fn default_true() -> bool {
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Example Configuration
// ─────────────────────────────────────────────────────────────────────────────

impl ExperimentConfig {
    /// Create an example configuration for documentation/testing.
    pub fn example() -> Self {
        Self {
            name: "btc_trend_following".to_string(),
            description: Some("Basic trend following strategy on BTC/USDT".to_string()),
            data: DataConfig {
                source: "binance".to_string(),
                symbols: vec!["BTCUSDT".to_string()],
                interval: "1h".to_string(),
                start_date: "2024-01-01".to_string(),
                end_date: None,
                lookback_candles: Some(5000),
                cache_dir: PathBuf::from("./data/cache"),
            },
            costs: CostConfig::default(),
            sizing: SizingConfig::default(),
            validation: ValidationConfig::default(),
            metrics: MetricsConfig::default(),
            strategy: StrategyConfig {
                strategy_type: "dynamic_trend".to_string(),
                params: serde_json::json!({
                    "ema_fast": 20,
                    "ema_slow": 50,
                    "rsi_period": 14,
                    "rsi_threshold": 30.0
                }),
                optimization: Some(OptimizationConfig {
                    n_iterations: 50,
                    method: "random".to_string(),
                    param_ranges: std::collections::HashMap::new(),
                }),
            },
            output: OutputConfig::default(),
        }
    }
}

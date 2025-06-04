use std::{
    fmt,
    path::{Path, PathBuf},
};

use binance::{account::Account, api::Binance};
use chrono::NaiveDate;
use directories::ProjectDirs;
use serde::{de, Deserialize, Deserializer, Serialize};
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt as _, AsyncWriteExt as _, BufReader},
};
use tracing::{debug, error, info, instrument, warn};

use crate::{data::interval::Interval, error::KryptoError};

// --- Defaults ---
const fn default_max_n() -> usize {
    25
}
const fn default_max_depth() -> usize {
    20
}
const fn default_generation_limit() -> u64 {
    100
}
const fn default_population_size() -> usize {
    100
}
const fn default_mutation_rate() -> f64 {
    0.05
} // Slightly increased default
const fn default_selection_ratio() -> f64 {
    0.7
}
const fn default_num_individuals_per_parents() -> usize {
    2
}
const fn default_reinsertion_ratio() -> f64 {
    0.7
}
const fn default_margin() -> f64 {
    1.0
}
fn default_symbols() -> Vec<String> {
    vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()]
}
fn default_intervals() -> Vec<Interval> {
    vec![Interval::OneHour, Interval::FourHours]
} // Changed default
fn default_cross_validations() -> usize {
    10
} // Note: Now used for Walk-Forward splits
const fn default_start_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2023, 1, 1).unwrap()
} // Earlier start date
fn default_technicals() -> Vec<String> {
    vec![
        "RSI".to_string(),
        "Fast Stochastic".to_string(),
        "Bollinger Bands".to_string(),
        "Candlestick Ratio".to_string(),
    ]
} // Added more defaults
fn default_cache_enabled() -> bool {
    true
}
fn default_cache_dir() -> Option<PathBuf> {
    ProjectDirs::from("com", "Krypto", "Krypto").map(|dirs| dirs.cache_dir().to_path_buf())
}
const fn default_backtest_margin_start() -> f64 {
    1.0
}
const fn default_backtest_margin_end() -> f64 {
    10.0
}
const fn default_backtest_margin_step() -> f64 {
    1.0
}
const fn default_trade_loop_wait_seconds() -> u64 {
    600
}
const fn default_trade_qty_percentage() -> f64 {
    0.85
} // Default % of max borrowable
const fn default_trade_stop_loss_percentage() -> Option<f64> {
    None
} // e.g. 0.05 for 5%
const fn default_trade_take_profit_percentage() -> Option<f64> {
    None
} // e.g. 0.10 for 10%
const fn default_walk_forward_train_ratio() -> f64 {
    0.7
} // 70% train, 30% test for each split

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct KryptoConfig {
    #[serde(
        rename = "start-date",
        default = "default_start_date",
        deserialize_with = "parse_date"
    )]
    pub start_date: NaiveDate,
    #[serde(rename = "api-key", default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(
        rename = "api-secret",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub api_secret: Option<String>,
    #[serde(default = "default_symbols")]
    pub symbols: Vec<String>,
    #[serde(default = "default_intervals")]
    pub intervals: Vec<Interval>,
    #[serde(rename = "cross-validations", default = "default_cross_validations")]
    pub cross_validations: usize, // For Walk-Forward: Number of splits
    #[serde(default)]
    pub fee: Option<f64>, // Trading fee per trade (e.g., 0.001 for 0.1%)

    // --- GA Parameters ---
    #[serde(rename = "max-n", default = "default_max_n")]
    pub max_n: usize, // Max PLS components
    #[serde(rename = "max-depth", default = "default_max_depth")]
    pub max_depth: usize, // Max lookback depth for features
    #[serde(rename = "generation-limit", default = "default_generation_limit")]
    pub generation_limit: u64,
    #[serde(rename = "population-size", default = "default_population_size")]
    pub population_size: usize,
    #[serde(rename = "mutation-rate", default = "default_mutation_rate")]
    pub mutation_rate: f64,
    #[serde(rename = "selection-ratio", default = "default_selection_ratio")]
    pub selection_ratio: f64,
    #[serde(
        rename = "num-individuals-per-parents",
        default = "default_num_individuals_per_parents"
    )]
    pub num_individuals_per_parents: usize,
    #[serde(rename = "reinsertion-ratio", default = "default_reinsertion_ratio")]
    pub reinsertion_ratio: f64,

    #[serde(default = "default_technicals")]
    pub technicals: Vec<String>, // Available technicals for GA
    #[serde(default = "default_margin")]
    pub margin: f64, // Default margin for backtest/trade (can be overridden)

    #[serde(default)]
    pub mission: Mission,

    // --- Caching ---
    #[serde(rename = "cache-enabled", default = "default_cache_enabled")]
    pub cache_enabled: bool,
    #[serde(rename = "cache-dir", default = "default_cache_dir")]
    pub cache_dir: Option<PathBuf>,

    // --- Backtesting Specific ---
    #[serde(
        rename = "backtest-margin-start",
        default = "default_backtest_margin_start"
    )]
    pub backtest_margin_start: f64,
    #[serde(
        rename = "backtest-margin-end",
        default = "default_backtest_margin_end"
    )]
    pub backtest_margin_end: f64,
    #[serde(
        rename = "backtest-margin-step",
        default = "default_backtest_margin_step"
    )]
    pub backtest_margin_step: f64,
    #[serde(
        rename = "walk-forward-train-ratio",
        default = "default_walk_forward_train_ratio"
    )]
    pub walk_forward_train_ratio: f64, // Ratio of data used for training in each WF split

    // --- Trading Specific ---
    #[serde(
        rename = "trade-loop-wait-seconds",
        default = "default_trade_loop_wait_seconds"
    )]
    pub trade_loop_wait_seconds: u64,
    #[serde(
        rename = "trade-qty-percentage",
        default = "default_trade_qty_percentage"
    )]
    pub trade_qty_percentage: f64, // Percentage of max borrowable/available balance to trade
    #[serde(
        rename = "trade-stop-loss-percentage",
        default = "default_trade_stop_loss_percentage"
    )]
    pub trade_stop_loss_percentage: Option<f64>, // e.g., 0.05 for 5% stop loss from entry
    #[serde(
        rename = "trade-take-profit-percentage",
        default = "default_trade_take_profit_percentage"
    )]
    pub trade_take_profit_percentage: Option<f64>, // e.g., 0.10 for 10% take profit from entry
}

impl Default for KryptoConfig {
    fn default() -> Self {
        Self {
            start_date: default_start_date(),
            api_key: None,
            api_secret: None,
            symbols: default_symbols(),
            intervals: default_intervals(),
            cross_validations: default_cross_validations(),
            fee: None, // Default to None, let Binance provide if not set? Or set a common default like 0.001?
            max_n: default_max_n(),
            max_depth: default_max_depth(),
            generation_limit: default_generation_limit(),
            population_size: default_population_size(),
            mutation_rate: default_mutation_rate(),
            selection_ratio: default_selection_ratio(),
            num_individuals_per_parents: default_num_individuals_per_parents(),
            reinsertion_ratio: default_reinsertion_ratio(),
            technicals: default_technicals(),
            margin: default_margin(),
            mission: Mission::default(),
            cache_enabled: default_cache_enabled(),
            cache_dir: default_cache_dir(),
            backtest_margin_start: default_backtest_margin_start(),
            backtest_margin_end: default_backtest_margin_end(),
            backtest_margin_step: default_backtest_margin_step(),
            walk_forward_train_ratio: default_walk_forward_train_ratio(),
            trade_loop_wait_seconds: default_trade_loop_wait_seconds(),
            trade_qty_percentage: default_trade_qty_percentage(),
            trade_stop_loss_percentage: default_trade_stop_loss_percentage(),
            trade_take_profit_percentage: default_trade_take_profit_percentage(),
        }
    }
}

impl KryptoConfig {
    /// Reads the configuration from a YAML file.
    ///
    /// If the file does not exist, it creates a default configuration file.
    ///
    /// # Arguments
    ///
    /// * `filename` - Optional path to the configuration file. Defaults to `config.yml` in the current dir.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `KryptoConfig` on success or an `Error` on failure.
    #[instrument(level = "info", skip(filename))]
    pub async fn read_config<P: AsRef<Path> + std::fmt::Debug>(
        filename: Option<P>,
    ) -> Result<Self, KryptoError> {
        let path = filename
            .map(|p| p.as_ref().to_path_buf())
            .unwrap_or(Path::new("config.yml").to_path_buf());

        info!(path = %path.display(), "Reading configuration");

        if !path.exists() {
            warn!(
                "Config file not found at {}. Creating default config.",
                path.display()
            );
            let default_config = KryptoConfig::default();
            let default_yaml = serde_yaml::to_string(&default_config)?;

            // Ensure parent directory exists
            if let Some(parent_dir) = path.parent() {
                fs::create_dir_all(parent_dir).await?;
            }

            let mut file = File::create(&path).await?;
            file.write_all(default_yaml.as_bytes()).await?;
            info!("Default configuration file created at {}", path.display());
            // Validate the default config before returning
            default_config.validate()?;
            return Ok(default_config);
        }

        let file = File::open(&path).await?;
        let mut reader = BufReader::new(file);
        let mut contents = String::new();
        reader.read_to_string(&mut contents).await?;
        let mut config: Self = serde_yaml::from_str(&contents)?;

        // Ensure cache dir exists if enabled
        if config.cache_enabled {
            if let Some(cache_dir) = &config.cache_dir {
                if !cache_dir.exists() {
                    info!("Creating cache directory: {}", cache_dir.display());
                    fs::create_dir_all(cache_dir)
                        .await
                        .map_err(|_| KryptoError::CacheDirError(cache_dir.clone()))?;
                }
            } else {
                warn!("Cache is enabled but cache directory could not be determined. Caching will be disabled.");
                config.cache_enabled = false;
            }
        }

        // Validate loaded config
        config.validate()?;

        // Check Binance connection if API keys are present
        if config.api_key.is_some() && config.api_secret.is_some() {
            info!("API keys found, attempting to connect to Binance...");
            let account: Account = config.get_binance();
            match account.get_account().await {
                Ok(_) => info!("Successfully connected to Binance account."),
                Err(e) => {
                    error!("Failed to verify Binance API keys: {}", e);
                    // Decide whether to error out or just warn
                    return Err(KryptoError::BinanceApiError(format!(
                        "Failed to verify API keys: {}",
                        e
                    )));
                }
            }
        } else {
            info!("No Binance API keys found in config. Trading functionality will be disabled.");
        }

        info!("Configuration loaded successfully: {}", config);
        Ok(config)
    }

    /// Validates the configuration parameters.
    pub fn validate(&self) -> Result<(), KryptoError> {
        if self.symbols.is_empty() {
            return Err(KryptoError::ConfigError(
                "Symbols list cannot be empty.".to_string(),
            ));
        }
        if self.intervals.is_empty() {
            return Err(KryptoError::ConfigError(
                "Intervals list cannot be empty.".to_string(),
            ));
        }
        if self.technicals.is_empty() {
            return Err(KryptoError::ConfigError(
                "Technicals list cannot be empty.".to_string(),
            ));
        }
        if self.max_n == 0
            || self.max_depth == 0
            || self.population_size == 0
            || self.generation_limit == 0
        {
            return Err(KryptoError::ConfigError("GA parameters (max_n, max_depth, population_size, generation_limit) must be greater than 0.".to_string()));
        }
        if !(0.0..=1.0).contains(&self.mutation_rate) {
            return Err(KryptoError::ConfigError(
                "Mutation rate must be between 0.0 and 1.0.".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.selection_ratio) {
            return Err(KryptoError::ConfigError(
                "Selection ratio must be between 0.0 and 1.0.".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.reinsertion_ratio) {
            return Err(KryptoError::ConfigError(
                "Reinsertion ratio must be between 0.0 and 1.0.".to_string(),
            ));
        }
        if self.num_individuals_per_parents == 0 {
            return Err(KryptoError::ConfigError(
                "Num individuals per parents must be greater than 0.".to_string(),
            ));
        }
        if self.cross_validations == 0 {
            return Err(KryptoError::ConfigError(
                "Cross-validations (walk-forward splits) must be greater than 0.".to_string(),
            ));
        }
        if !(0.0..1.0).contains(&self.walk_forward_train_ratio) {
            return Err(KryptoError::ConfigError(
                "Walk-forward train ratio must be between 0.0 and 1.0 (exclusive of 1.0)."
                    .to_string(),
            ));
        }
        if let Some(fee) = self.fee {
            if fee < 0.0 {
                return Err(KryptoError::ConfigError(
                    "Fee cannot be negative.".to_string(),
                ));
            }
        }
        if self.margin <= 0.0 {
            return Err(KryptoError::ConfigError(
                "Margin must be greater than 0.".to_string(),
            ));
        }
        if self.backtest_margin_start <= 0.0
            || self.backtest_margin_end <= 0.0
            || self.backtest_margin_step <= 0.0
        {
            return Err(KryptoError::ConfigError(
                "Backtest margin parameters must be greater than 0.".to_string(),
            ));
        }
        if self.backtest_margin_start > self.backtest_margin_end {
            return Err(KryptoError::ConfigError(
                "Backtest margin start cannot be greater than end.".to_string(),
            ));
        }
        if self.trade_loop_wait_seconds == 0 {
            return Err(KryptoError::ConfigError(
                "Trade loop wait seconds must be greater than 0.".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.trade_qty_percentage) {
            return Err(KryptoError::ConfigError(
                "Trade quantity percentage must be between 0.0 and 1.0.".to_string(),
            ));
        }
        if let Some(sl) = self.trade_stop_loss_percentage {
            if !(0.0..=1.0).contains(&sl) {
                return Err(KryptoError::ConfigError(
                    "Stop loss percentage must be between 0.0 and 1.0.".to_string(),
                ));
            }
        }
        if let Some(tp) = self.trade_take_profit_percentage {
            if !(0.0..=1.0).contains(&tp) {
                return Err(KryptoError::ConfigError(
                    "Take profit percentage must be between 0.0 and 1.0.".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Creates a Binance client using the provided API key and secret.
    #[instrument(level = "debug", skip(self))]
    pub fn get_binance<T: Binance>(&self) -> T {
        debug!("Creating Binance client instance");
        T::new(self.api_key.clone(), self.api_secret.clone())
    }
}

impl fmt::Display for KryptoConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Krypto Configuration:")?;
        writeln!(f, "  Mission: {:?}", self.mission)?;
        writeln!(f, "  Start Date: {}", self.start_date)?;
        writeln!(f, "  Symbols: {:?}", self.symbols)?;
        writeln!(f, "  Intervals: {:?}", self.intervals)?;
        writeln!(f, "  Technicals: {:?}", self.technicals)?;
        writeln!(f, "  Fee: {:?}", self.fee)?;
        writeln!(f, "  Default Margin: {}", self.margin)?;
        writeln!(f, "  Walk-Forward Splits: {}", self.cross_validations)?;
        writeln!(
            f,
            "  Walk-Forward Train Ratio: {}",
            self.walk_forward_train_ratio
        )?;
        writeln!(f, "  Cache Enabled: {}", self.cache_enabled)?;
        writeln!(
            f,
            "  Cache Dir: {:?}",
            self.cache_dir.as_ref().map(|p| p.display())
        )?;
        writeln!(f, "  GA Settings:")?;
        writeln!(f, "    Max PLS Components (n): {}", self.max_n)?;
        writeln!(f, "    Max Lookback Depth (d): {}", self.max_depth)?;
        writeln!(f, "    Generations: {}", self.generation_limit)?;
        writeln!(f, "    Population Size: {}", self.population_size)?;
        writeln!(f, "    Mutation Rate: {}", self.mutation_rate)?;
        writeln!(f, "    Selection Ratio: {}", self.selection_ratio)?;
        writeln!(
            f,
            "    Num Individuals/Parents: {}",
            self.num_individuals_per_parents
        )?;
        writeln!(f, "    Reinsertion Ratio: {}", self.reinsertion_ratio)?;
        writeln!(f, "  Backtest Settings:")?;
        writeln!(f, "    Margin Start: {}", self.backtest_margin_start)?;
        writeln!(f, "    Margin End: {}", self.backtest_margin_end)?;
        writeln!(f, "    Margin Step: {}", self.backtest_margin_step)?;
        writeln!(f, "  Trade Settings:")?;
        writeln!(f, "    Loop Wait (s): {}", self.trade_loop_wait_seconds)?;
        writeln!(f, "    Quantity (%): {}", self.trade_qty_percentage)?;
        writeln!(
            f,
            "    Stop Loss (%): {:?}",
            self.trade_stop_loss_percentage
        )?;
        writeln!(
            f,
            "    Take Profit (%): {:?}",
            self.trade_take_profit_percentage
        )?;
        write!(
            f,
            "  API Keys Present: {}",
            self.api_key.is_some() && self.api_secret.is_some()
        )
    }
}

// Helper to parse date, ensuring compatibility with serde
fn parse_date<'de, D>(deserializer: D) -> Result<NaiveDate, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    NaiveDate::parse_from_str(&s, "%Y-%m-%d").map_err(de::Error::custom)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mission {
    Trade,
    #[default]
    Optimise,
    Backtest,
}

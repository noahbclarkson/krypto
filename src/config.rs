use std::{
    fs::File,
    io::{BufReader, Write as _},
    path::Path,
};

use binance::{account::Account, api::Binance};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;
use tracing::{debug, error, info, instrument};

use crate::{data::interval::Interval, error::KryptoError};

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
    0.015
}

const fn default_margin() -> f64 {
    1.0
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct KryptoConfig {
    #[serde(rename = "start-date")]
    pub start_date: String,
    #[serde(rename = "api-key")]
    pub api_key: Option<String>,
    #[serde(rename = "api-secret")]
    pub api_secret: Option<String>,
    pub symbols: Vec<String>,
    pub intervals: Vec<Interval>,
    #[serde(rename = "cross-validations")]
    pub cross_validations: usize,
    pub fee: Option<f64>,
    #[serde(rename = "max-n", default = "default_max_n")]
    pub max_n: usize,
    #[serde(rename = "max-depth", default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(rename = "generation-limit", default = "default_generation_limit")]
    pub generation_limit: u64,
    #[serde(rename = "population-size", default = "default_population_size")]
    pub population_size: usize,
    #[serde(rename = "mutation-rate", default = "default_mutation_rate")]
    pub mutation_rate: f64,
    pub technicals: Vec<String>,
    #[serde(default = "default_margin")]
    pub margin: f64,
}

const DEFAULT_DATA: &str = r#"
start-date: "2024-01-01"
api-key: null
api-secret: null
symbols:
  - BTCUSDT
  - ETHUSDT
intervals:
    - 2h
    - 4h
cross-validations: 25
fee: 0.002
max-n: 25
max-depth: 20
technicals:
  - RSI
  - Fast Stochastic
"#;

impl Default for KryptoConfig {
    fn default() -> Self {
        Self {
            start_date: "2024-01-01".to_string(),
            api_key: None,
            api_secret: None,
            symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()],
            intervals: vec![Interval::TwoHours, Interval::FourHours],
            cross_validations: 25,
            fee: Some(0.001),
            max_n: default_max_n(),
            max_depth: default_max_depth(),
            generation_limit: default_generation_limit(),
            population_size: default_population_size(),
            mutation_rate: default_mutation_rate(),
            technicals: vec!["RSI".to_string(), "Fast Stochastic".to_string()],
            margin: default_margin(),
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
    /// * `filename` - Optional path to the configuration file.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `KryptoConfig` on success or an `Error` on failure.
    #[instrument(level = "info", skip(filename))]
    pub fn read_config<P: AsRef<Path>>(filename: Option<P>) -> Result<Self, KryptoError> {
        let path = filename
            .map(|p| p.as_ref().to_path_buf())
            .unwrap_or_else(|| Path::new("config.yml").to_path_buf());

        info!(path = %path.display(), "Reading configuration");

        if !path.exists() {
            info!(
                "Config file does not exist. Creating default config at {}",
                path.display()
            );
            let mut file = File::create(&path)?;
            file.write_all(DEFAULT_DATA.as_bytes())?;
            debug!("Default configuration file created");
            return Ok(KryptoConfig::default());
        }

        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        let config: Self = from_reader(reader)?;
        let account: Account = config.get_binance();
        if config.api_key.is_some() || config.api_secret.is_some() {
            let account_info = account.get_account().map_err(|e| {
                error!("Failed to get account info: {}", e);
                KryptoError::BinanceApiError(e.to_string())
            })?;
            for asset in account_info.balances {
                let free = asset.free.parse::<f64>().unwrap_or(0.0);
                let locked = asset.locked.parse::<f64>().unwrap_or(0.0);
                if free + locked > 0.0 {
                    info!(
                        "Asset: {}, Free: {}, Locked: {}",
                        asset.asset, asset.free, asset.locked
                    );
                }
            }
        }
        info!("Configuration loaded successfully");
        Ok(config)
    }

    /// Converts the start date to a `NaiveDate`.
    ///
    /// # Returns
    ///
    /// A `NaiveDate` representing the start date.
    ///
    /// # Errors
    ///
    /// Returns an error if the date cannot be parsed.
    pub fn start_date(&self) -> Result<NaiveDate, KryptoError> {
        let date = NaiveDate::parse_from_str(&self.start_date, "%Y-%m-%d")?;
        Ok(date)
    }

    /// Converts interval enums to their corresponding minutes.
    ///
    /// # Returns
    ///
    /// A vector of minutes for each interval.
    pub fn interval_minutes(&self) -> Vec<i64> {
        let minutes = self.intervals.iter().map(|i| i.to_minutes()).collect();
        minutes
    }

    // Creates a Binance client using the provided API key and secret.
    ///
    /// # Returns
    ///
    /// An instance of the Binance client.
    #[instrument(level = "debug", skip(self))]
    pub fn get_binance<T: Binance>(&self) -> T {
        debug!("Creating Binance client");
        T::new(self.api_key.clone(), self.api_secret.clone())
    }
}

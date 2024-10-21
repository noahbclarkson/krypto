// config.rs

use std::{
    fs::File,
    io::{BufReader, Write},
    path::Path,
};

use binance::api::Binance;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;
use tracing::{debug, error, info, instrument};

use crate::{algorithm_type::AlgorithmType, interval::Interval, KryptoError};

/// Configuration structure for the Krypto application.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct KryptoConfig {
    #[serde(rename = "start-date")]
    pub start_date: String,
    pub fee: f64,
    #[serde(rename = "api-key")]
    pub api_key: Option<String>,
    #[serde(rename = "api-secret")]
    pub api_secret: Option<String>,
    #[serde(rename = "pls-components")]
    pub pls_components: usize,
    pub tickers: Vec<String>,
    pub margin: f64,
    pub intervals: Vec<Interval>,
    pub split: f64,
    pub algorithms: Vec<AlgorithmType>,
}

const DEFAULT_DATA: &str = r#"---
start-date: "2023-01-01"
fee: 0.001
margin: 1.0
split: 0.8
pls-components: 3
tickers:
  - BTCUSDT
  - ETHUSDT
intervals:
  - 3m
  - 5m
algorithms:
  - RandomForest
  - PartialLeastSquares
  - RMatrix
"#;

impl Default for KryptoConfig {
    fn default() -> Self {
        KryptoConfig {
            start_date: "2023-01-01".to_string(),
            fee: 0.001,
            api_key: None,
            api_secret: None,
            pls_components: 3,
            tickers: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()],
            margin: 1.0,
            intervals: vec![Interval::ThreeMinutes, Interval::FiveMinutes],
            split: 0.8,
            algorithms: vec![
                AlgorithmType::RandomForest,
                AlgorithmType::PartialLeastSquares,
                AlgorithmType::RMatrix,
            ],
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
            let mut file = File::create(&path).map_err(|e| {
                error!(error = %e, "Failed to create config file");
                e
            })?;
            file.write_all(DEFAULT_DATA.as_bytes()).map_err(|e| {
                error!(error = %e, "Failed to write default config");
                e
            })?;
            info!("Default configuration file created");
            return Ok(KryptoConfig::default());
        }

        let file = File::open(&path).map_err(|e| {
            error!(error = %e, "Failed to open config file");
            e
        })?;
        let reader = BufReader::new(file);
        let config: Self = from_reader(reader).map_err(|e| {
            error!(error = %e, "Failed to parse config file");
            KryptoError::ConfigLoadError
        })?;
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
    #[instrument(level = "debug")]
    pub fn start_date(&self) -> Result<NaiveDate, KryptoError> {
        let date = NaiveDate::parse_from_str(&self.start_date, "%Y-%m-%d").map_err(|e| {
            error!(error = %e, "Failed to parse start date");
            KryptoError::ConfigLoadError
        })?;
        Ok(date)
    }

    /// Converts interval enums to their corresponding minutes.
    ///
    /// # Returns
    ///
    /// A vector of minutes for each interval.
    #[instrument(level = "debug")]
    pub fn interval_minutes(&self) -> Vec<i64> {
        let minutes = self.intervals.iter().map(|i| i.to_minutes()).collect();
        debug!(?minutes, "Converted intervals to minutes");
        minutes
    }

    /// Creates a Binance client using the provided API key and secret.
    ///
    /// # Returns
    ///
    /// An instance of the Binance client.
    #[instrument(level = "info", skip(self))]
    pub fn get_binance<T: Binance>(&self) -> T {
        info!("Creating Binance client");
        T::new(self.api_key.clone(), self.api_secret.clone())
    }
}

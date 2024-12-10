use std::{fmt, path::Path};

use binance::{account::Account, api::Binance};
use chrono::NaiveDate;
use serde::{de, Deserialize, Deserializer, Serialize};
use tokio::{
    fs::File,
    io::{AsyncReadExt as _, AsyncWriteExt as _, BufReader},
};
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

fn default_symbols() -> Vec<String> {
    vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()]
}

fn default_intervals() -> Vec<Interval> {
    vec![Interval::TwoHours, Interval::FourHours]
}

fn default_cross_validations() -> usize {
    10
}

const fn default_start_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
}

fn default_technicals() -> Vec<String> {
    vec!["RSI".to_string(), "Fast Stochastic".to_string()]
}

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
    pub cross_validations: usize,
    #[serde(default)]
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
    #[serde(default = "default_technicals")]
    pub technicals: Vec<String>,
    #[serde(default = "default_margin")]
    pub margin: f64,
    #[serde(default)]
    pub mission: Mission,
    #[serde(skip)]
    parsed_start_date: Option<NaiveDate>,
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
            fee: None,
            max_n: default_max_n(),
            max_depth: default_max_depth(),
            generation_limit: default_generation_limit(),
            population_size: default_population_size(),
            mutation_rate: default_mutation_rate(),
            technicals: default_technicals(),
            margin: default_margin(),
            mission: Mission::default(),
            parsed_start_date: None,
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
    pub async fn read_config<P: AsRef<Path>>(filename: Option<P>) -> Result<Self, KryptoError> {
        let path = filename
            .map(|p| p.as_ref().to_path_buf())
            .unwrap_or(Path::new("config.yml").to_path_buf());

        info!(path = %path.display(), "Reading configuration");

        if !path.exists() {
            info!(
                "Config file does not exist. Creating default config at {}",
                path.display()
            );
            let mut file = File::create(&path).await?;
            let default_config = serde_yaml::to_string(&KryptoConfig::default())?;
            file.write_all(default_config.as_bytes()).await?;
            debug!("Default configuration file created");
            return Ok(KryptoConfig::default());
        }

        let file = File::open(&path).await?;
        let mut reader = BufReader::new(file);
        let mut contents = String::new();
        reader.read_to_string(&mut contents).await?;
        let config: Self = serde_yaml::from_str(&contents)?;
        let account = config.get_binance::<Account>();
        if config.api_key.is_some() || config.api_secret.is_some() {
            if let Err(e) = account.get_account().await {
                error!("Failed to get account info: {}", e);
                return Err(KryptoError::BinanceApiError(e.to_string()));
            } else {
                info!("Successfully connected to Binance account.");
            }
        }
        info!("Configuration loaded successfully: {}", config);
        Ok(config)
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

impl fmt::Display for KryptoConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Start Date: {}\nSymbols: {:?}\nIntervals: {:?}\nCross Validations: {}\nFee: {:?}\nMax N: {}\nMax Depth: {}\nGeneration Limit: {}\nPopulation Size: {}\nMutation Rate: {}\nTechnicals: {:?}\nMargin: {}", self.start_date, self.symbols, self.intervals, self.cross_validations, self.fee, self.max_n, self.max_depth, self.generation_limit, self.population_size, self.mutation_rate, self.technicals, self.margin
        )
    }
}

fn parse_date<'de, D>(deserializer: D) -> Result<NaiveDate, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    NaiveDate::parse_from_str(&s, "%Y-%m-%d").map_err(de::Error::custom)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum Mission {
    #[serde(rename = "trade")]
    Trade,
    #[serde(rename = "optimise")]
    #[default]
    Optimise,
    #[serde(rename = "backtest")]
    Backtest,
}

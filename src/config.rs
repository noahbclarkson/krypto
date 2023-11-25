use std::{
    fs::File,
    io::{BufReader, Write},
    path::Path,
};

use binance_r_matrix::{HistoricalDataConfig, HistoricalDataConfigBuilder, Interval};
use getset::{Getters, Setters};
use r_matrix::{
    matricies::{ForestConfig, ForestConfigBuilder, SimpleConfigBuilder, SimpleConfig},
    rtest::{RTestConfig, RTestConfigBuilder}, math::NormalizationFunctionType,
};
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;

use crate::errors::ConfigurationError;

const DEFAULT_DATA: &str = r#"training-periods: 2000
testing-periods: 1000
interval: "15m"
depth: 3
margin: 1.0
tickers: 
    - "BTCBUSD"
    - "ETHBUSD"
"#;

#[derive(Debug, Serialize, Deserialize, Clone, Getters, Setters)]
#[getset(get = "pub", set = "pub")]
pub struct Config {
    #[serde(rename = "training-periods")]
    training_periods: usize,
    #[serde(rename = "testing-periods")]
    testing_periods: usize,
    interval: String,
    depth: usize,
    fee: Option<f64>,
    margin: f64,
    trees: Option<usize>,
    function: Option<String>,
    #[serde(rename = "function-multiplier")]
    function_multiplier: Option<f64>,
    seed: Option<u64>,
    #[serde(rename = "sampling-rate")]
    sampling_rate: Option<f32>,
    #[serde(rename = "min-score")]
    min_score: Option<f64>,
    tickers: Vec<String>,
    #[serde(rename = "api-key")]
    api_key: Option<String>,
    #[serde(rename = "api-secret")]
    api_secret: Option<String>,
}

impl Config {
    #[inline]
    pub async fn read_config(filename: Option<&str>) -> Result<Self, ConfigurationError> {
        let path = match filename {
            Some(filename) => Path::new(filename),
            None => Path::new("config.yml"),
        };

        if !path.exists() {
            let mut file = File::create(path)?;
            file.write_all(DEFAULT_DATA.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            return Err(ConfigurationError::FileNotFound);
        }

        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let config: Config = from_reader(reader)?;
        Ok(config)
    }

    #[inline]
    pub fn get_interval(&self) -> Interval {
        match self.interval.as_str() {
            "1m" => Interval::OneMinute,
            "3m" => Interval::ThreeMinutes,
            "5m" => Interval::FiveMinutes,
            "15m" => Interval::FifteenMinutes,
            "30m" => Interval::ThirtyMinutes,
            "1h" => Interval::OneHour,
            "2h" => Interval::TwoHours,
            "4h" => Interval::FourHours,
            "6h" => Interval::SixHours,
            "8h" => Interval::EightHours,
            "12h" => Interval::TwelveHours,
            "1d" => Interval::OneDay,
            "3d" => Interval::ThreeDays,
            "1w" => Interval::OneWeek,
            "1M" => Interval::OneMonth,
            _ => Interval::OneMinute,
        }
    }
}

impl From<Config> for HistoricalDataConfig {
    fn from(val: Config) -> Self {
        let interval = val.get_interval();
        HistoricalDataConfigBuilder::default()
            .tickers(val.tickers)
            .periods(val.training_periods + val.testing_periods)
            .interval(interval)
            .api_key(val.api_key)
            .api_secret(val.api_secret)
            .build()
            .unwrap()
    }
}

impl From<Config> for ForestConfig {
    fn from(val: Config) -> Self {
        ForestConfigBuilder::default()
            .depth(val.depth)
            .trees(val.trees.unwrap_or(100))
            .seed(val.seed.unwrap_or(0))
            .ending_position(val.training_periods)
            .max_samples((val.sampling_rate.unwrap_or(1.0) * val.training_periods as f32) as usize)
            .build()
            .unwrap()
    }
}

impl From<Config> for SimpleConfig {
    fn from(val: Config) -> Self {
        SimpleConfigBuilder::default()
            .depth(val.depth)
            .function(NormalizationFunctionType::from_string(val.function.as_deref().unwrap_or("Tanh")))
            .training_periods(val.training_periods)
            .function_multiplier(val.function_multiplier.unwrap_or(1.0))
            .build()
            .unwrap()
    }
}

impl From<Config> for RTestConfig {
    fn from(val: Config) -> Self {
        RTestConfigBuilder::default()
            .margin(val.margin)
            .starting_position(val.training_periods + val.depth)
            .min_change(val.min_score.unwrap_or(0.0))
            .margin(val.margin)
            .starting_cash(1000.0)
            .build()
            .unwrap()
    }
}

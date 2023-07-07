use std::{
    error::Error,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::Path,
};

use getset::{Getters, Setters};
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;

const DEFAULT_DATA: &str = r#"
periods: 2000
interval: "15m"
depth: 3
"#;

#[derive(Debug, Serialize, Deserialize, Clone, Getters, Setters)]
#[getset(get = "pub", set = "pub")]
pub struct Config {
    periods: usize,
    interval: String,
    depth: usize,
    fee: Option<f32>,
    #[serde(rename = "min-score")]
    min_score: Option<f32>,
    #[serde(rename = "api-key")]
    api_key: Option<String>,
    #[serde(rename = "api-secret")]
    api_secret: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            periods: 2000,
            interval: "15m".to_string(),
            depth: 3,
            fee: None,
            min_score: None,
            api_key: None,
            api_secret: None,
        }
    }
}

impl Config {
    pub async fn read_config() -> Result<Self, Box<dyn Error>> {
        let path = Path::new("config.yml");

        if !path.exists() {
            let mut file = File::create(path)?;
            file.write_all(DEFAULT_DATA.as_bytes())?;
            return Err(Box::new(ConfigurationError::FileNotFound));
        }

        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let config: Config = from_reader(reader).map_err(|err| Box::new(err) as Box<dyn Error>)?;
        Ok(config)
    }

    pub async fn read_tickers() -> Result<Vec<String>, Box<dyn Error>> {
        let path = Path::new("tickers.txt");

        if !path.exists() {
            return Err(Box::new(ConfigurationError::TickerFileNotFound));
        }

        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let tickers: Vec<String> = reader
            .lines()
            .map(|line| line.unwrap().to_uppercase().trim().to_string())
            .collect::<Vec<String>>();
        Ok(tickers)
    }

    pub fn interval_minutes(&self) -> Result<i64, Box<dyn std::error::Error>> {
        match self.interval.as_str() {
            "1m" => Ok(1),
            "3m" => Ok(3),
            "5m" => Ok(5),
            "15m" => Ok(15),
            "30m" => Ok(30),
            "1h" => Ok(60),
            "2h" => Ok(120),
            "4h" => Ok(240),
            "6h" => Ok(360),
            "8h" => Ok(480),
            "12h" => Ok(720),
            "1d" => Ok(1440),
            "3d" => Ok(4320),
            "1w" => Ok(10080),
            "1M" => Ok(43200),
            _ => Err(Box::new(ConfigurationError::IntervalError(
                self.interval.clone(),
            ))),
        }
    }
}

pub async fn load_configuration() -> Result<(Box<Config>, Vec<String>), Box<dyn Error>> {
    let tasks = futures::future::join(Config::read_config(), Config::read_tickers());
    let (config, tickers) = tasks.await;
    Ok((Box::new(config?), tickers?))
}

pub const DEFAULT_TICKERS: [&str; 4] = [
    "BTCBUSD", "ETHBUSD", "BNBBUSD", "ADAUSDT",
];

#[derive(thiserror::Error, Debug)]
pub enum ConfigurationError {
    #[error("Configuration file not found")]
    FileNotFound,
    #[error("Ticker file not found")]
    TickerFileNotFound,
    #[error("Interval (`{0}`) not supported")]
    IntervalError(String),
}

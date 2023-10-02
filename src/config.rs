use std::{
    error::Error,
    fs::File,
    io::{BufReader, Write},
    path::Path,
};

use binance::api::Binance;
use getset::{Getters, Setters};
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;

use crate::krypto_error::ConfigurationError;

const DEFAULT_DATA: &str = r#"
periods: 2000
interval: "15m"
tickers: 
    - "BTCBUSD"
    - "ETHBUSD"
"#;

#[derive(Debug, Serialize, Deserialize, Clone, Getters, Setters)]
#[getset(get = "pub", set = "pub")]
pub struct Config {
    periods: usize,
    intervals: Box<[String]>,
    fee: Option<f64>,
    leverage: f64,
    depth: usize,
    #[serde(rename = "min-score")]
    min_score: Option<f64>,
    tickers: Vec<String>,
    #[serde(rename = "api-key")]
    api_key: Option<String>,
    #[serde(rename = "api-secret")]
    api_secret: Option<String>,
    #[serde(skip)]
    depths: Vec<usize>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            periods: 2000,
            depth: 1,
            intervals: Box::from(["15m".to_string()]),
            depths: vec![1],
            leverage: 1.0,
            tickers: vec!["BTCBUSD".to_string(), "ETHBUSD".to_string()],
            fee: None,
            min_score: None,
            api_key: None,
            api_secret: None,
        }
    }
}

impl Config {
    #[inline]
    pub async fn read_config(filename: Option<&str>) -> Result<Box<Self>, Box<dyn Error>> {
        let path = match filename {
            Some(filename) => Path::new(filename),
            None => Path::new("config.yml"),
        };
        let path = path.canonicalize()?;

        if !path.exists() {
            let mut file = File::create(path)?;
            file.write_all(DEFAULT_DATA.as_bytes())?;
            return Err(Box::new(ConfigurationError::FileNotFound));
        }

        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut config: Config = from_reader(reader).map_err(|err| Box::new(err) as Box<dyn Error>)?;
        config.tickers.dedup();
        let mut depths = vec![(*config.depth(), config.interval_minutes(0))];
        for (i, _) in config.intervals().iter().enumerate().skip(1) {
            let last = depths.last().unwrap();
            let depth = last.0 * last.1 / config.interval_minutes(i);
            depths.push((depth, config.interval_minutes(i)));
        }
        config.depths = depths.into_iter().map(|(d, _)| d).collect();
        Ok(Box::new(config))
    }

    #[inline]
    pub fn interval_minutes(&self, index: usize) -> usize {
        match self.intervals[index].as_str() {
            "1m" => 1,
            "3m" => 3,
            "5m" => 5,
            "15m" => 15,
            "30m" => 30,
            "1h" => 60,
            "2h" => 120,
            "4h" => 240,
            "6h" => 360,
            "8h" => 480,
            "12h" => 360,
            "1d" => 1440,
            "3d" => 4320,
            "1w" => 10080,
            "1M" => 43200,
            _ => panic!("Interval not supported: {}", self.intervals[index]),
        }
    }

    pub fn get_binance<T: Binance>(&self) -> T {
        T::new(self.api_key.clone(), self.api_secret.clone())
    }
}

pub fn get_intervals() -> Vec<String> {
    vec![
        "1m".to_string(),
        "3m".to_string(),
        "5m".to_string(),
        "15m".to_string(),
        "30m".to_string(),
        "1h".to_string(),
        "2h".to_string(),
        "4h".to_string(),
        "6h".to_string(),
        "8h".to_string(),
        "12h".to_string(),
        "1d".to_string(),
        "3d".to_string(),
        "1w".to_string(),
        "1M".to_string(),
    ]
}

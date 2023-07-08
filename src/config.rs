use std::{
    error::Error,
    fs::File,
    io::{BufReader, Write},
    path::Path,
};

use getset::{Getters, Setters};
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;

const DEFAULT_DATA: &str = r#"
periods: 2000
interval: "15m"
depth: 3
tickers: 
    - "BTCBUSD"
    - "ETHBUSD"
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
    tickers: Vec<String>,
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
        let config: Config = from_reader(reader).map_err(|err| Box::new(err) as Box<dyn Error>)?;
        Ok(Box::new(config))
    }

    #[inline]
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

#[derive(thiserror::Error, Debug)]
pub enum ConfigurationError {
    #[error("Configuration file not found")]
    FileNotFound,
    #[error("Interval (`{0}`) not supported")]
    IntervalError(String),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn test_interval_minutes() {
        let config = Config::default();
        assert_eq!(config.interval_minutes().unwrap(), 15);
    }

    #[test]
    fn check_default() {
        let config = Config::default();
        assert_eq!(config.periods, 2000);
        assert_eq!(config.interval, "15m");
        assert_eq!(config.depth, 3);
        assert_eq!(
            config.tickers,
            vec!["BTCBUSD".to_string(), "ETHBUSD".to_string()]
        );
    }

    #[tokio::test]
    async fn check_default_match() {
        let default_config = Config::default();
        let path = Path::new("test_config.yml");
        let mut file = File::create(path).unwrap();
        file.write_all(DEFAULT_DATA.as_bytes()).unwrap();
        let config = Config::read_config(Some("test_config.yml")).await.unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(config.periods, default_config.periods);
        assert_eq!(config.interval, default_config.interval);
        assert_eq!(config.depth, default_config.depth);
        assert_eq!(config.tickers, default_config.tickers);
    }
}

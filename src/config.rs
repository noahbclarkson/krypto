use std::{
    error::Error,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use getset::{Getters, Setters};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Getters, Setters)]
#[getset(get = "pub", set = "pub")]
pub struct Config {
    periods: usize,
    interval: String,
    fee: f64,
    #[serde(rename = "api-key")]
    api_key: Option<String>,
    #[serde(rename = "api-secret")]
    api_secret: Option<String>,
    #[serde(rename = "testnet-api-key")]
    testnet_api_key: Option<String>,
    #[serde(rename = "testnet-api-secret")]
    testnet_api_secret: Option<String>,
}

impl Config {
    pub fn get_interval_minutes(&self) -> Result<i64, Box<dyn Error>> {
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
            _ => Err(Box::new(IntervalError)),
        }
    }

    pub async fn read_tickers() -> Result<Vec<String>, Box<dyn Error>> {
        let path = Path::new("tickers.txt");
        if !path.exists() {
            File::create(path).unwrap();
            return Err(Box::new(ConfigReadError));
        }
        let mut tickers = Vec::new();
        let file = File::open(path).unwrap();
        for line in BufReader::new(file).lines() {
            let ticker = line.unwrap_or_default().to_uppercase().trim().to_string();
            tickers.push(ticker);
        }
        tickers.retain(|ticker| !ticker.is_empty());
        Ok(tickers)
    }

    pub async fn read_config() -> Result<Self, Box<dyn Error>> {
        let path = Path::new("config.yml");

        if !path.exists() {
            File::create(path)?;
            return Err(Box::new(TickerReadError));
        }

        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let config: Config =
            serde_yaml::from_reader(reader).map_err(|err| Box::new(err) as Box<dyn Error>)?;
        Ok(config)
    }

    pub fn get_test_config() -> Self {
        Self {
            periods: 2000,
            interval: "15m".to_string(),
            fee: 0.0,
            api_key: None,
            api_secret: None,
            testnet_api_key: None,
            testnet_api_secret: None,
        }
    }
}

#[derive(Debug, Clone)]
struct TickerReadError;

impl std::fmt::Display for TickerReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Tickers file not found. Please enter your tickers in the tickers.txt file that was created.")
    }
}

impl Error for TickerReadError {}

#[derive(Debug, Clone)]
struct ConfigReadError;

impl std::fmt::Display for ConfigReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Config file not found. Please enter your config in the config.yml file that was created.")
    }
}

impl Error for ConfigReadError {}

#[derive(Debug, Clone)]
struct IntervalError;

impl std::fmt::Display for IntervalError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Invalid interval")
    }
}

impl Error for IntervalError {}

#[cfg(test)]
mod tests {
    use std::io::BufWriter;
    use std::io::Write;

    use super::*;

    #[test]
    fn test_get_interval_minutes() {
        let config = Config {
            periods: 2000,
            interval: "15m".to_string(),
            fee: 0.0,
            api_key: None,
            api_secret: None,
            testnet_api_key: None,
            testnet_api_secret: None,
        };
        assert_eq!(config.get_interval_minutes().unwrap(), 15);
    }

    #[tokio::test]
    async fn test_read_tickers() {
        // Create tickers file if there isn't one
        let path = Path::new("tickers.txt");
        let path_exists = path.exists();
        if !path_exists {
            // Insert tickers
            let mut file = File::create(path).unwrap();
            // Create new BufferWriter instance
            let mut writer = BufWriter::new(&mut file);
            // Write to file using `write` trait
            writer.write_all(b"BTCUSDT\nETHUSDT").unwrap();
        }
        let tickers = Config::read_tickers().await;
        assert!(tickers.is_ok());
        if !path_exists {
            let tickers = tickers.unwrap();
            assert_eq!(tickers[0], "BTCUSDT");
            assert_eq!(tickers[1], "ETHUSDT");
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn test_get_test_config() {
        let config = Config::get_test_config();
        assert_eq!(config.periods, 2000);
        assert_eq!(config.interval, "15m");
        assert_eq!(config.fee, 0.0);
        assert_eq!(config.api_key, None);
        assert_eq!(config.api_secret, None);
        assert_eq!(config.testnet_api_key, None);
        assert_eq!(config.testnet_api_secret, None);
    }

    #[tokio::test]
    async fn test_read_config() {
        let path = Path::new("config.yml");
        let path_exists = path.exists();
        if !path_exists {
            let mut file = File::create(path).unwrap();
            let config_data = r#"
                periods: 1000
                interval: "1h"
                fee: 0.1
                api-key: "your-api-key"
                api-secret: "your-api-secret"
                testnet-api-key: "your-testnet-api-key"
                testnet-api-secret: "your-testnet-api-secret"
            "#;
            file.write_all(config_data.as_bytes()).unwrap();
        }
        let config = Config::read_config().await;
        assert!(config.is_ok());
        if !path_exists {
            let config = config.unwrap();
            assert_eq!(config.periods, 1000);
            assert_eq!(config.interval, "1h");
            assert_eq!(config.fee, 0.1);
            assert_eq!(config.api_key, Some("your-api-key".to_string()));
            assert_eq!(config.api_secret, Some("your-api-secret".to_string()));
            assert_eq!(
                config.testnet_api_key,
                Some("your-testnet-api-key".to_string())
            );
            assert_eq!(
                config.testnet_api_secret,
                Some("your-testnet-api-secret".to_string())
            );
            std::fs::remove_file(path).unwrap();
        }
    }
}

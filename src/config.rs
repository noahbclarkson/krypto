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
}

const DEFAULT_DATA: &str = r#"
start-date: "2024-01-01"
symbols:
  - "BTCUSDT"
  - "ETHUSDT"
intervals:
    - "2h"
    - "4h"
cross-validations: 25
fee: 0.001
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

#[cfg(test)]
mod tests {
    use super::*;
    use binance::market::Market;
    use std::fs::{self};
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_config_file_does_not_exist() {
        // Create a temp file path but don't create the file
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();
        drop(temp_file); // Delete the temp file

        // Ensure the file does not exist
        assert!(!path.exists());

        // Read the config
        let config = KryptoConfig::read_config(Some(&path)).unwrap();

        // Verify default config is returned
        assert_eq!(config, KryptoConfig::default());

        // Verify the default config file is created
        assert!(path.exists());

        // Clean up
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_read_config_file_exists_valid_yaml() {
        // Create a temp file with valid YAML content
        let mut temp_file = NamedTempFile::new().unwrap();
        let yaml_content = r#"
start-date: "2023-01-01"
symbols:
  - "BTCUSDT"
  - "ETHUSDT"
intervals:
  - "1h"
  - "4h"
cross-validations: 25
"#;
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        // Read the config
        let config = KryptoConfig::read_config(Some(temp_file.path())).unwrap();

        // Verify the config is loaded correctly
        assert_eq!(config.start_date, "2023-01-01");
        assert_eq!(config.api_key, None);
        assert_eq!(config.api_secret, None);
        assert_eq!(
            config.symbols,
            vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()]
        );
        assert_eq!(
            config.intervals,
            vec![Interval::OneHour, Interval::FourHours]
        );
        assert_eq!(config.cross_validations, 25);
    }

    #[test]
    fn test_start_date_valid() {
        let config = KryptoConfig {
            start_date: "2023-01-01".to_string(),
            ..Default::default()
        };
        let date = config.start_date().unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2023, 1, 1).unwrap());
    }

    #[test]
    fn test_start_date_invalid() {
        let config = KryptoConfig {
            start_date: "invalid-date".to_string(),
            ..Default::default()
        };
        let result = config.start_date();
        assert!(matches!(result, Err(KryptoError::ParseDateError(_))));
    }

    #[test]
    fn test_interval_minutes() {
        let config = KryptoConfig {
            intervals: vec![Interval::OneHour, Interval::FourHours],
            ..Default::default()
        };
        let minutes = config.interval_minutes();
        assert_eq!(minutes, vec![60, 240]);
    }

    #[test]
    fn test_get_binance_with_credentials() {
        struct MockBinance {
            api_key: Option<String>,
            api_secret: Option<String>,
        }

        impl Binance for MockBinance {
            fn new(api_key: Option<String>, api_secret: Option<String>) -> Self {
                MockBinance {
                    api_key,
                    api_secret,
                }
            }

            fn new_with_config(
                api_key: Option<String>,
                api_secret: Option<String>,
                _config: &binance::config::Config,
            ) -> Self {
                Self::new(api_key, api_secret)
            }

            // Implement other required methods or use default implementations
        }

        let config = KryptoConfig {
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            ..Default::default()
        };
        let binance = config.get_binance::<MockBinance>();
        assert_eq!(binance.api_key, Some("test_key".to_string()));
        assert_eq!(binance.api_secret, Some("test_secret".to_string()));
    }

    #[test]
    fn test_get_binance_without_credentials() {
        struct MockBinance {
            api_key: Option<String>,
            api_secret: Option<String>,
        }

        impl Binance for MockBinance {
            fn new(api_key: Option<String>, api_secret: Option<String>) -> Self {
                MockBinance {
                    api_key,
                    api_secret,
                }
            }

            fn new_with_config(
                api_key: Option<String>,
                api_secret: Option<String>,
                _config: &binance::config::Config,
            ) -> Self {
                Self::new(api_key, api_secret)
            }

            // Implement other required methods or use default implementations
        }

        let config = KryptoConfig {
            api_key: None,
            api_secret: None,
            ..Default::default()
        };
        let binance = config.get_binance::<MockBinance>();
        assert_eq!(binance.api_key, None);
        assert_eq!(binance.api_secret, None);
        let _: Market = config.get_binance();
    }

    #[test]
    fn test_read_config_with_empty_symbols() {
        // Create a temp file with empty symbols list
        let mut temp_file = NamedTempFile::new().unwrap();
        let yaml_content = r#"
start-date: "2023-01-01"
symbols: []
intervals:
  - "1h"
cross-validations: 25
"#;
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        // Read the config
        let config = KryptoConfig::read_config(Some(temp_file.path())).unwrap();

        // Verify that the symbols list is empty
        assert!(config.symbols.is_empty());
    }

    #[test]
    fn test_read_config_with_missing_fields() {
        // Create a temp file with missing fields
        let mut temp_file = NamedTempFile::new().unwrap();
        let yaml_content = r#"
symbols:
  - "BTCUSDT"
intervals:
  - "1h"
"#; // Missing start-date
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        // Read the config
        let result = KryptoConfig::read_config(Some(temp_file.path()));

        assert!(result.is_err());
    }

    #[test]
    fn test_read_config_with_extra_fields() {
        // Create a temp file with extra fields
        let mut temp_file = NamedTempFile::new().unwrap();
        let yaml_content = r#"
start-date: "2023-01-01"
symbols:
  - "BTCUSDT"
intervals:
  - "1h"
cross-validations: 25
extra-field: "extra"
"#;
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        // Read the config
        let config = KryptoConfig::read_config(Some(temp_file.path())).unwrap();

        // Verify that the extra field is ignored and config is loaded
        assert_eq!(config.start_date, "2023-01-01");
    }

    #[test]
    fn test_read_config_with_invalid_start_date_format() {
        // Create a temp file with invalid start date format
        let mut temp_file = NamedTempFile::new().unwrap();
        let yaml_content = r#"
start-date: "01-01-2023" # Invalid format
symbols:
  - "BTCUSDT"
intervals:
  - "1h"
cross-validations: 25
"#;
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        // Read the config
        let config = KryptoConfig::read_config(Some(temp_file.path())).unwrap();

        // Attempt to parse the start date and expect an error
        let result = config.start_date();
        assert!(matches!(result, Err(KryptoError::ParseDateError(_))));
    }

    #[test]
    fn compare_default_config() {
        let default_config = KryptoConfig::default();
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(DEFAULT_DATA.as_bytes()).unwrap();
        let config = KryptoConfig::read_config(Some(temp_file.path())).unwrap();
        assert_eq!(default_config, config);
    }

    #[test]
    fn test_config_with_no_intervals() {
        // Create a temp file with no intervals
        let mut temp_file = NamedTempFile::new().unwrap();
        let yaml_content = r#"
start-date: "2023-01-01"
symbols:
  - "BTCUSDT"
cross-validations: 25
"#; // No intervals field
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        // Read the config
        let config_result = KryptoConfig::read_config(Some(temp_file.path()));

        // Verify that deserialization fails because intervals is a required field
        assert!(matches!(config_result, Err(KryptoError::SerdeYamlError(_))));
    }

    #[test]
    fn test_config_with_invalid_api_keys() {
        // Create a temp file with invalid API keys (non-string types)
        let mut temp_file = NamedTempFile::new().unwrap();
        let yaml_content = r#"
start-date: "2023-01-01"
api-key: 12345
api-secret: true
symbols:
  - "BTCUSDT"
intervals:
  - "1h"
cross-validations: 25
"#;
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        // Read the config and expect a deserialization error
        let config_result = KryptoConfig::read_config(Some(temp_file.path()));

        // Verify that deserialization fails with a ConfigReadError
        assert!(matches!(
            config_result,
            Err(KryptoError::BinanceApiError(_))
        ));
    }
}

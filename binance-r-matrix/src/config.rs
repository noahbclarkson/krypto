use binance::api::Binance;
use derive_builder::Builder;
use getset::{Getters, Setters};

use crate::interval::Interval;

#[derive(Debug, Getters, Setters, Builder, Clone)]
#[getset(get = "pub", set = "pub")]
pub struct HistoricalDataConfig {
    periods: usize,
    interval: Interval,
    tickers: Vec<String>,
    #[builder(default)]
    api_key: Option<String>,
    #[builder(default)]
    api_secret: Option<String>,
}

impl HistoricalDataConfig {
    pub fn get_binance<T: Binance>(&self) -> T {
        T::new(self.api_key.clone(), self.api_secret.clone())
    }

    pub fn interval_string(&self) -> &str {
        self.interval.to_string()
    }

    pub fn interval_minutes(&self) -> usize {
        self.interval.to_minutes()
    }
}

impl Default for HistoricalDataConfig {
    fn default() -> Self {
        Self {
            periods: 100,
            interval: Interval::OneMinute,
            tickers: Vec::new(),
            api_key: None,
            api_secret: None,
        }
    }
}

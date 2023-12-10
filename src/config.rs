use std::{
    fmt::{Display, Formatter},
    fs,
    io::Write as _,
    path::Path,
};

use binance_r_matrix::{
    config::{HistoricalDataConfig, HistoricalDataConfigBuilder},
    interval::Interval,
};
use getset::Getters;
use r_matrix::{
    r_matrix::{cmaes::CMAESOptimize, matrix::RMatrixBuilder},
    NormalizationFunctionType, RMatrix,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Getters, Clone)]
#[serde(rename_all = "camelCase")]
#[getset(get = "pub")]
pub struct Config {
    depth: usize,
    tickers: Vec<String>,
    forward_depth: usize,
    interval: Interval,
    periods: usize,
    train_test_split: f64,
    function_type: NormalizationFunctionType,
    #[serde(rename = "cmaesGoal")]
    cmaes_optimization_goal: CMAESOptimize,
    with_individuals: bool,
    fee: f64,
    margin: f64,
    api_key: Option<String>,
    api_secret: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_content = include_str!("config.yml");
        let config_path = Path::new("config.yml");

        // Check if the file exists
        if !config_path.exists() {
            // Create and write the file if it doesn't exist
            let mut file = fs::File::create(config_path)?;
            file.write_all(config_content.as_bytes())?;
        }

        let config_str = fs::read_to_string(config_path)?;
        let config: Config = serde_yaml::from_str(&config_str)?;

        Ok(config)
    }
}

impl From<Config> for HistoricalDataConfig {
    fn from(val: Config) -> Self {
        HistoricalDataConfigBuilder::default()
            .api_key(val.api_key)
            .api_secret(val.api_secret)
            .tickers(val.tickers)
            .periods(val.periods)
            .interval(val.interval)
            .build()
            .unwrap()
    }
}

impl From<Config> for RMatrix {
    fn from(val: Config) -> Self {
        RMatrixBuilder::default()
            .depth(val.depth)
            .max_forward_depth(val.forward_depth)
            .function(val.function_type)
            .reduction(val.fee)
            .margin(val.margin)
            .build()
            .unwrap()
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Depth: {}", self.depth)?;
        writeln!(f, "Tickers: {:?}", self.tickers)?;
        writeln!(f, "Forward Depth: {}", self.forward_depth)?;
        writeln!(f, "Interval: {}", self.interval)?;
        writeln!(f, "Periods: {}", self.periods)?;
        writeln!(f, "Train Test Split: {}", self.train_test_split)?;
        writeln!(
            f,
            "Normalization Function Type: {}",
            self.function_type.get_name()
        )?;
        writeln!(
            f,
            "CMAES Optimization Goal: {}",
            self.cmaes_optimization_goal
        )?;
        writeln!(f, "With Individuals: {}", self.with_individuals)?;
        writeln!(f, "Fee: {}", self.fee)?;
        writeln!(f, "Margin: {}", self.margin)?;
        Ok(())
    }
}

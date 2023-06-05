use getset::{Getters, Setters};
use serde::Deserialize;
use serde_yaml;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

// This struct is used to store configuration data from the config.yml file.
#[derive(Debug, Deserialize, Clone, Getters, Setters)]
pub struct Config {
    // Getters and setters are auto-generated using the getset crate.
    #[getset(get = "pub", set = "pub")]
    periods: usize,

    #[getset(get = "pub", set = "pub")]
    interval: String,

    #[getset(get = "pub", set = "pub")]
    margin: f64,

    #[getset(get = "pub", set = "pub")]
    fee: f64,
}

impl Config {
    // This function converts the interval string into its equivalent in minutes.
    // Instead of panicking on an invalid interval, it returns a Result and
    // propagates the error up to the caller.
    pub fn get_interval_minutes(&self) -> Result<usize, &'static str> {
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
            _ => Err("Invalid interval"),
        }
    }

    // This function returns a Vec of default ticker symbols.
    pub fn get_default_tickers() -> Vec<&'static str> {
        vec![
            "BTCUSDT",
            "ETHUSDT",
            "BNBUSDT",
            "ADABUSD",
            "XRPBUSD",
            "DOGEBUSD",
            "SOLBUSD",
            "FTMBUSD",
            "DODOBUSD",
            "GALABUSD",
            "TRXBUSD",
            "1000LUNCBUSD",
            "LTCBUSD",
            "MATICBUSD",
            "1000SHIBBUSD",
            "LDOBUSD",
            "APTBUSD",
            "AGIXBUSD",
        ]
    }

    pub fn get_default_config() -> Config {
        Config {
            periods: 7000,
            interval: "15m".to_string(),
            margin: 2.0,
            fee: 0.00054,
        }
    }
}

// This function reads ticker symbols from a file.
// If the file doesn't exist, it creates one with default values.
// If there's an error opening the file, it returns a Vec with default values.
pub async fn read_tickers() -> io::Result<Vec<String>> {
    let path = Path::new("tickers.txt");
    let default_tickers = Config::get_default_tickers();

    if !path.exists() {
        let mut file = File::create(&path)?;
        for ticker in &default_tickers {
            writeln!(file, "{}", ticker)?;
        }
    }

    let mut tickers = Vec::new();
    let file = File::open(&path)?;
    for line in io::BufReader::new(file).lines() {
        let ticker = line?;
        tickers.push(ticker);
    }

    Ok(tickers)
}

// This function reads configuration data from a YAML file.
// If the file doesn't exist, it creates one with default values.
// If there's an error opening the file, it returns a Config struct with default values.
pub async fn read_config() -> Result<Config, Box<dyn std::error::Error>> {
    let path = Path::new("config.yml");
    let default_config = Config::get_default_config();

    if !path.exists() {
        let mut file = File::create(&path)?;
        writeln!(file, "periods: {}", default_config.periods)?;
        writeln!(file, "interval: {}", default_config.interval)?;
        writeln!(file, "margin: {}", default_config.margin)?;
        writeln!(file, "fee: {}", default_config.fee)?;
    }

    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let config: Config = serde_yaml::from_reader(reader)
        .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)?;

    Ok(config)
}

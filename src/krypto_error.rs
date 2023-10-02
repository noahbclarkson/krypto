use std::error::Error;

#[derive(thiserror::Error, Debug)]
pub enum DataError {
    #[error("Not enough data for ticker {0}")]
    NotEnoughData(String),
    #[error("Data time mismatch between ticker {0} and {1} ({2}:{3})")]
    DataTimeMismatch(String, String, i64, i64),
    #[error("Binance error for symbol {symbol}: {error}")]
    BinanceError {
        symbol: String,
        error: binance::errors::Error,
    },
    #[error("Technical calculation error: {error}")]
    TechnicalCalculationError { error: Box<dyn Error> },
    #[error("File error for file {file_name}: {error}")]
    FileError {
        error: std::io::Error,
        file_name: String,
    },
    #[error("Csv error for file {file_name}: {error}")]
    CsvError {
        error: csv::Error,
        file_name: String,
    },
    #[error("Invalid split ratio: {ratios:?}")]
    InvalidSplitRatio { ratios: (f32, f32, f32) },
    #[error("Ticker {ticker} has NaN values")]
    TickerHasNaN { ticker: String },
}

#[derive(thiserror::Error, Debug)]
pub enum ConfigurationError {
    #[error("Configuration file not found")]
    FileNotFound,
    #[error("Interval (`{0}`) not supported")]
    IntervalError(String),
}

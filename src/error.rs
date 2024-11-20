use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum KryptoError {
    #[error("Invalid candlestick date time on {when} occurred with time {timestamp}.")]
    InvalidCandlestickDateTime { when: When, timestamp: i64 },
    #[error("Failed to parse value {value_name} at time {timestamp}.")]
    ParseError { value_name: String, timestamp: i64 },
    #[error("Open time is greater than close time for candle. Open time: {open_time}, Close time: {close_time}.")]
    OpenTimeGreaterThanCloseTime { open_time: i64, close_time: i64 },
    #[error("Parse Interval Error: {0}")]
    ParseIntervalError(#[from] ParseIntervalError),
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to parse date: {0}")]
    ParseDateError(#[from] chrono::ParseError),
    #[error("Failed to convert date: {0}")]
    DateConversionError(String),
    #[error("Serde YAML Error: {0}")]
    SerdeYamlError(#[from] serde_yaml::Error),
    #[error("Binance API Error: {0}")]
    BinanceApiError(String),
    #[error("Failed to fit PLS model: {0}")]
    FitError(#[from] linfa_pls::PlsError),
    #[error("CSV Error: {0}")]
    CsvError(#[from] csv::Error),
    #[error("Candles and predictions cannot be empty")]
    EmptyCandlesAndPredictions,
    #[error("Candles and predictions must be of the same length")]
    UnequalCandlesAndPredictions,
    #[error("Shape Error: {0}")]
    ShapeError(#[from] ndarray::ShapeError),
}

#[derive(Debug, Clone, PartialEq)]
pub enum When {
    Open,
    Close,
}

impl fmt::Display for When {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            When::Open => write!(f, "open"),
            When::Close => write!(f, "close"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ParseIntervalError {
    #[error("Failed to parse interval from string: {0}")]
    ParseError(String),
    #[error("Failed to parse interval from integer: {0}")]
    ParseIntError(usize),
}

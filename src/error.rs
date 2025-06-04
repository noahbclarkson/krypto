use std::fmt;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum KryptoError {
    #[error("Invalid candlestick date time on {when} occurred with time {timestamp}. Symbol: {symbol}, Interval: {interval}")]
    InvalidCandlestickDateTime {
        when: When,
        timestamp: i64,
        symbol: String,
        interval: String,
    },
    #[error("Failed to parse value {value_name} at time {timestamp}. Symbol: {symbol}, Interval: {interval}")]
    CandlestickParseError {
        value_name: String,
        timestamp: i64,
        symbol: String,
        interval: String,
    },
    #[error("Open time {open_time} is greater than close time {close_time} for candle. Symbol: {symbol}, Interval: {interval}")]
    OpenTimeGreaterThanCloseTime {
        open_time: i64,
        close_time: i64,
        symbol: String,
        interval: String,
    },
    #[error("Parse Interval Error: {0}")]
    ParseIntervalError(#[from] ParseIntervalError),
    #[error("Failed to parse value error: {0}")]
    ParseFloatError(#[from] std::num::ParseFloatError),
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Chrono Parse Error: {0}")]
    ChronoParseError(#[from] chrono::ParseError),
    #[error("Serde YAML Error: {0}")]
    SerdeYamlError(#[from] serde_yaml::Error),
    #[error("Serde Bincode Error: {0}")]
    SerdeBincodeError(#[from] Box<bincode::error::EncodeError>),
    #[error("Binance API Error: {0}")]
    BinanceApiError(String),
    #[error("Failed to convert date: {0}")]
    DateConversionError(String),
    #[error("Failed to fit PLS model: {0}")]
    PlsFitError(#[from] linfa_pls::PlsError),
    #[error("CSV Error: {0}")]
    CsvError(#[from] csv::Error),
    #[error("Backtest Error: Candles and predictions cannot be empty")]
    EmptyCandlesOrPredictions,
    #[error("Backtest Error: Candles ({0}) and predictions ({1}) must be of the same length")]
    UnequalCandlesAndPredictions(usize, usize),
    #[error("Shape Error: {0}")]
    ShapeError(#[from] ndarray::ShapeError),
    #[error("Invalid Dataset: Mismatched lengths (Features: {0}, Labels: {1}, Candles: {2})")]
    InvalidDatasetLengths(usize, usize, usize),
    #[error("Insufficient data for operation: Got {got}, Required {required}. Context: {context}")]
    InsufficientData {
        got: usize,
        required: usize,
        context: String,
    },
    #[error("PLS Fit Error (Internal): {0}")]
    PlsInternalError(String),
    #[error("Symbol '{0}' not found in exchange information")]
    SymbolNotFound(String),
    #[error("Interval '{0}' not found in loaded dataset")]
    IntervalNotFound(String),
    #[error("Configuration Error: {0}")]
    ConfigError(String),
    #[error("Cache directory not found or could not be created: {0}")]
    CacheDirError(PathBuf),
    #[error("Failed to calculate fitness for strategy: {0}")]
    FitnessCalculationError(String),
    #[error("Technical indicator calculation failed for '{indicator}': {reason}")]
    TechnicalIndicatorError { indicator: String, reason: String },
    #[error("Walk forward validation error: {0}")]
    WalkForwardError(String),
    #[error("Precision formatting error for {value_type}: {value}")]
    PrecisionFormatError { value_type: String, value: f64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseIntervalError {
    #[error("Failed to parse interval from string: {0}")]
    ParseStringError(String),
    #[error("Failed to parse interval from integer: {0}")]
    ParseIntError(usize),
}

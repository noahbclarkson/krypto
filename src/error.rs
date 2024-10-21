use smartcore::error::Failed;

#[derive(thiserror::Error, Debug)]
pub enum KryptoError {
    #[error("Configuration file not found")]
    ConfigFileNotFound,
    #[error("Interval (`{0}`) not supported")]
    IntervalError(String),
    #[error("Date parsing error: {0}")]
    DateParseError(String),
    #[error("Binance error: {0}")]
    BinanceError(#[from] binance::errors::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("No data found for ticker `{0}`")]
    NoDataFound(String),
    #[error("Error loading config file")]
    ConfigLoadError,
    #[error("Invalid timestamp")]
    InvalidTimestamp,
    #[error("Invalid date time")]
    InvalidDateTime,
    #[error("Join error")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("CSV error: {0}")]
    CsvError(#[from] csv::Error),
    #[error("Random Forest error: {0}")]
    RandomForestError(#[from] Failed),
    #[error("TA error: {0}")]
    TaError(#[from] ta::errors::TaError),
    #[error(
        "Data mismatch: features length {features_len} does not match labels length {labels_len}"
    )]
    DataMismatch {
        features_len: usize,
        labels_len: usize,
    },
    #[error("Features vector is empty")]
    EmptyData,
    #[error("Test data features are empty")]
    EmptyTestData,
    #[error("Training error: {0}")]
    TrainingError(String),
    #[error("Prediction error: {0}")]
    PredictionError(String),
}

#[derive(Debug, thiserror::Error)]
pub enum BinanceDataError {
    #[error("Binance error: {error} for symbol {symbol}")]
    BinanceError {
        symbol: String,
        error: binance::errors::Error,
    },
    #[error("Technical error: {0}")]
    TechnicalError(#[from] ta::errors::TaError),
    #[error("{symbol} doesn't match desired data length (desired: {desired}, actual: {actual})")]
    DatasizeMismatch {
        symbol: String,
        desired: usize,
        actual: usize,
    },
    #[error("{symbol} has mismatched close times: {time_1} != {time_2}")]
    MismatchedCloseTimes {
        symbol: String,
        time_1: i64,
        time_2: i64,
    },
}



#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("Binance error: {error} for symbol {symbol}")]
    BinanceError{
        symbol: String,
        error: binance::errors::Error,
    },
    #[error("Technical error: {0}")]
    TechnicalError(#[from] ta::errors::TaError),
    #[error("{symbol} doesn't have enough data (desired: {desired}, actual: {actual})")]
    NotEnoughData{
        symbol: String,
        desired: usize,
        actual: usize,
    },
    #[error("{symbol} has too much data (desired: {desired}, actual: {actual})")]
    TooMuchData{
        symbol: String,
        desired: usize,
        actual: usize,
    },
    #[error("{symbol} has mismatched close times: {time_1} != {time_2}")]
    MismatchedCloseTimes{
        symbol: String,
        time_1: i64,
        time_2: i64,
    }
}
use getset::Getters;

use crate::{
    config::Config, historical_data_request::HistoricalDataRequest, krypto_error::DataError,
    technical_calculator::TechnicalCalculator, ticker_data::{TickerData, check_valid},
};

pub const MINS_TO_MILLIS: i64 = 60_000;

#[derive(Getters)]
#[getset(get = "pub")]
pub struct HistoricalData {
    tickers: Box<[TickerData]>,
}

impl HistoricalData {
    pub async fn load(config: &Config, interval_index: usize) -> Result<Self, DataError> {
        let request = HistoricalDataRequest::new(config.clone(), interval_index);
        let tasks = config
            .tickers()
            .iter()
            .map(|ticker| request.run(ticker, interval_index));
        let tickers = futures::future::join_all(tasks).await;
        let tickers = tickers.into_iter().collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            tickers: tickers.into_boxed_slice(),
        })
    }

    pub fn calculate_technicals(&mut self) -> Result<(), DataError> {
        let mut technical_calculator = TechnicalCalculator::new();
        let tickers = technical_calculator
            .calculate_technicals(self.tickers.clone())
            .map_err(|e| DataError::TechnicalCalculationError {
                error: Box::from(e),
            })?;
        self.tickers = tickers;
        for ticker in self.tickers.iter() {
            ticker.find_nan()?;
            for other in self.tickers.iter() {
                if ticker.ticker() != other.ticker() {
                    ticker.ensure_validity(other)?;
                }
            }
        }
        Ok(())
    }

    pub async fn print_to_files(&self, folder: &str) -> Result<(), std::io::Error> {
        let tasks = self
            .tickers
            .iter()
            .map(|ticker| ticker.print_to_file(folder));
        let results = futures::future::join_all(tasks).await;
        for result in results {
            result?;
        }
        Ok(())
    }

    pub fn get_scores(&self) -> Vec<(String, f64)> {
        self.tickers
            .iter()
            .map(|ticker| (ticker.ticker().to_string(), ticker.average_variance_score()))
            .collect()
    }

    pub fn find_matching_close_time_index(&self, close_time: i64) -> usize {
        let ticker_data = &self.tickers[0];
        let candles = ticker_data.candles();
        let mut index = 0;
        for (i, candle) in candles.iter().enumerate() {
            if check_valid(candle.close_time(), &close_time) {
                index = i;
                break;
            }
        }
        index
    }
}

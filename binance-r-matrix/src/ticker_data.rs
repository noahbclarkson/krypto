use binance::rest_model::KlineSummary;
use getset::Getters;

use crate::{error::BinanceDataError, technical_calculator::TechnicalCalulator};

#[derive(Debug, Getters)]
#[getset(get = "pub")]
pub struct TickerData {
    klines: Box<[KlineSummary]>,
    technicals: Box<[Vec<f64>]>,
    ticker: Box<str>,
}

impl TickerData {
    pub(crate) fn new(ticker: String, klines: Vec<KlineSummary>) -> Self {
        Self {
            ticker: ticker.into_boxed_str(),
            klines: klines.into_boxed_slice(),
            technicals: Box::new([]),
        }
    }

    pub(crate) fn close_times(&self) -> impl Iterator<Item = i64> + '_ {
        self.klines.iter().map(|kline| kline.close_time)
    }

    pub fn len(&self) -> usize {
        self.klines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.klines.is_empty()
    }

    pub fn validate(&self, periods: usize) -> Result<(), BinanceDataError> {
        let actual = self.klines.len();
        let desired = periods;
        match actual.cmp(&desired) {
            std::cmp::Ordering::Equal => Ok(()),
            _ => Err(BinanceDataError::DatasizeMismatch {
                symbol: self.ticker.to_string(),
                actual,
                desired,
            }),
        }
    }

    pub fn load_technicals(&mut self) -> Result<(), BinanceDataError> {
        let mut calculator = TechnicalCalulator::new();
        let technicals = calculator.calculate_technicals(&self.klines)?;
        self.technicals = technicals.into_boxed_slice();
        Ok(())
    }

    // pub fn get_features(&self, index: usize) -> &[f64] {
    //     &self.technicals[index]
    // }

}
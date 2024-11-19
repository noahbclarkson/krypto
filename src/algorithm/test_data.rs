use std::fmt;

use crate::{
    config::KryptoConfig, data::candlestick::Candlestick, error::KryptoError,
    util::date_utils::days_between,
};

const STARTING_CASH: f64 = 1000.0;

pub struct TestData {
    pub cash_history: Vec<f64>,
    pub accuracy: f64,
    pub monthly_return: f64,
}

impl TestData {
    pub fn new(
        predictions: Vec<f64>,
        candles: Vec<Candlestick>,
        config: &KryptoConfig,
    ) -> Result<Self, KryptoError> {
        if candles.is_empty() || predictions.is_empty() {
            return Err(KryptoError::EmptyCandlesAndPredictions);
        }

        if candles.len() != predictions.len() {
            return Err(KryptoError::UnequalCandlesAndPredictions);
        }

        let fee = config.fee.unwrap_or(0.0);
        let days = days_between(
            candles.first().unwrap().open_time,
            candles.last().unwrap().close_time,
        );
        let mut position: Option<Position> = None;
        let mut inner = InnerTestData::default();

        for (prediction, candle) in predictions.iter().zip(candles.iter()) {
            let prediction_sign = prediction.signum();

            let new_position = match prediction_sign {
                p if p > 0.0 => Some(Position::Long(candle.close)),
                p if p < 0.0 => Some(Position::Short(candle.close)),
                _ => None,
            };

            // Check if we need to close the existing position
            if position.is_some() && position != new_position {
                // Close the existing position
                if let Some(ref pos) = position {
                    inner.close_position(pos, candle, fee);
                }

                position = new_position;
            } else if position.is_none() {
                // Open a new position if we don't have one
                position = new_position.clone();
            }

            // No position change; continue holding or staying out
        }

        // Close any remaining open position at the end
        if let Some(ref pos) = position {
            inner.close_position(pos, candles.last().unwrap(), fee);
        }

        let months = days as f64 / 30.44;
        let total_trades = inner.correct + inner.incorrect;
        let accuracy = if total_trades > 0 {
            inner.correct as f64 / total_trades as f64
        } else {
            0.0
        };
        let monthly_return = if months > 0.0 && inner.cash.is_finite() && inner.cash > 0.0 && inner.cash_history.len() > 1 {
            (inner.cash / 1000.0).powf(1.0 / months) - 1.0
        } else {
            0.0
        };

        Ok(Self {
            cash_history: inner.cash_history,
            accuracy,
            monthly_return,
        })
    }
}

impl fmt::Display for TestData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Accuracy: {:.2}% | Monthly Return: {:.2}%",
            self.accuracy * 100.0,
            self.monthly_return * 100.0
        )
    }
}

#[derive(Debug, Clone)]
enum Position {
    Long(f64),
    Short(f64),
}

impl Position {
    fn get_return(&self, close_price: f64) -> f64 {
        match *self {
            Position::Long(entry_price) => (close_price - entry_price) / entry_price,
            Position::Short(entry_price) => (entry_price - close_price) / entry_price,
        }
    }
}

impl PartialEq for Position {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Position::Long(_), Position::Long(_)) | (Position::Short(_), Position::Short(_))
        )
    }
}

struct InnerTestData {
    cash: f64,
    correct: u32,
    incorrect: u32,
    cash_history: Vec<f64>,
}

impl InnerTestData {
    fn close_position(&mut self, position: &Position, candle: &Candlestick, fee: f64) {
        let return_now = position.get_return(candle.close);
        self.cash += self.cash * return_now;
        self.cash -= self.cash * fee;
        self.cash_history.push(self.cash);

        if return_now > 0.0 {
            self.correct += 1;
        } else {
            self.incorrect += 1;
        }
    }
}

impl Default for InnerTestData {
    fn default() -> Self {
        Self {
            cash: STARTING_CASH,
            correct: 0,
            incorrect: 0,
            cash_history: vec![STARTING_CASH],
        }
    }
}

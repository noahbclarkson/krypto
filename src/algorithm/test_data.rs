use std::fmt;

use crate::{
    config::KryptoConfig, data::candlestick::Candlestick, util::date_utils::days_between_datetime,
};

pub struct TestData {
    pub cash_history: Vec<f64>,
    pub accuracy: f64,
    pub monthly_return: f64,
}

impl TestData {
    pub fn new(predictions: Vec<f64>, candles: Vec<Candlestick>, config: &KryptoConfig) -> Self {
        let days =
            days_between_datetime(candles[0].open_time, candles[candles.len() - 1].close_time);
        let mut position = Position::None;
        let mut cash = 1000.0;
        let mut correct = 0;
        let mut incorrect = 0;
        let mut cash_history = vec![cash];
        for i in 0..predictions.len() {
            let prediction = predictions[i].signum();
            let position_now = Position::from_f64(prediction, candles[i].close);
            if position == Position::None {
                position = position_now.clone();
            }
            if position != position_now {
                let return_now = position.get_return(candles[i].close);
                cash += cash * return_now;
                cash -= cash * config.fee.unwrap_or_default();
                position = position_now;
                if return_now > 0.0 {
                    correct += 1;
                } else {
                    incorrect += 1;
                }
                cash_history.push(cash);
            }
        }
        let months = days as f64 / 30.0;
        let accuracy = correct as f64 / (correct + incorrect) as f64;
        let monthly_return = (cash / 1000.0).powf(1.0 / months) - 1.0;
        Self {
            cash_history,
            accuracy,
            monthly_return,
        }
    }
}

impl fmt::Display for TestData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Accuracy: {:.2} | Monthly Return: {:.2}%",
            self.accuracy * 100.0,
            self.monthly_return * 100.0
        )
    }
}

#[derive(Debug, Clone)]
enum Position {
    Long(f64),
    Short(f64),
    None,
}

impl Position {
    fn get_return(&self, close: f64) -> f64 {
        match self {
            Position::Long(entry) => (close - entry) / entry,
            Position::Short(entry) => (entry - close) / entry,
            Position::None => 0.0,
        }
    }

    fn from_f64(value: f64, open_price: f64) -> Self {
        if value > 0.0 {
            Position::Long(open_price)
        } else if value < 0.0 {
            Position::Short(open_price)
        } else {
            Position::None
        }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Position::Long(entry) => write!(f, "Long: ${}", entry),
            Position::Short(entry) => write!(f, "Short: ${}", entry),
            Position::None => write!(f, "None"),
        }
    }
}

impl PartialEq<Position> for f64 {
    fn eq(&self, other: &Position) -> bool {
        match other {
            Position::Long(_) => self > &0.0,
            Position::Short(_) => self < &0.0,
            Position::None => self == &0.0,
        }
    }
}

impl PartialEq<f64> for Position {
    fn eq(&self, other: &f64) -> bool {
        match self {
            Position::Long(_) => other > &0.0,
            Position::Short(_) => other < &0.0,
            Position::None => other == &0.0,
        }
    }
}

impl PartialEq for Position {
    fn eq(&self, other: &Position) -> bool {
        // Simply compare the type of the enum
        match self {
            Position::Long(_) => matches!(other, Position::Long(_)),
            Position::Short(_) => matches!(other, Position::Short(_)),
            Position::None => matches!(other, Position::None),
        }
    }
}

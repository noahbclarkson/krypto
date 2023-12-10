use std::fmt::Display;

use getset::Getters;

const STARTING_CASH: f64 = 1000.0;

#[derive(Debug, Clone, Getters)]
#[getset(get = "pub")]
pub struct TestData {
    pub cash: f64,
    pub correct: usize,
    pub incorrect: usize,
    pub mses: Vec<f64>,
    pub cash_history: Vec<f64>,
    pub hold_periods: usize,
}

impl TestData {
    pub fn add_cash(&mut self, cash: f64) {
        self.cash += cash;
        self.cash_history.push(self.cash);
        if cash > 0.0 {
            self.correct += 1;
        } else {
            self.incorrect += 1;
        }
    }

    pub fn add_error(&mut self, prediction: f64, real: f64) {
        self.mses.push((prediction - real).powi(2));
    }

    pub fn get_mse(&self) -> f64 {
        self.mses.iter().sum::<f64>() / self.mses.len() as f64
    }

    pub fn get_accuracy(&self) -> f64 {
        self.correct as f64 / (self.correct + self.incorrect) as f64
    }

    pub fn add_hold_period(&mut self) {
        self.hold_periods += 1;
    }

    pub fn add_hold_periods(&mut self, periods: usize) {
        self.hold_periods += periods;
    }
}

impl Default for TestData {
    fn default() -> Self {
        Self {
            cash: STARTING_CASH,
            correct: 0,
            incorrect: 0,
            mses: Vec::new(),
            cash_history: Vec::new(),
            hold_periods: 0,
        }
    }
}

impl Display for TestData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Cash: ${}", self.cash)?;
        writeln!(f, "Correct: {}", self.correct)?;
        writeln!(f, "Incorrect: {}", self.incorrect)?;
        writeln!(f, "MSE: {}", self.get_mse())?;
        writeln!(f, "Accuracy: %{}", self.get_accuracy() * 100.0)?;
        Ok(())
    }
}

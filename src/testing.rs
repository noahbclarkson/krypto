use std::fmt::{Display, Formatter};

use getset::Getters;

use crate::math::format_number;

#[derive(Debug, Clone, Getters)]
pub struct TestData {
    #[getset(get = "pub")]
    cash: f64,
    correct: usize,
    incorrect: usize,
}

impl TestData {
    pub fn new(cash: f64) -> Self {
        Self {
            cash,
            correct: 0,
            incorrect: 0,
        }
    }

    pub fn get_accuracy(&self) -> f64 {
        self.correct as f64 / (self.correct + self.incorrect) as f64
    }

    pub fn add_correct(&mut self) {
        self.correct += 1;
    }

    pub fn add_incorrect(&mut self) {
        self.incorrect += 1;
    }

    pub fn add_cash(&mut self, cash: f64) {
        self.cash += cash;
    }
}

impl Display for TestData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Cash: ${},  Accuracy: {:.2}%",
            format_number(self.cash),
            self.get_accuracy() * 100.0
        )
    }
}

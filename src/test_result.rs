use std::fmt::{Display, Formatter};

use getset::{Getters, Setters};

use crate::math::format_number;

#[derive(Debug, Clone, Getters, Setters)]
#[getset(get = "pub")]
pub struct TestResult {
    #[getset(set = "pub")]
    cash: f64,
    correct: usize,
    incorrect: usize,
    cash_history: Vec<f64>,
}

impl TestResult {
    pub fn new(cash: f64) -> Self {
        Self {
            cash,
            correct: 0,
            incorrect: 0,
            cash_history: vec![cash],
        }
    }

    #[inline(always)]
    pub fn get_accuracy(&self) -> f64 {
        self.correct as f64 / (self.correct + self.incorrect) as f64
    }

    #[inline(always)]
    pub fn add_correct(&mut self) {
        self.correct += 1;
    }

    #[inline(always)]
    pub fn add_incorrect(&mut self) {
        self.incorrect += 1;
    }

    #[inline(always)]
    pub fn add_cash(&mut self, cash: f64) {
        self.cash += cash;
        self.cash_history.push(self.cash);
    }

    pub fn get_return(&self, period: PerPeriod, periods: usize, interval: usize) -> f64 {
        let start = self.cash_history[0];
        let end = self.cash;
        let total_minutes = periods * interval;
        let minutes_per_period = period.get_periods();
        let periods = total_minutes as f64 / minutes_per_period as f64;
        f64::powf(end/start, 1.0/periods) - 1.0
    }
}

impl Display for TestResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Cash: ${},  Accuracy: {:.2}%",
            format_number(self.cash),
            self.get_accuracy() * 100.0
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PerPeriod {
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl PerPeriod {
    pub fn get_periods(&self) -> usize {
        match self {
            PerPeriod::Hourly => 60,
            PerPeriod::Daily => 60 * 24,
            PerPeriod::Weekly => 60 * 24 * 7,
            PerPeriod::Monthly => 60 * 24 * 30,
            PerPeriod::Yearly => 60 * 24 * 365,
        }
    }
}

impl Display for PerPeriod {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let period_str = match self {
            PerPeriod::Hourly => "Hourly",
            PerPeriod::Daily => "Daily",
            PerPeriod::Weekly => "Weekly",
            PerPeriod::Monthly => "Monthly",
            PerPeriod::Yearly => "Yearly",
        };

        write!(f, "{}", period_str)
    }
}

pub const fn test_headers() -> [&'static str; 11] {
    [
        "Cash ($)",
        "Accuracy (%)",
        "Ticker",
        "Score",
        "Correct/Incorrect",
        "Enter Price",
        "Exit Price",
        "Change (%)",
        "Enter Time",
        "Exit Time",
        "Fee Reduction",
    ]
}

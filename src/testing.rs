use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
};

use getset::{Getters, Setters};

use crate::math::format_number;

#[derive(Debug, Clone, Getters, Setters)]
#[getset(get = "pub")]
pub struct TestData {
    #[getset(set = "pub")]
    cash: f32,
    correct: usize,
    incorrect: usize,
    cash_history: Vec<f32>,
}

impl TestData {
    pub fn new(cash: f32) -> Self {
        Self {
            cash,
            correct: 0,
            incorrect: 0,
            cash_history: vec![cash],
        }
    }

    #[inline(always)]
    pub fn get_accuracy(&self) -> f32 {
        self.correct as f32 / (self.correct + self.incorrect) as f32
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
    pub fn add_cash(&mut self, cash: f32) {
        self.cash += cash;
        self.cash_history.push(self.cash);
    }

    #[inline]
    pub fn compute_average_return(
        &self,
        period: PerPeriod,
        interval: usize,
        multiplier: usize,
        data_points: usize,
    ) -> f32 {
        let periods = match period {
            PerPeriod::Hourly => (interval * data_points * multiplier) as f32 / 60.0,
            PerPeriod::Daily => (interval * data_points * multiplier) as f32 / (60.0 * 24.0),
            PerPeriod::Weekly => (interval * data_points * multiplier) as f32 / (60.0 * 24.0 * 7.0),
            PerPeriod::Monthly => {
                (interval * data_points * multiplier) as f32 / (60.0 * 24.0 * 30.0)
            }
            PerPeriod::Yearly => {
                (interval * data_points * multiplier) as f32 / (60.0 * 24.0 * 365.0)
            }
        };

        if data_points > 1 {
            let start_cash = self.cash_history[0];
            let final_cash = self.cash_history[self.cash_history.len() - 1];
            ((final_cash / start_cash).powf(1.0 / periods) - 1.0) * 100.0
        } else {
            0.0
        }
    }

    #[inline]
    pub fn calculate_average_returns(
        &self,
        interval: usize,
        multiplier: usize,
        data_points: usize,
    ) -> BTreeMap<PerPeriod, f32> {
        let mut average_returns: BTreeMap<PerPeriod, f32> = BTreeMap::new();
        let periods = [
            PerPeriod::Hourly,
            PerPeriod::Daily,
            PerPeriod::Weekly,
            PerPeriod::Monthly,
            PerPeriod::Yearly,
        ];

        for period in periods.iter() {
            let average_return =
                self.compute_average_return(*period, interval, multiplier, data_points);
            average_returns.insert(*period, average_return);
        }

        average_returns
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PerPeriod {
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
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

pub const fn test_headers() -> [&'static str; 9] {
    [
        "Cash ($)",
        "Accuracy (%)",
        "Ticker",
        "Score",
        "Correct/Incorrect",
        "Enter Price",
        "Exit Price",
        "Change (%)",
        "Time",
    ]
}

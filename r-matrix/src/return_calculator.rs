#[derive(Debug, Clone)]
pub struct ReturnCalculator {
    interval: usize,
    cash_history: Vec<f64>,
    hold_periods: usize,
}

impl ReturnCalculator {
    pub fn new(interval: usize, cash_history: Vec<f64>, hold_periods: usize) -> Self {
        ReturnCalculator {
            interval,
            cash_history,
            hold_periods,
        }
    }

    fn minutes_of_investment(&self) -> usize {
        self.interval * self.hold_periods
    }

    fn fv_and_pv(&self) -> (f64, f64) {
        let fv = *self.cash_history.last().unwrap();
        let pv = *self.cash_history.first().unwrap();
        (fv, pv)
    }

    // Public method to calculate average hourly return
    pub fn average_hourly_return(&self) -> f64 {
        let minutes = self.minutes_of_investment();
        let hours = minutes as f64 / 60.0;
        let (fv, pv) = self.fv_and_pv();
        (fv / pv).powf(1.0 / hours) - 1.0
    }

    // Public method to calculate average daily return
    pub fn average_daily_return(&self) -> f64 {
        let minutes = self.minutes_of_investment();
        let days = minutes as f64 / 60.0 / 24.0;
        let (fv, pv) = self.fv_and_pv();
        (fv / pv).powf(1.0 / days) - 1.0
    }

    // Public method to calculate average weekly return
    pub fn average_weekly_return(&self) -> f64 {
        let minutes = self.minutes_of_investment();
        let weeks = minutes as f64 / 60.0 / 24.0 / 7.0;
        let (fv, pv) = self.fv_and_pv();
        (fv / pv).powf(1.0 / weeks) - 1.0
    }

    // Public method to calculate average monthly return
    pub fn average_monthly_return(&self) -> f64 {
        let minutes = self.minutes_of_investment();
        let months = minutes as f64 / 60.0 / 24.0 / 30.0;
        let (fv, pv) = self.fv_and_pv();
        (fv / pv).powf(1.0 / months) - 1.0
    }
}

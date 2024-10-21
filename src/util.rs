use chrono::{DateTime, Utc};

pub struct Normalization {
    pub mean: f64,
    pub std_dev: f64,
}

impl Normalization {
    pub fn new(mean: f64, std_dev: f64) -> Self {
        Normalization { mean, std_dev }
    }

    pub fn from(data: &[f64]) -> Self {
        let mean = data.iter().sum::<f64>() / data.len() as f64;
        let variance = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / data.len() as f64;
        let std_dev = variance.sqrt();
        Normalization { mean, std_dev }
    }

    pub fn normalize(&self, value: f64) -> f64 {
        (value - self.mean) / self.std_dev
    }

    pub fn denormalize(&self, value: f64) -> f64 {
        (value * self.std_dev) + self.mean
    }

    pub fn normalize_vec(&self, data: &[f64]) -> Vec<f64> {
        data.iter().map(|x| self.normalize(*x)).collect()
    }

    pub fn denormalize_vec(&self, data: &[f64]) -> Vec<f64> {
        data.iter().map(|x| self.denormalize(*x)).collect()
    }
}

pub fn transpose<T>(matrix: &[Vec<T>]) -> Vec<Vec<T>>
where
    T: Clone,
{
    let mut transposed = Vec::new();
    for i in 0..matrix[0].len() {
        let mut row = Vec::new();
        for row_data in matrix {
            row.push(row_data[i].clone());
        }
        transposed.push(row);
    }
    transposed
}

/// Calculates the Calmar Ratio for a given cash history and investment period.
///
/// # Arguments
///
/// * `cash_history` - A slice of f64 representing the portfolio values over time.
/// * `start_date` - The start date of the investment period.
/// * `end_date` - The end date of the investment period.
///
/// # Returns
///
/// * `f64` - The calculated Calmar Ratio. Returns `f64::NAN` if calculation cannot be performed.
pub fn calmar_ratio(
    cash_history: &[f64],
    start_date: DateTime<Utc>,
    end_date: DateTime<Utc>,
) -> f64 {
    // Validation Checks
    if cash_history.len() < 2 {
        // Not enough data to compute returns
        return f64::NAN;
    }

    if end_date <= start_date {
        // Invalid date range
        return f64::NAN;
    }

    let first_value = cash_history.first().unwrap();
    let last_value = cash_history.last().unwrap();

    if *first_value <= 0.0 {
        // Invalid initial portfolio value
        return f64::NAN;
    }

    // Calculate Cumulative Return: (Last Value / First Value) - 1
    let cumulative_return = (last_value / first_value) - 1.0;

    // Calculate Total Investment Period in Years (including partial year)
    let duration = end_date.signed_duration_since(start_date);
    let total_seconds = duration.num_seconds() as f64;
    let seconds_in_year = 365.25 * 24.0 * 60.0 * 60.0; // Average seconds in a year accounting for leap years
    let total_years = total_seconds / seconds_in_year;

    if total_years <= 0.0 {
        // Invalid time period
        return f64::NAN;
    }

    // Annualize the Cumulative Return
    let annualized_return = (1.0 + cumulative_return).powf(1.0 / total_years) - 1.0;

    // Calculate Maximum Drawdown (MDD)
    let max_drawdown = calculate_max_drawdown(cash_history);

    if max_drawdown <= 0.0 {
        // Prevent division by zero or negative drawdown
        return f64::NAN;
    }

    // Calculate Calmar Ratio
    annualized_return / max_drawdown
}

/// Helper function to calculate the Maximum Drawdown (MDD) from a series of portfolio values.
///
/// # Arguments
///
/// * `cash_history` - A slice of f64 representing the portfolio values over time.
///
/// # Returns
///
/// * `f64` - The Maximum Drawdown as a positive decimal (e.g., 0.15 for 15%)
fn calculate_max_drawdown(cash_history: &[f64]) -> f64 {
    let mut peak = cash_history[0];
    let mut max_drawdown = 0.0;

    for &value in cash_history.iter().skip(1) {
        if value > peak {
            peak = value; // New peak
        } else {
            let drawdown = (peak - value) / peak;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }
    }

    max_drawdown
}

pub fn annualized_return(
    cash_history: &[f64],
    start_date: DateTime<Utc>,
    end_date: DateTime<Utc>,
) -> f64 {
    // Validation Checks
    if cash_history.len() < 2 {
        // Not enough data to compute returns
        return f64::NAN;
    }

    if end_date <= start_date {
        // Invalid date range
        return f64::NAN;
    }

    let first_value = cash_history.first().unwrap();
    let last_value = cash_history.last().unwrap();

    if *first_value <= 0.0 {
        // Invalid initial portfolio value
        return f64::NAN;
    }

    // Calculate Cumulative Return: (Last Value / First Value) - 1
    let cumulative_return = (last_value / first_value) - 1.0;

    // Calculate Total Investment Period in Years (including partial year)
    let duration = end_date.signed_duration_since(start_date);
    let total_seconds = duration.num_seconds() as f64;
    let seconds_in_year = 365.25 * 24.0 * 60.0 * 60.0; // Average seconds in a year accounting for leap years
    let total_years = total_seconds / seconds_in_year;

    if total_years <= 0.0 {
        // Invalid time period
        return f64::NAN;
    }

    // Annualize the Cumulative Return
    (1.0 + cumulative_return).powf(1.0 / total_years) - 1.0
}

#[inline]
pub fn format_number(num: f32) -> String {
    let original_num = num;
    let suffixes = [
        "", "K", "M", "B", "T", "Qa", "Qi", "Sx", "Sp", "Oc", "No", "De", "Ud", "Dd", "Td", "Qad",
        "Qid", "Sxd", "Spd", "Od", "Nd", "V", "Ct",
    ];

    let mut num = num;
    let mut index = 0;

    while num >= 1000.0 && index < suffixes.len() - 1 {
        num /= 1000.0;
        index += 1;
    }

    match original_num {
        n if n < 10.0 => format!("{:.3}{}", num, suffixes[index]),
        n if n < 100.0 => format!("{:.2}{}", num, suffixes[index]),
        _ => format!("{:.1}{}", num, suffixes[index]),
    }
}

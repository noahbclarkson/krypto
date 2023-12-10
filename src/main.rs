use std::error::Error;

use binance_r_matrix::{config::HistoricalDataConfig, historical_data::HistoricalData};
use r_matrix::{
    r_matrix::cmaes::{CMAESOptimize, RMatrixCMAESSettingsBuilder},
    return_calculator::ReturnCalculator,
    Dataset, RMatrix,
};

use crate::config::Config;

mod config;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::load()?;
    let h_config: HistoricalDataConfig = config.clone().into();
    let mut historical_data = HistoricalData::new(h_config);
    historical_data.load().await?;
    historical_data.calculate_technicals()?;
    let dataset = historical_data.to_dataset();
    let mut r_matrix: RMatrix = config.clone().into();
    let (train_data, test_data) = dataset.split(*config.train_test_split());
    let train = Box::new(train_data);
    let test = Box::new(test_data);
    r_matrix.train(&train).unwrap();
    let cmaes_settings = RMatrixCMAESSettingsBuilder::default()
        .optimize(CMAESOptimize::Cash)
        .with_individuals(*config.with_individuals())
        .interval(config.interval().to_minutes())
        .build()
        .unwrap();
    r_matrix.optimize(&test, cmaes_settings);
    r_matrix.plot_cash(&test).unwrap();
    let test_data = r_matrix.clone().test(&test);
    println!("{}", test_data);
    let returns = ReturnCalculator::new(
        config.interval().clone().to_minutes(),
        test_data.cash_history().clone(),
        *test_data.hold_periods(),
    );
    print_returns(returns.clone());
    optimize_for_min_score(&mut r_matrix, &test);
    let returns = ReturnCalculator::new(
        config.interval().clone().to_minutes(),
        test_data.cash_history().clone(),
        *test_data.hold_periods(),
    );
    print_returns(returns.clone());
    Ok(())
}

fn print_returns(calculator: ReturnCalculator) {
    println!(
        "Average Hourly Return: {:.2}%",
        calculator.average_hourly_return() * 100.0
    );
    println!(
        "Average Daily Return: {:.2}%",
        calculator.average_daily_return() * 100.0
    );
    println!(
        "Average Weekly Return: {:.2}%",
        calculator.average_weekly_return() * 100.0
    );
    println!(
        "Average Monthly Return: {:.2}%",
        calculator.average_monthly_return() * 100.0
    );
}

fn optimize_for_min_score(r_matrix: &mut RMatrix, dataset: &Dataset) {
    let mut best_min_score = 0.0;
    let mut best_cash = 0.0;
    for min_score in 0..=1000 {
        r_matrix.set_min_score(min_score as f64 / 1000000.0);
        let test_data = r_matrix.test(dataset);
        if test_data.cash() < &0.0 || test_data.get_accuracy().is_nan() {
            break;
        }
        println!(
            "Min Score: {} | Accuracy: {:.2}% | Cash: ${:.2}",
            min_score,
            test_data.get_accuracy() * 100.0,
            test_data.cash()
        );
        if *test_data.cash() > best_cash {
            best_min_score = min_score as f64 / 1000000.0;
            best_cash = *test_data.cash();
        }
    }
    println!(
        "Best Min Score: {} | Best Cash: ${:.2}",
        best_min_score, best_cash
    );
    r_matrix.set_min_score(best_min_score);
}

use std::{error::Error, io::Write as _};

use binance_r_matrix::{config::HistoricalDataConfig, historical_data::HistoricalData};
use krypto::{binance_interactions::wait, config::Config};
use r_matrix::{
    r_matrix::cmaes::{RMatrixCMAESSettings, RMatrixCMAESSettingsBuilder},
    return_calculator::ReturnCalculator,
    Dataset, RMatrix,
};
use rand::Rng as _;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::load()?;
    let h_config: HistoricalDataConfig = config.clone().into();
    let mut historical_data = HistoricalData::new(h_config);
    historical_data.load().await?;
    historical_data.calculate_technicals()?;
    let dataset = historical_data.to_dataset();
    let mut r_matrix: RMatrix = find_best_default_settings(&dataset, &config).into();
    r_matrix.train(&dataset)?;
    let cmaes_settings = RMatrixCMAESSettingsBuilder::default()
        .interval(config.interval().to_minutes())
        .optimize(r_matrix::r_matrix::cmaes::CMAESOptimize::Cash)
        .build()?;
    r_matrix.optimize(&dataset, cmaes_settings);
    let test_data = r_matrix.test(&dataset);
    let return_calculator = ReturnCalculator::new(
        config.interval().clone().to_minutes(),
        test_data.cash_history().clone(),
        *test_data.hold_periods(),
    );
    println!(
        "Accuracy: {:.2}% | Cash: ${:.2}",
        test_data.get_accuracy() * 100.0,
        test_data.cash()
    );
    print_returns(return_calculator);
    optimize_for_min_score(&mut r_matrix, &dataset);
    Ok(())
}

async fn get_latest_data(config: &Config) -> Dataset {
    let h_config: HistoricalDataConfig = config.clone().into();
    let mut historical_data = HistoricalData::new(h_config);
    historical_data.load().await.unwrap();
    historical_data.calculate_technicals().unwrap();
    historical_data.to_dataset()
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

const DEFAULT_SETTINGS_ITERATIONS: usize = 500;

fn find_best_default_settings(dataset: &Dataset, config: &Config) -> Config {
    let mut best_config = config.clone();
    println!("Finding best default settings...");
    println!("Default Settings:\n{}", best_config);
    let mut best_config_return = test(&best_config, dataset);
    let file = std::fs::File::create("best_config.txt").unwrap();
    let mut file = std::io::BufWriter::new(file);
    for _ in 0..DEFAULT_SETTINGS_ITERATIONS {
        let mut new_config = config.clone();
        randomize_config(&mut new_config);
        let new_config_return = test(&new_config, dataset);
        if new_config_return > best_config_return {
            best_config = new_config;
            best_config_return = new_config_return;
            println!("\nNew Best Settings: \nDepth: {}\nMax Forward Depth: {}\nOptimization-Algorithm: {}\nTickers: \n{}", best_config.depth(), best_config.forward_depth(), best_config.function_type().get_name(), best_config.tickers().join("\n"));
            println!("New Best Return: {:.2}%", new_config_return * 100.0);
            file.write_all(format!("{}\n", best_config).as_bytes())
                .unwrap();
            file.write_all(format!("Best Return: {:.2}%\n", new_config_return * 100.0).as_bytes())
                .unwrap();
            file.flush().unwrap();
        }
    }
    println!("Best Default Settings: {}", best_config);
    println!("Best Default Return: {:.2}%", best_config_return * 100.0);
    best_config
}

fn test(config: &Config, dataset: &Dataset) -> f64 {
    println!(
        "Testing settings: \nDepth: {}\nMax Forward Depth: {}\nOptimization-Algorithm: {}",
        config.depth(),
        config.forward_depth(),
        config.function_type().get_name()
    );
    let mut r_matrix: RMatrix = config.clone().into();
    let (train_data, test_data) = dataset.split(*config.train_test_split());
    let start = std::time::Instant::now();
    r_matrix.train(&train_data).unwrap();
    println!("Training took {:.2} seconds", start.elapsed().as_secs_f64());
    let test_data = r_matrix.test(&test_data);
    let return_calculator = ReturnCalculator::new(
        config.interval().clone().to_minutes(),
        test_data.cash_history().clone(),
        *test_data.hold_periods(),
    );
    println!(
        "Accuracy: {:.2}% | Cash: ${:.2}",
        test_data.get_accuracy() * 100.0,
        test_data.cash()
    );
    let daily = return_calculator.average_daily_return();
    println!("Average Daily Return: {:.2}%", daily * 100.0);
    daily
}

fn randomize_config(config: &mut Config) {
    let mut rng = rand::thread_rng();
    let depth = rng.gen_range(4..=30);
    let max_forward_depth = rng.gen_range(4..=depth);
    config.set_depth(depth);
    config.set_forward_depth(max_forward_depth);
}

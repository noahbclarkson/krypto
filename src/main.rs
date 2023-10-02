use std::error::Error;

use futures::future::try_join_all;
use krypto::{
    algorithm::{Algorithm, Ratio},
    config::Config,
    historical_data::HistoricalData,
    krypto_error::DataError,
    test_result::{PerPeriod, TestResult},
};

const R_TRAIN_RATIO: Ratio = Ratio {
    start: 0.0,
    end: 0.5,
};

const R_TEST_RATIO: Ratio = Ratio {
    start: 0.5,
    end: 0.8,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::read_config(None).await?;
    let data = get_data(&config).await?;
    let config = optimize(&config, &data).await;
    run_algorithm(&data, &config).await?;
    Ok(())
}

pub async fn get_data(config: &Config) -> Result<Box<[HistoricalData]>, DataError> {
    let futures: Vec<_> = config
        .intervals()
        .iter()
        .enumerate()
        .map(|(interval_index, _)| get_individual_data(config, interval_index))
        .collect();

    match try_join_all(futures).await {
        Ok(data) => Ok(data.into_boxed_slice()),
        Err(e) => Err(e),
    }
}

async fn get_individual_data(
    config: &Config,
    interval_index: usize,
) -> Result<HistoricalData, DataError> {
    let mut data = HistoricalData::load(&config, interval_index).await?;
    data.calculate_technicals()?;
    Ok(data)
}

async fn run_algorithm(data: &[HistoricalData], config: &Config) -> Result<(), Box<dyn Error>> {
    let ratio = Ratio::new(0.8, 1.0);
    let mut algorithm = Algorithm::new(data, &config, R_TRAIN_RATIO).await;
    algorithm.set_write_to_file(true);
    let test = algorithm.backtest(data, &config, ratio, true);
    println!("{}", test);
    Ok(())
}

async fn optimize(config: &Config, data: &[HistoricalData]) -> Config {
    let mut best_score = f64::MIN;
    let mut best_config = config.clone();
    let mut algorithm = Algorithm::new(data, &config, R_TRAIN_RATIO).await;
    for min in 0..=5000 {
        let mut config_clone = config.clone();
        let min = min as f64 / 500.0;
        config_clone.set_min_score(Some(min));

        for lev in 1..=5 {
            config_clone.set_leverage(lev as f64);
            let test = algorithm.backtest(data, &config_clone, R_TEST_RATIO, false);
            let score = test.get_return(
                PerPeriod::Daily,
                R_TEST_RATIO.get_periods(*config_clone.periods()) * config_clone.depth(),
                config_clone.interval_minutes(0),
            );
            if score > best_score {
                best_score = update_best_config(&mut best_config, score, &test, min, lev);
            }
        }
    }
    best_config
}

fn update_best_config(
    config: &mut Config,
    score: f64,
    test: &TestResult,
    min_score: f64,
    leverage: usize,
) -> f64 {
    println!("New best score: {}", test);
    println!("Average Daily Return: {:.4}%", score * 100.0);
    println!("Trades: {}", (test.cash_history().len() - 1) / 2);
    println!("Min Score: {}", min_score);
    println!("Leverage: {}", leverage);
    config.set_min_score(Some(min_score));
    config.set_leverage(leverage as f64);
    score
}

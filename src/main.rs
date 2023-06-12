use std::error::Error;

use krypto::{algorithm::Algorithm, config::*, historical_data::HistoricalData};

const MAX_DEPTH_TEST_END: usize = 12;
const MAX_DEPTH_TEST_START: usize = 8;
const MAX_MARGIN_TEST: f64 = 20.0;
const MAX_MINIMUM_SCORE_TEST: f64 = 0.04;
const MINIMUM_SCORE_STEP: f64 = 0.0001;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (tickers, config) = get_configuration().await?;
    let mut data = load_data(&tickers, &config).await;
    data.calculate_technicals();
    let mut algorithm = Algorithm::new(data);
    algorithm.compute_relationships();
    println!("Computed the relationships successfully");
    let test = algorithm.test(&config);
    println!("Initial Test Result: ");
    println!("{}", test);
    let mut best_result = *test.cash();
    // best_result = find_best_depth(&mut algorithm, &config, &best_result);
    // best_result = *algorithm.test(&config).cash();
    best_result = find_best_minimum_score(&mut algorithm, &config, &best_result);
    // find_best_margin(&mut algorithm, &config, &best_result);
    // algorithm.live_test(&config, &tickers).await?;
    Ok(())
}

pub async fn get_configuration() -> Result<(Vec<String>, Config), Box<dyn Error>> {
    let (tickers_res, config_res) = tokio::join!(read_tickers(), read_config());
    let tickers = tickers_res.unwrap_or_else(|_| {
        eprintln!("Failed to read tickers, using default values.");
        Config::get_default_tickers()
            .iter()
            .map(|s| s.to_string())
            .collect()
    });
    let config = config_res.unwrap_or_else(|_| {
        eprintln!("Failed to read config, using default values.");
        Config::default()
    });
    println!("Read the tickers and config successfully");
    Ok((tickers, config))
}

async fn load_data(tickers: &Vec<String>, config: &Config) -> HistoricalData {
    let mut data = HistoricalData::new(tickers);
    data.load(config).await;
    println!("Loaded the data successfully");
    data
}

#[allow(dead_code)]
pub fn find_best_depth(algorithm: &mut Algorithm, config: &Config, best_result: &f64) -> f64 {
    let mut best_depth = 0;
    let mut best_result = *best_result;
    for i in MAX_DEPTH_TEST_START..MAX_DEPTH_TEST_END + 1 {
        algorithm.settings.set_depth(i);
        algorithm.compute_relationships();
        let result = algorithm.test(config);
        if *result.cash() > best_result {
            println!("New best depth: {}", i);
            println!("New best result: {}", result);
            best_result = *result.cash();
            best_depth = i;
        }
    }
    algorithm.settings.set_depth(best_depth);
    algorithm.compute_relationships();
    best_result
}

#[allow(dead_code)]
pub fn find_best_margin(algorithm: &mut Algorithm, config: &Config, best_result: &f64) -> f64 {
    let mut best_margin = 0.0;
    let mut best_result = *best_result;
    for i in 1..MAX_MARGIN_TEST as usize {
        algorithm.settings.set_margin(i as f64);
        let result = algorithm.test(config);
        if *result.cash() > best_result {
            println!("New best margin: {}", i);
            println!("New best result: {}", result);
            best_result = *result.cash();
            best_margin = i as f64;
        }
    }
    algorithm.settings.set_margin(best_margin);
    best_result
}

#[allow(dead_code)]
pub fn find_best_minimum_score(
    algorithm: &mut Algorithm,
    config: &Config,
    best_result: &f64,
) -> f64 {
    let mut best_minimum_score = 0.0;
    let mut best_result = *best_result;
    for i in 1..(MAX_MINIMUM_SCORE_TEST / MINIMUM_SCORE_STEP) as usize {
        algorithm
            .settings
            .set_min_score(i as f64 * MINIMUM_SCORE_STEP);
        let result = algorithm.test(config);
        println!("Testing minimum score: {:.5}", i as f64 * MINIMUM_SCORE_STEP);
        println!("{}", result);
        if *result.cash() > best_result {
            println!("New best minimum score: {}", i as f64 * MINIMUM_SCORE_STEP);
            println!("New best result: {}", result);
            best_result = *result.cash();
            best_minimum_score = i as f64 * MINIMUM_SCORE_STEP;
        }
    }
    algorithm.settings.set_min_score(best_minimum_score);
    best_result
}

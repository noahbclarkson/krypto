
use krypto::{
    algorithm::algo::{Algorithm, AlgorithmSettings},
    config::KryptoConfig,
    data::interval::Interval, util::test_util::setup_default_data,
};
use tracing::info;

#[test]
#[ignore]
fn test_algorithm() {
    let config = KryptoConfig {
        start_date: "2018-01-01".to_string(),
        symbols: vec!["BTCUSDT".to_string()],
        intervals: vec![Interval::OneHour],
        cross_validations: 25,
        ..Default::default()
    };
    let (dataset, _gaurds) = setup_default_data("algorithm", Some(config.clone()));
    info!("Shape: {:?}", dataset.shape());
    let interval = dataset.keys().next().unwrap();
    let interval_data = dataset.get(interval).unwrap();
    let symbol = config.symbols[0].clone();
    let settings = AlgorithmSettings::new(3, 3, &symbol);
    let result = Algorithm::load(interval_data, settings, &config);
    match result {
        Ok(_) => {
            info!("Algorithm Loaded Successfully");
        }
        Err(e) => {
            panic!("Error: {}", e);
        }
    }
}

#[test]
#[ignore]
fn test_algo_on_all_data() {
    let config = KryptoConfig {
        start_date: "2019-01-01".to_string(),
        symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string(), "BNBUSDT".to_string()],
        intervals: vec![Interval::TwoHours],
        ..Default::default()
    };
    let (dataset, _gaurds) = setup_default_data("algo_on_all_unseen_data", Some(config.clone()));
    info!("Shape: {:?}", dataset.shape());
    let interval = dataset.keys().next().unwrap();
    let interval_data = dataset.get(interval).unwrap();
    let symbol = config.symbols[0].clone();
    let settings = AlgorithmSettings::new(10, 18, &symbol);
    let result = Algorithm::load(interval_data, settings, &config).unwrap();
    info!("Algorithm Loaded Successfully");
    let algo_result = result.backtest_on_all_seen_data(interval_data, &config).unwrap();
    info!("Algorithm Result: {}", algo_result);
}

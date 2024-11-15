use common::setup_default_data;
use krypto::{algorithm::algo::Algorithm, config::KryptoConfig, data::interval::Interval};
use tracing::info;

mod common;

#[test]
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
    let symbol = interval_data.keys().next().unwrap();
    let result = Algorithm::load(interval_data, 3, 3, symbol, &config);
    match result {
        Ok(_) => {
            info!("Algorithm Loaded Successfully");
        }
        Err(e) => {
            panic!("Error: {}", e);
        }
    }
}

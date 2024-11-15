use common::setup_default_data;
use krypto::{config::KryptoConfig, util::date_utils::MINS_TO_MILLIS};
use tracing::info;

mod common;

#[test]
fn test_data_load() {
    let _ = setup_default_data("data_load", None);
}

#[test]
fn test_data_shape() {
    let (dataset, _gaurds) = setup_default_data("data_shape", None);
    let shape = dataset.shape();
    info!("{:?}", shape);
    assert_eq!((shape.0, shape.1), (2, 2));
    for value in dataset.values() {
        let data_lengths = value
            .values()
            .map(|d| d.get_candles().len())
            .collect::<Vec<_>>();
        let technicals_lengths = value
            .values()
            .map(|d| d.get_technicals().len())
            .collect::<Vec<_>>();
        let labels_lengths = value
            .values()
            .map(|d| d.get_labels().len())
            .collect::<Vec<_>>();
        assert!(data_lengths.iter().all(|&x| x == data_lengths[0]));
        assert!(technicals_lengths
            .iter()
            .all(|&x| x == technicals_lengths[0]));
        assert!(labels_lengths.iter().all(|&x| x == labels_lengths[0]));
    }
}

#[test]
fn test_data_times_match() {
    let config = KryptoConfig {
        start_date: "2021-02-02".to_string(),
        symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string(), "BNBUSDT".to_string(), "ADAUSDT".to_string(), "XRPUSDT".to_string()],
        ..Default::default()
    };
    let (dataset, _gaurds) = setup_default_data("data_times_match", Some(config));
    for (key, value) in dataset.get_map() {
        let maximum_variance = key.to_minutes() * MINS_TO_MILLIS / 2;
        let symbol_datas = value.values();
        let times = symbol_datas
            .map(|d| {
                d.get_candles()
                    .clone()
                    .iter()
                    .map(|v| v.close_time)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        for i in 0..times[0].len() {
            for j in 0..times.len() {
                for k in 0..times.len() {
                    let difference = (times[j][i] - times[k][i]).abs();
                    let difference = difference.num_milliseconds();
                    assert!(difference <= maximum_variance, "Difference: {}", difference);
                }
            }
        }
    }
}

use core::f64;

use krypto::{
    algorithm::algo::{Algorithm, AlgorithmSettings},
    config::KryptoConfig,
    data::{dataset::Dataset, technicals::TECHNICAL_COUNT},
    error::KryptoError,
    logging::setup_tracing,
};
use tracing::info;

pub fn main() {
    let (_, file_guard) = setup_tracing(Some("logs")).expect("Failed to set up tracing");
    let result = run();
    if let Err(e) = result {
        eprintln!("Error: {:?}", e);
    }
    drop(file_guard);
}

fn run() -> Result<(), KryptoError> {
    let config = KryptoConfig::read_config::<&str>(None)?;
    let dataset = Dataset::load(&config).unwrap();
    let mut algorithms = Vec::new();
    let mut best_return = f64::NEG_INFINITY;
    let mut best_algorithm = None;
    let mut i = 0;
    let mut csv =
        csv::Writer::from_path("results.csv").map_err(|e| KryptoError::CsvError(e.to_string()))?;
    csv.write_record([
        "n",
        "depth",
        "ticker",
        "monthly_return",
        "accuracy",
        "interval",
    ])
    .map_err(|e| KryptoError::CsvError(e.to_string()))?;
    for (interval, interval_data) in dataset.get_map() {
        info!("Interval: {}", interval);
        for symbol in interval_data.keys() {
            info!("Symbol: {}", symbol);
            for n in 1..50 {
                for depth in 1..75 {
                    if n >= depth * TECHNICAL_COUNT {
                        continue;
                    }
                    let settings = AlgorithmSettings::new(n, depth, symbol);
                    let algorithm = Algorithm::load(interval_data, settings, &config)?;
                    if algorithm.get_monthly_return() > best_return {
                        best_return = algorithm.get_monthly_return();
                        best_algorithm = Some(i);
                        info!("New best algorithm: {}", &algorithm);
                    }
                    csv.write_record(&[
                        n.to_string(),
                        depth.to_string(),
                        symbol.to_string(),
                        algorithm.get_monthly_return().to_string(),
                        algorithm.get_accuracy().to_string(),
                        interval.to_string(),
                    ])
                    .map_err(|e| KryptoError::CsvError(e.to_string()))?;
                    csv.flush()
                        .map_err(|e| KryptoError::CsvError(e.to_string()))?;
                    i += 1;
                    algorithms.push(algorithm);
                }
            }
        }
    }
    let best_algorithm = algorithms.get(best_algorithm.unwrap());
    info!("Best Algorithm: {}", &best_algorithm.unwrap());
    Ok(())
}

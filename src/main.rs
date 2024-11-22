use core::f64;

use krypto::{
    algorithm::algo::{Algorithm, AlgorithmSettings},
    config::KryptoConfig,
    data::dataset::Dataset,
    error::KryptoError,
    logging::setup_tracing,
};
use tracing::{error, info};

const MAX_N: usize = 50;
const MAX_DEPTH: usize = 50;

pub fn main() {
    let (_, file_guard) = setup_tracing(Some("logs")).expect("Failed to set up tracing");
    let result = run();
    if let Err(e) = result {
        error!("Error: {:?}", e);
    }
    drop(file_guard);
}

fn run() -> Result<(), KryptoError> {
    let config = KryptoConfig::read_config::<&str>(None)?;
    let dataset = Dataset::load(&config)?;

    let mut best_return = f64::NEG_INFINITY;
    let mut best_algorithm: Option<Algorithm> = None;

    let mut csv = csv::Writer::from_path("results.csv")?;

    csv.write_record([
        "n",
        "depth",
        "ticker",
        "monthly_return",
        "accuracy",
        "interval",
    ])?;

    for (interval, interval_data) in dataset.get_map() {
        info!("Interval: {}", interval);
        let all_settings = AlgorithmSettings::all(config.symbols.clone(), MAX_N, MAX_DEPTH);
        for settings in all_settings {
            let algorithm = Algorithm::load(interval_data, settings.clone(), &config)?;
            let monthly_return = algorithm.get_monthly_return();
            csv.write_record(&[
                settings.n.to_string(),
                settings.depth.to_string(),
                settings.symbol.to_string(),
                monthly_return.to_string(),
                algorithm.get_accuracy().to_string(),
                interval.to_string(),
            ])?;

            if monthly_return > best_return {
                best_return = monthly_return;
                info!("New best algorithm: {}", &algorithm);
                best_algorithm = Some(algorithm);
            }

            csv.flush()?;
        }
    }

    match best_algorithm {
        Some(algorithm) => info!("Best Algorithm: {}", &algorithm),
        None => info!("No algorithm found."),
    }

    Ok(())
}

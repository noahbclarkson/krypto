use clap::Parser;
use krypto::{
    algorithm::Algorithm,
    args::Args,
    candlestick::Candlestick,
    config::Config,
    historical_data::HistoricalData,
    krypto_app::{get_configuration, KryptoApp},
    testing::PerPeriod,
};
use std::error::Error;

#[tokio::main]
async fn main() {
    let result = match_gui().await;
    match result {
        Ok(_) => (),
        Err(e) => println!("Error: {}", e),
    }
}

async fn match_gui() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    match args.gui() {
        Some(true) => run_gui()?,
        _ => run_cli(&args).await?,
    }
    Ok(())
}

fn run_gui() -> Result<(), Box<dyn Error>> {
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(640.0, 480.0)),
        ..Default::default()
    };
    eframe::run_native(
        "Krypto",
        options,
        Box::new(|_cc| Box::<KryptoApp>::default()),
    )?;
    Ok(())
}

async fn run_cli(args: &Args) -> Result<(), Box<dyn Error>> {
    let (tickers, config) = get_configuration().await?;
    let mut data = HistoricalData::new(&tickers);
    data.load(&config, None).await?;
    data.calculate_candlestick_technicals()?;
    data.normalize_technicals();
    let candles = data.candles();
    let mut algorithm = match args.optimize() {
        Some(true) => find_best_parameters(&config, candles).await,
        _ => {
            let mut algorithm = Algorithm::new(&config);
            algorithm.compute_relationships(candles).await;
            algorithm
        }
    };
    match args.backtest() {
        Some(true) => {
            let test = algorithm.test(candles);
            println!("{}", test);
        }
        _ => (),
    }
    match args.livetest() {
        Some(true) => {
            let test = algorithm.live_test(&config, &tickers).await?;
            println!("{}", test);
        }
        _ => (),
    }
    Ok(())
}

#[allow(dead_code)]
async fn find_best_parameters(config: &Config, candles: &Vec<Vec<Candlestick>>) -> Algorithm {
    let mut best_return = 0.0;
    let mut best_config = None;
    let mut best_settings = None;
    let interval_num = config.get_interval_minutes().unwrap();
    let mut results_file = csv::Writer::from_path("results.csv").unwrap();
    let headers = vec!["min_score", "depth", "cash", "accuracy", "return"];
    results_file.write_record(&headers).unwrap();
    let mut algorithm = Algorithm::new(&config);
    for depth in 3..15 {
        algorithm.settings_mut().set_depth(depth);
        algorithm.compute_relationships(candles).await;
        for i in 0..50 {
            let min_score = i as f32 / 5.0;
            algorithm.settings_mut().set_min_score(Some(min_score));
            let test = algorithm.test(candles);
            let test_return =
                test.compute_average_return(PerPeriod::Daily, interval_num as usize, depth);
            if test_return > best_return {
                best_return = test_return;
                best_config = Some(config.clone());
                best_settings = Some(algorithm.settings().clone());
                println!(
                    "New best: ({:.2}, {}): {} with daily return: {:.2}%",
                    min_score, depth, test, test_return
                );
            }
            if test.get_accuracy().is_nan() {
                break;
            }
            let record = vec![
                min_score.to_string(),
                depth.to_string(),
                test.cash().to_string(),
                test.get_accuracy().to_string(),
                test_return.to_string(),
            ];
            results_file.write_record(&record).unwrap();
            results_file.flush().unwrap();
        }
    }
    let mut algorithm = Algorithm::new(&best_config.unwrap());
    algorithm.set_settings(best_settings.unwrap());
    algorithm.compute_relationships(candles).await;
    algorithm
}

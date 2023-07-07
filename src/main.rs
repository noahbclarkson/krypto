use std::error::Error;

use krypto::{
    algorithm::{backtest, compute_relationships, livetest},
    config::{load_configuration, Config},
    historical_data::{calculate_technicals, load, TickerData},
    testing::PerPeriod,
};

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    // let args = Args::parse();
    let (mut config, tickers) = load_configuration().await?;
    println!("Loaded configuration");
    let candles = load(config.as_mut(), tickers.clone()).await?;
    let candles = calculate_technicals(candles);
    let relationships = compute_relationships(candles.as_ref(), config.as_ref()).await;
    let test = backtest(candles.as_ref(), relationships.as_ref(), config.as_ref());
    println!("{}", test);
    // let _config = find_best_parameters(config.as_mut(), candles.as_ref()).await;
    livetest(tickers, config.as_ref()).await?;
    Ok(())
}

#[allow(dead_code)]
async fn find_best_parameters(config: &mut Config, candles: &[TickerData]) -> Config {
    let mut best_return = 0.0;
    let mut best_config = config.clone();
    let interval_num = config.interval_minutes().unwrap() as usize;
    let mut results_file = csv::Writer::from_path("results.csv").unwrap();
    let headers = vec!["min_score", "depth", "cash", "accuracy", "return"];
    results_file.write_record(&headers).unwrap();
    for depth in 4..15 {
        let config = config.set_depth(depth);
        let relationships = compute_relationships(candles, config).await;
        for i in 0..50 {
            let min_score = i as f32 / 50.0;
            let config = config.set_min_score(Some(min_score));
            let test = backtest(candles, relationships.as_ref(), config);
            let test_return = test.compute_average_return(
                PerPeriod::Daily,
                interval_num,
                depth,
                config.periods() - depth * 2,
            );
            if test_return > best_return {
                best_return = test_return;
                best_config = config.clone();
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
    best_config
}

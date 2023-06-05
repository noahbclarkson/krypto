use krypto::{
    algorithm::Algorithm,
    config::{read_config, read_tickers, Config},
    historical_data::HistoricalData,
};
use std::error::Error;

const DEFAULT_ITERATIONS: usize = 20;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (mut tickers, mut config) = get_configuration().await?;

    let mut algorithm = create_new_algorithm(&mut tickers, &mut config).await?;

    algorithm
        .live_test(algorithm.ticker.clone().unwrap().as_str(), &config, &tickers)
        .await.unwrap_or_else(|e| {
            eprintln!("Failed to live test the algorithm.");
            eprintln!("{}", e);
            std::process::exit(1);
        });

    Ok(())
}

async fn create_new_algorithm(mut tickers: &mut Vec<String>, config: &mut Config) -> Result<Algorithm, Box<dyn Error>> {
    let mut data = load_data(tickers.clone(), &config).await;
    let mut t = data.get_tickers().clone();
    tickers = &mut t;

    data.calculate_technicals();
    println!("Calculated the technicals successfully");

    let mut algorithm = Algorithm::new(data);
    algorithm.calculate_relationships();
    println!("Calculated the relationships successfully");

    find_highest_ticker(&tickers, &mut algorithm, &config);

    let iterations = get_user_input("How many iterations would you like to perform to optimize the algorithm's weights?", |input| {
        input.parse::<usize>().map_err(|_| "Please enter a valid number.".into())
    })?;

    algorithm.optimize_weights(&config, iterations);

    find_highest_margin(&mut algorithm, &config);

    config.set_margin(algorithm.margin.unwrap());

    Ok(algorithm)
}

pub async fn get_configuration() -> Result<(Vec<String>, Config), Box<dyn Error>> {
    let (tickers_res, config_res) = tokio::join!(read_tickers(), read_config());
    let tickers = tickers_res.unwrap_or_else(|_| {
        eprintln!("Failed to read tickers, using default values.");
        Config::get_default_tickers().iter().map(|s| s.to_string()).collect()
    });
    let config = config_res.unwrap_or_else(|_| {
        eprintln!("Failed to read config, using default values.");
        Config::get_default_config()
    });
    println!("Read the tickers and config successfully");
    Ok((tickers, config))
}

async fn load_data(tickers: Vec<String>, config: &Config) -> HistoricalData {
    let mut data = HistoricalData::new(&tickers);
    data.load_data(tickers, config).await;
    println!("Loaded the data successfully");
    data
}

fn find_highest_ticker(
    tickers: &[String],
    algorithm: &mut Algorithm,
    config: &Config,
){
    let mut highest_ticker = "";
    let mut highest_cash = 0.0;

    for ticker in tickers {
        let test = algorithm.test(ticker, config, false);
        if *test.cash() > highest_cash {
            highest_ticker = ticker;
            highest_cash = *test.cash();
        }
    }

    algorithm.ticker = Some(highest_ticker.to_string());
}

fn find_highest_margin(
    algorithm: &mut Algorithm,
    config: &Config,
) {
    let mut highest_margin = 0.0;

    let mut test = algorithm.test(algorithm.ticker.clone().unwrap().as_str(), &config, false);

    for i in 1..=DEFAULT_ITERATIONS {
        let margin = i as f64;
        let mut config = config.clone();
        config.set_margin(margin);
        let current_test = algorithm.test(algorithm.ticker.clone().unwrap().as_str(), &config, false);
        if test.cash() < current_test.cash() {
            highest_margin = margin;
            test = current_test;
        }
    }

    println!("The highest margin is {}", highest_margin);
    println!("{}", test);

    algorithm.margin = Some(highest_margin);
}

fn get_user_input<T, F: FnOnce(&str) -> Result<T, Box<dyn Error>>>(
    prompt: &str,
    parse: F,
) -> Result<T, Box<dyn Error>> {
    println!("{}", prompt);

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim();

    parse(input)
}
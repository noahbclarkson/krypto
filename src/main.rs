use krypto::{
    algorithm::Algorithm,
    config::{read_config, read_tickers, Config},
    historical_data::HistoricalData, math::format_number,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Run read_tickers and read_config concurrently
    let (tickers, config) = get_configuration().await;
    // Get the latest data
    let mut data = load_data(tickers, &config).await;
    // Set the tickers to the valid tickers
    let tickers = data.get_tickers();
    // Calculate the technicals
    data.calculate_technicals();
    println!("Calculated the technicals successfully");
    // Calculate the relationships
    let mut algorithm = Algorithm::new(data);
    algorithm.calculate_relationships();
    println!("Calculated the relationships successfully");
    // Find the best ticker to trade
    let (highest_ticker, highest_cash) = find_highest_ticker(&tickers, &algorithm, &config);
    let highest_ticker = highest_ticker.as_str();
    // Optimize the algorithm's weights
    algorithm.optimize_weights(highest_ticker, &config, 1500);
    // Find the margin that gives the highest cash
    println!("Finding highest margin");
    let (highest_margin, _) = find_highest_margin(highest_ticker, &algorithm, &config, highest_cash);
    // Set the margin to the highest margin
    let mut config = config.clone();
    config.set_margin(highest_margin);
    // Run the live test
    algorithm.live_test(highest_ticker, &config).await;
    Ok(())
}

pub async fn get_configuration() -> (Vec<String>, Config) {
    let (tickers, config) = tokio::join!(read_tickers(), read_config());
    let tickers = tickers.unwrap_or_else(|_| {
        eprintln!("Failed to read tickers, using default values.");
        Config::get_default_tickers().iter().map(|s| s.to_string()).collect()
    });
    let config = config.unwrap_or_else(|_| {
        eprintln!("Failed to read config, using default values.");
        Config::get_default_config()
    });
    println!("Read the tickers and config successfully");
    (tickers, config)
}

async fn load_data(tickers: Vec<String>, config: &Config) -> HistoricalData {
    let mut data = HistoricalData::new(&tickers.clone());
    data.load_data(tickers.clone(), &config).await;
    println!("Loaded the data successfully");
    data
}

fn find_highest_ticker(tickers: &Vec<String>, algorithm: &Algorithm, config: &Config) -> (String, f64) {
    let mut highest_ticker = "";
    let mut highest_cash = 0.0;
    for ticker in tickers {
        println!("Testing {}", ticker);
        let test = algorithm.test(ticker, &config);
        if *test.cash() > highest_cash {
            highest_ticker = ticker;
            highest_cash = *test.cash();
        }
    }
    println!("Highest ticker: {}", highest_ticker);
    println!("Highest cash: ${}", format_number(highest_cash));
    (highest_ticker.to_string(), highest_cash)
}

fn find_highest_margin(ticker: &str, algorithm: &Algorithm, config: &Config, mut highest_cash: f64) -> (f64, f64) {
    let mut highest_margin = 0.0;
    for i in 1..21 {
        let margin = i as f64;
        let mut config = config.clone();
        config.set_margin(margin);
        let test = algorithm.test(ticker, &config);
        if *test.cash() > highest_cash {
            highest_margin = margin;
            highest_cash = *test.cash();
        }
    }
    println!("Highest margin: {}", highest_margin);
    println!("Highest cash: ${}", format_number(highest_cash));
    (highest_margin, highest_cash)
}
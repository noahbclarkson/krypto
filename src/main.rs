use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc, Mutex},
    thread,
    time::Duration,
};

use binance::websockets::{WebSockets, WebsocketEvent};
use krypto::{
    algorithm::{get_pls, load_algorithm},
    candlestick::Candlestick,
    dataset::{self, normalize, DataArray, Dataset, Key},
    krypto_account::KryptoAccount,
    logging::setup_tracing,
    technicals::compute_technicals,
    KryptoConfig, KryptoError,
};
use linfa::traits::Predict as _;
use linfa_pls::PlsRegression;
use ndarray::Array2;
use tracing::{debug, info, instrument};

#[tokio::main]
pub async fn main() -> Result<(), KryptoError> {
    let (_, file_guard) = setup_tracing().expect("Failed to set up tracing");
    let result = run().await;
    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }
    drop(file_guard);
    Ok(())
}

#[instrument(level = "info")]
async fn run() -> Result<(), KryptoError> {
    let mut config: KryptoConfig = KryptoConfig::read_config(None::<&str>)?;
    let dataset = Dataset::load_from_binance(&config).await?;
    let mut best_result = None;
    for i in 1..=6 {
        let mut config_clone = config.clone();
        config_clone.pls_components = i;
        let mut results = load_algorithm(&config_clone, &dataset).await?;
        results.sort();
        results.reverse();
        let best = results.first().unwrap();
        info!("PLS Components: {}", i);
        info!("Best Result for PLS:");
        info!("{}", best);
        match &best_result {
            None => {
                best_result = Some(best.clone());
                config = config_clone;
            }
            Some(current_best) if best > current_best => {
                best_result = Some(best.clone());
                config = config_clone;
            }
            _ => {}
        }
    }
    let mut results = load_algorithm(&config, &dataset).await?;
    results.sort();
    results.reverse();
    let (dataset, _) = normalize(dataset, None);
    let best = results.first().unwrap();
    info!("Best Result:");
    info!("{}", best);
    debug!("All Results:");
    for result in results.iter() {
        debug!("{}", result);
    }
    let ticker = best.ticker.clone();
    let interval = best.interval;
    let data = dataset
        .data
        .get(&(ticker.clone(), interval))
        .unwrap()
        .clone();
    let keep_running = AtomicBool::new(true);
    let pls = get_pls(&data, &config);
    let data = Arc::new(Mutex::new(data));
    let key = (ticker, interval);
    setup_web_socket(data, key, keep_running, pls, &config).await;
    Ok(())
}

const QUANTITY_MULTIPLIER: f64 = 0.75;

#[instrument(level = "info", skip(data, key, keep_running, pls, config))]
async fn setup_web_socket(
    data: Arc<Mutex<DataArray>>,
    key: Key,
    keep_running: AtomicBool,
    pls: PlsRegression<f64>,
    config: &KryptoConfig,
) {
    info!("Setting up web-socket");
    let (ticker, interval) = key.clone();
    let data = data.clone();
    let kline = format!("{}@kline_{}", ticker.clone().to_lowercase(), interval);
    let mut ka = KryptoAccount::new(config);
    let precision = ka.get_precision_data(ticker.clone()).await.unwrap();
    debug!("Listening to: {}", kline);
    let mut currently_holding = false;
    let ticker = ticker.clone();
    let mut web_socket = WebSockets::new(|event: WebsocketEvent| {
        if let WebsocketEvent::Kline(kline_event) = event {
            if !kline_event.kline.is_final_bar {
                return Ok(());
            }
            let kline = Candlestick::from_kline(kline_event.kline).unwrap();
            let mut data = data.lock().unwrap();
            info!("Received kline: {}", kline);
            data.data.push(kline);
            dataset::sort(&mut data);
            dataset::dedeup(&mut data);
            dataset::compute_percentage_change(&mut data);
            compute_technicals(&mut data);
            let mut map = HashMap::new();
            map.insert(key.clone(), data.clone());
            let dataset = Dataset { data: map };
            let (dataset, _) = normalize(dataset, None);
            let d = dataset.data.get(&key).unwrap().clone();
            data.replace(d.data);
            let features = data.data.last().unwrap().features.as_ref().unwrap();
            let input_features: Array2<f64> =
                Array2::from_shape_vec((1, features.len()), features.clone()).unwrap();
            let prediction = pls.predict(&input_features);
            let prediction = prediction.into_raw_vec()[0];
            debug!("Raw Prediction: {}", prediction);
            let prediction = prediction.signum() as usize;
            let buy_bool = match prediction == 1 {
                true => {
                    info!("Prediction: Buy");
                    true
                }
                false => {
                    info!("Prediction: Sell");
                    false
                }
            };
            debug!(
                "Buy: {}, Currently Holding: {}",
                buy_bool, currently_holding
            );
            if buy_bool && !currently_holding {
                let balances = ka.account.get_account().unwrap().balances;
                let balance = balances
                    .iter()
                    .find(|balance| balance.asset == "USDT")
                    .unwrap();
                let price = ka.market.get_price(key.0.as_str()).unwrap().price;
                let quantity = (balance.free.parse::<f64>().unwrap() * QUANTITY_MULTIPLIER) / price;
                let quantity = precision.fmt_quantity(quantity).unwrap();
                let transaction = ka.account.market_buy(ticker.as_str(), quantity).unwrap();
                currently_holding = true;
                info!(
                    "Bought: {} {} at {}",
                    quantity,
                    ticker,
                    transaction.clone().fills.unwrap()[0].price
                );
                debug!("Transaction: {:?}", transaction);
            } else if !buy_bool && currently_holding {
                let balances = ka.account.get_account().unwrap().balances;
                let balance = balances
                    .iter()
                    .find(|balance| balance.asset == ticker.as_str().replace("USDT", ""))
                    .unwrap();
                let quantity = precision
                    .fmt_quantity(balance.free.parse::<f64>().unwrap())
                    .unwrap();
                let transaction = ka.account.market_sell(ticker.as_str(), quantity).unwrap();
                currently_holding = false;
                info!(
                    "Sold: {} {} at {}",
                    quantity,
                    ticker,
                    transaction.clone().fills.unwrap()[0].price
                );
                debug!("Transaction: {:?}", transaction);
            }
            thread::sleep(Duration::from_secs(1));
            let balances = ka.account.get_account().unwrap().balances;
            let usdt_balance = balances
                .iter()
                .find(|balance| balance.asset == "USDT")
                .unwrap()
                .free
                .parse::<f64>()
                .unwrap();
            info!("USDT Balance: ${}", usdt_balance);
            let asset_balance = balances
                .iter()
                .find(|balance| balance.asset == ticker.as_str().replace("USDT", ""))
                .unwrap()
                .free
                .parse::<f64>()
                .unwrap();
            info!("{} Balance: {}", ticker.replace("USDT", ""), asset_balance);
            let total_balance = usdt_balance
                + (asset_balance * ka.market.get_price(ticker.as_str()).unwrap().price);
            info!("Total Balance in USDT: ${}", total_balance);
        }
        Ok(())
    });
    web_socket.connect(&kline).unwrap();
    if let Err(e) = web_socket.event_loop(&keep_running) {
        let err = e;
        {
            println!("Error: {:?}", err);
        }
    }
    web_socket.disconnect().unwrap();
}

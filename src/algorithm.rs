use std::{error::Error, sync::Arc, time::Duration};

use binance::rest_model::OrderSide;
use chrono::Utc;
use getset::Getters;
use once_cell::sync::Lazy;
use tokio::{sync::Mutex, task};

use crate::{
    candlestick::TECHNICAL_COUNT,
    config::Config,
    historical_data::{calculate_technicals, load, TickerData, MINS_TO_MILLIS},
    krypto_account::KryptoAccount,
    math::percentage_change,
    order_event::{OrderDetails, OrderEvent},
    testing::{test_headers, TestData},
};

const MARGIN: f32 = 0.05;
const STARTING_CASH: f32 = 1000.0;
const WAIT_WINDOW: i64 = 10000;
const ENTRY_TIME_PERCENT: f64 = 0.075;
const EXIT_TIME_PERCENT: f64 = 0.05;

static BLACKLIST_INDEXES: Lazy<Mutex<Option<Vec<usize>>>> = Lazy::new(|| Mutex::new(None));

#[derive(Debug, Clone, PartialEq, Getters)]
#[getset(get = "pub")]
pub struct Relationship {
    correlation: f32,
    depth: usize,
    r_type: usize,
    target_index: usize,
    predict_index: usize,
}

pub async fn compute_relationships(candles: &[TickerData], config: &Config) -> Box<[Relationship]> {
    let mut relationships = Vec::new();
    for (target_index, target_candles) in candles.iter().enumerate() {
        let tasks = candles
            .iter()
            .enumerate()
            .map(|(predict_index, predict_candles)| {
                compute_relationship(
                    target_index,
                    predict_index,
                    target_candles,
                    predict_candles,
                    *config.depth(),
                )
            });
        futures::future::join_all(tasks)
            .await
            .into_iter()
            .for_each(|mut new_relationships| relationships.append(&mut new_relationships));
    }
    Box::from(relationships)
}

async fn compute_relationship(
    target_index: usize,
    predict_index: usize,
    target_candles: &TickerData,
    predict_candles: &TickerData,
    depth: usize,
) -> Vec<Relationship> {
    let mut results = vec![Vec::new(); TECHNICAL_COUNT * depth];
    for i in depth + 1..predict_candles.candles().len() - 1 {
        let target = &target_candles.candles()[i + 1].p_change().clone();
        for d in 0..depth {
            for (j, technical) in target_candles.candles()[i - d]
                .technicals()
                .iter()
                .enumerate()
            {
                results[d * TECHNICAL_COUNT + j].push((technical * target).tanh());
            }
        }
    }
    let correlations = results
        .iter()
        .map(|v| v.iter().sum::<f32>() / v.len() as f32)
        .collect::<Vec<f32>>();
    let mut relationships = Vec::new();
    for d in 0..depth {
        for j in 0..TECHNICAL_COUNT {
            let correlation = correlations[d * TECHNICAL_COUNT + j];
            relationships.push(Relationship {
                correlation,
                depth: d + 1,
                r_type: j,
                target_index,
                predict_index,
            });
        }
    }
    relationships
}

#[inline(always)]
pub async fn predict(
    relationships: &[Relationship],
    current_position: usize,
    candles: &[TickerData],
    config: &Config,
) -> (usize, f32) {
    let blacklist = config.blacklist().clone().unwrap_or_default();
    let tickers = config.tickers().clone();
    let blacklist_indexes_task = task::spawn(get_blacklist_indexes(tickers, blacklist));

    let mut scores = vec![0.0; candles.len()];
    for relationship in relationships {
        for d in 0..relationship.depth {
            let predict = candles[relationship.predict_index].candles()[current_position - d]
                .technicals()[relationship.r_type];
            scores[relationship.target_index] += (predict * relationship.correlation).tanh();
        }
    }

    let blacklist_indexes = blacklist_indexes_task.await.unwrap();

    let mut max_index = None;
    let mut max = None;
    for (i, score) in scores.iter().enumerate().skip(1) {
        if (max_index.is_none() || max_index.is_none() || score > max.unwrap())
            && !blacklist_indexes.contains(&i)
        {
            max_index = Some(i);
            max = Some(score);
        }
    }
    (max_index.unwrap(), *max.unwrap())
}

async fn get_blacklist_indexes(tickers: Vec<String>, blacklist: Vec<String>) -> Vec<usize> {
    let mut data = BLACKLIST_INDEXES.lock().await;
    match &*data {
        Some(indexes) => indexes.clone(),
        None => {
            let indexes: Vec<usize> = tickers
                .iter()
                .enumerate()
                .filter_map(|(index, ticker)| {
                    if blacklist.contains(ticker) {
                        Some(index)
                    } else {
                        None
                    }
                })
                .collect();

            *data = Some(indexes.clone());
            indexes
        }
    }
}

pub async fn backtest(
    candles: &[TickerData],
    relationships: &[Relationship],
    config: &Config,
) -> TestData {
    let mut test = TestData::new(STARTING_CASH);

    for i in *config.depth()..*config.periods() - *config.depth() {
        let (index, score) = predict(relationships, i, candles, config).await;
        if score > config.min_score().unwrap_or_default() {
            let current_price = candles[index].candles()[i].close();
            let exit_price = candles[index].candles()[i + *config.depth()].close();

            let change = percentage_change(*current_price, *exit_price);
            let fee_change = test.cash() * config.fee().unwrap_or_default() * MARGIN;

            test.add_cash(-fee_change);
            test.add_cash(test.cash() * MARGIN * change);

            match change {
                x if x > 0.0 => test.add_correct(),
                x if x < 0.0 => test.add_incorrect(),
                _ => (),
            }

            if *test.cash() <= 0.0 {
                test.set_cash(0.0);
                break;
            }
        }
    }
    test
}

pub async fn livetest(config: &Config) -> Result<(), Box<dyn Error>> {
    let mut test = TestData::new(STARTING_CASH);
    let depth = *config.depth();
    let min_score = config.min_score().unwrap_or_default();
    let fee = config.fee().unwrap_or_default();

    let mut enter_price: Option<f32> = None;
    let mut last_index: Option<usize> = None;
    let mut last_score: Option<f32> = None;

    let mut file = csv::Writer::from_path("livetest.csv")?;
    let headers = test_headers();
    file.write_record(headers)?;
    file.flush()?;

    loop {
        let candles = load_new_data(config, 1).await;
        let candles = match candles {
            Ok(candles) => candles,
            Err(err) => {
                println!("Error: {}", err);
                wait(config, 1).await?;
                continue;
            }
        };
        let candles = calculate_technicals(candles);
        let relationships = compute_relationships(&candles, config).await;
        wait(config, 1).await?;
        let mut c_clone = config.clone();
        c_clone.set_periods(1000);
        let lc = load_new_data(&c_clone, 3).await;
        let lc = match lc {
            Ok(lc) => lc,
            Err(err) => {
                println!("Error: {}", err);
                wait(config, 1).await?;
                continue;
            }
        };
        let lc = calculate_technicals(lc);
        if enter_price.is_some() && last_index.is_some() {
            let ep = enter_price.unwrap();
            let li = last_index.unwrap();
            let current_price = lc[li].candles()[999].close();
            let change = percentage_change(ep, *current_price);
            let fee_change = test.cash() * fee * MARGIN;

            test.add_cash(-fee_change);
            test.add_cash(test.cash() * MARGIN * change);

            match change {
                x if x > 0.0 => test.add_correct(),
                x if x < 0.0 => test.add_incorrect(),
                _ => (),
            }

            if *test.cash() <= 0.0 {
                test.set_cash(0.0);
                break;
            }

            println!(
                "{}: ${:.5} -> ${:.5} ({:.3}%)",
                lc[li].ticker(),
                ep,
                current_price,
                change
            );
            println!("{}", test);

            let record = vec![
                test.cash().to_string(),
                test.get_accuracy().to_string(),
                lc[li].ticker().to_string(),
                last_score.unwrap().to_string(),
                match change {
                    x if x > 0.0 => "Correct".to_string(),
                    x if x < 0.0 => "Incorrect".to_string(),
                    _ => "None".to_string(),
                },
                ep.to_string(),
                current_price.to_string(),
                change.to_string(),
                chrono::Utc::now().to_rfc3339(),
            ];

            file.write_record(&record).unwrap_or_else(|err| {
                println!("Error writing record: {}", err);
            });
            file.flush().unwrap_or_else(|err| {
                println!("Error flushing file: {}", err);
            });
        }

        let (index, score) = predict(&relationships, 999, &lc, config).await;
        if score > min_score {
            let current_price = lc[index].candles()[999].close();
            enter_price = Some(*current_price);
            last_index = Some(index);
            last_score = Some(score);

            println!("Entered {} at ${:.5}", lc[index].ticker(), current_price);

            wait(config, depth - 1).await?;
        } else {
            enter_price = None;
            last_index = None;
            last_score = None;
            println!("No trade ({:.5} < {})", score, min_score);
        }
    }

    Ok(())
}

pub async fn run(config: &Config) -> Result<(), Box<dyn Error>> {
    let depth = *config.depth();
    let min_score = config.min_score().unwrap_or_default();
    let interval_mins = config.interval_minutes()?;
    let order_len = depth as i64 * interval_mins;

    let kr = Arc::new(Mutex::new(KryptoAccount::new(config)));
    kr.lock().await.update_exchange_info().await?;
    let mut test = TestData::new(kr.lock().await.get_balance().await? as f32);

    let mut file = csv::Writer::from_path("live.csv")?;
    let headers = test_headers();
    file.write_record(headers)?;
    let starting_records = vec![
        test.cash().to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        chrono::Utc::now().to_rfc3339(),
    ];
    file.write_record(&starting_records)?;
    file.flush()?;

    let mut candles = load_new_data(config, 2).await?;
    candles = calculate_technicals(candles);
    let mut relationships = compute_relationships(&candles, config).await;

    loop {
        wait(config, 1).await?;

        let mut c_clone = config.clone();
        c_clone.set_periods(1000);
        let lc = load_new_data(&c_clone, 3).await;
        let lc = match lc {
            Ok(lc) => lc,
            Err(err) => {
                println!("Error: {}", err);
                wait(config, 1).await?;
                continue;
            }
        };
        let lc = calculate_technicals(lc);

        let (index, score) = predict(&relationships, 999, &lc, config).await;
        if score > min_score {
            let ticker = lc[index].ticker();
            let (max_entry_time, min_exit_time) = get_entry_and_exit_times(order_len);
            let order_details = OrderDetails {
                ticker: ticker.to_string(),
                side: OrderSide::Buy,
                quantity: None,
                max_time: Some(max_entry_time),
            };
            println!("Buying {} ({:.5})", ticker, score);
            let order = OrderEvent::new(order_details, kr.lock().await.to_owned()).await;

            if order.is_err() {
                println!("Error: {}", order.unwrap_err());
                continue;
            }

            let order = order?;
            let enter_price = order.current_order_price().unwrap();
            let qty = order.details().quantity.unwrap();
            let update_time = (min_exit_time - Utc::now().timestamp_millis()) / 2;
            tokio::time::sleep(Duration::from_millis(update_time as u64)).await;
            let c_result = load_new_data(config, 1).await;

            if c_result.is_err() {
                println!("Error: {}", c_result.unwrap_err());
                continue;
            }

            candles = c_result.unwrap();
            candles = calculate_technicals(candles);
            relationships = compute_relationships(&candles, config).await;
            kr.lock().await.update_exchange_info().await?;
            let update_time = min_exit_time - Utc::now().timestamp_millis();
            tokio::time::sleep(Duration::from_millis(update_time as u64)).await;

            loop {
                let lc = load_new_data(&c_clone, 1).await;
                if lc.is_err() {
                    println!("Error: {}", lc.unwrap_err());
                    break;
                } else {
                    let c = lc.unwrap();
                    let c = calculate_technicals(c);
                    let (index_2, score_2) = predict(&relationships, 999, &c, config).await;
                    if score_2 > 0.0 && index_2 == index {
                        let (_, min_exit_time_2) = get_entry_and_exit_times(order_len);
                        println!("Continuing to hold {} ({:.5})", ticker, score_2);
                        let update_time_2 = min_exit_time_2 - Utc::now().timestamp_millis();
                        tokio::time::sleep(Duration::from_millis(update_time_2 as u64)).await;
                    } else {
                        break;
                    }
                }
            }

            let details = OrderDetails {
                ticker: ticker.to_string(),
                side: OrderSide::Sell,
                quantity: Some(qty),
                max_time: None,
            };

            let order = OrderEvent::new(details, kr.lock().await.to_owned()).await;

            if order.is_err() {
                println!("Error loading order event");
                println!("This could be an issue! Check your account!");
                continue;
            }

            let order = order?;
            let exit_price = order.current_order_price().unwrap();
            let change = percentage_change(enter_price as f32, exit_price as f32);
            test.set_cash(kr.lock().await.get_balance().await? as f32);
            match change {
                x if x > 0.0 => {
                    test.add_correct();
                }
                x if x < 0.0 => {
                    test.add_incorrect();
                }
                _ => (),
            }

            let record = vec![
                test.cash().to_string(),
                test.get_accuracy().to_string(),
                ticker.to_string(),
                score.to_string(),
                match change {
                    x if x > 0.0 => "Correct".to_string(),
                    x if x < 0.0 => "Incorrect".to_string(),
                    _ => "None".to_string(),
                },
                enter_price.to_string(),
                exit_price.to_string(),
                change.to_string(),
                chrono::Utc::now().to_rfc3339(),
            ];

            file.write_record(&record).unwrap_or_else(|err| {
                println!("Error writing record: {}", err);
            });

            file.flush().unwrap_or_else(|err| {
                println!("Error flushing file: {}", err);
            });
        } else {
            println!("No trade ({:.5} < {})", score, min_score);
        }
    }
}

fn get_entry_and_exit_times(order_length: i64) -> (i64, i64) {
    let entry_amount = ENTRY_TIME_PERCENT * order_length as f64;
    let max_entry_time = Utc::now().timestamp_millis() + (entry_amount as i64 * MINS_TO_MILLIS);
    let exit_amount = (1.0 - EXIT_TIME_PERCENT) * order_length as f64;
    let min_exit_time = Utc::now().timestamp_millis() + (exit_amount as i64 * MINS_TO_MILLIS);
    (max_entry_time, min_exit_time)
}

async fn load_new_data(
    config: &Config,
    repeats: usize,
) -> Result<Box<[TickerData]>, Box<dyn Error>> {
    let repeat_count = 0;
    let mut error = None;
    while repeat_count <= repeats {
        let new_candles = load(config).await;
        match new_candles {
            Ok(new_candles) => {
                return Ok(new_candles);
            }
            Err(err) => {
                error = Some(err);
            }
        }
    }
    Err(error.unwrap())
}

#[inline]
async fn wait(config: &Config, periods: usize) -> Result<(), Box<dyn Error>> {
    for _ in 0..periods {
        loop {
            let now = Utc::now().timestamp_millis();
            let millis = config.interval_minutes()? * MINS_TO_MILLIS;
            let next_interval = (now / millis) * millis + millis;
            let wait_time = next_interval - now - WAIT_WINDOW;
            if wait_time > WAIT_WINDOW {
                tokio::time::sleep(Duration::from_millis(wait_time as u64)).await;
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(WAIT_WINDOW as u64 + 1)).await;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {

    use crate::historical_data::{calculate_technicals, load};

    use super::*;

    #[tokio::test]
    #[ignore = "Invalid for CI"]
    async fn test_compute_relationships() {
        let config = Config::default();
        let candles = load(&config).await.unwrap();
        let candles = calculate_technicals(candles);
        let relationships = compute_relationships(&candles, &config).await;
        assert_eq!(
            relationships.len(),
            config.tickers().len().pow(2) * TECHNICAL_COUNT * config.depth()
        );
    }

    #[tokio::test]
    #[ignore = "Invalid for CI"]
    async fn test_predict() {
        let config = Config::default();
        let candles = load(&config).await.unwrap();
        let candles = calculate_technicals(candles);
        let relationships = compute_relationships(&candles, &config).await;
        let (index, score) = predict(&relationships, *config.depth(), &candles, &config).await;
        assert!(score != 0.0);
        assert!(index < config.tickers().len());
    }
}

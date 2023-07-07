use std::{error::Error, time::Duration};

use chrono::Utc;
use getset::Getters;

use crate::{
    candlestick::TECHNICAL_COUNT,
    config::Config,
    historical_data::{calculate_technicals, load, TickerData, MINUTES_TO_MILLIS},
    math::percentage_change,
    testing::TestData,
};

const MARGIN: f32 = 0.1;
const STARTING_CASH: f32 = 1000.0;

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
                    &target_candles,
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
pub fn predict(
    relationships: &[Relationship],
    current_position: usize,
    candles: &[TickerData],
) -> (usize, f32) {
    let mut scores = vec![0.0; candles.len()];
    for relationship in relationships {
        for d in 0..relationship.depth {
            let predict = candles[relationship.predict_index].candles()[current_position - d]
                .technicals()[relationship.r_type];
            scores[relationship.target_index] += predict * relationship.correlation;
        }
    }
    let mut max_index = 0;
    let mut max = scores[0];
    for i in 1..scores.len() {
        if scores[i] > max {
            max_index = i;
            max = scores[i];
        }
    }
    (max_index, max)
}

pub fn backtest(
    candles: &[TickerData],
    relationships: &[Relationship],
    config: &Config,
) -> TestData {
    let mut test = TestData::new(STARTING_CASH);
    let depth = config.depth().clone();
    let periods = config.periods().clone();
    let min_score = config.min_score().unwrap_or_default();
    let fee = config.fee().unwrap_or_default();

    for i in depth..periods - depth {
        let (index, score) = predict(relationships, i, candles);
        if score > min_score {
            let current_price = candles[index].candles()[i].close();
            let exit_price = candles[index].candles()[i + depth].close();
            let change = percentage_change(*current_price, *exit_price);
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
        }
    }
    test
}

pub async fn livetest(tickers: Vec<String>, config: &Config) -> Result<(), Box<dyn Error>> {
    let mut test = TestData::new(STARTING_CASH);
    let depth = config.depth().clone();
    let min_score = config.min_score().unwrap_or_default();
    let fee = config.fee().unwrap_or_default();
    let data_size = depth + 15;

    let mut enter_price: Option<f32> = None;
    let mut last_index: Option<usize> = None;
    let mut last_score: Option<f32> = None;

    let mut file = csv::Writer::from_path("livetest.csv")?;
    let headers = vec![
        "Cash ($)",
        "Accuracy (%)",
        "Ticker",
        "Score",
        "Correct/Incorrect",
        "Enter Price",
        "Exit Price",
        "Change (%)",
        "Time",
    ];
    file.write_record(&headers)?;
    file.flush()?;

    loop {
        let candles = load(config, tickers.clone()).await?;
        let candles = calculate_technicals(candles);
        let relationships = compute_relationships(candles.as_ref(), config).await;
        wait(config, 1).await?;
        let mut c_clone = config.clone();
        c_clone.set_periods(data_size);
        let lc = load(&c_clone, tickers.clone()).await?;
        let lc = calculate_technicals(lc);
        if enter_price.is_some() && last_index.is_some() {
            let ep = enter_price.unwrap();
            let li = last_index.unwrap();
            let current_price = lc[li].candles()[data_size - 1].close();
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

            file.write_record(&record)?;
            file.flush()?;
        }

        let (index, score) = predict(relationships.as_ref(), data_size - 1, lc.as_ref());
        if score > min_score {
            let current_price = lc[index].candles()[data_size - 1].close();
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

const WAIT_WINDOW: i64 = 5000;

async fn wait(config: &Config, periods: usize) -> Result<(), Box<dyn Error>> {
    for _ in 0..periods {
        loop {
            let now = Utc::now().timestamp_millis();
            let millis = (config.interval_minutes()? * MINUTES_TO_MILLIS) as i64;
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

    use crate::{
        config::DEFAULT_TICKERS,
        historical_data::{calculate_technicals, load},
    };

    use super::*;

    #[tokio::test]
    async fn test_compute_relationships() {
        let config = Config::default();
        let tickers = DEFAULT_TICKERS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let candles = load(&config, tickers.clone()).await.unwrap();
        let candles = calculate_technicals(candles);
        let relationships = compute_relationships(candles.as_ref(), &config).await;
        assert_eq!(
            relationships.len(),
            tickers.len().pow(2) * TECHNICAL_COUNT * config.depth()
        );
    }
}

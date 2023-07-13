use getset::Getters;
use once_cell::sync::Lazy;
use tokio::{sync::Mutex, task};

use crate::{candlestick::TECHNICAL_COUNT, config::Config, historical_data::TickerData};

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
        if (max_index.is_none() || score > max.unwrap()) && !blacklist_indexes.contains(&i) {
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

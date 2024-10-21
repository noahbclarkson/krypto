use binance::market::Market;
use chrono::{DateTime, TimeZone as _, Utc};
use tracing::{debug, info, instrument};

use crate::{
    candlestick::{map_to_candlesticks, Candlestick},
    connection,
    interval::Interval,
    technicals::compute_technicals,
    util::{self, Normalization},
    KryptoConfig, KryptoError,
};
use std::collections::HashMap;

//
pub type Key = (String, Interval);

// 1 minute in milliseconds
const MINS_TO_MILLIS: i64 = 60 * 1000;

#[derive(Debug, Clone)]
pub struct Dataset {
    pub data: HashMap<Key, DataArray>,
}

#[derive(Debug, Clone)]
pub struct DataArray {
    pub data: Vec<Candlestick>,
}

impl Dataset {
    #[instrument(skip(config))]
    pub async fn load_from_binance(config: &KryptoConfig) -> Result<Self, KryptoError> {
        let market: Market = config.get_binance();
        info!(
            "Loading data from Binance: tickers: {:?}, intervals: {:?}",
            config.tickers, config.intervals
        );
        let mut tasks = Vec::new();
        let keys = config.tickers.iter().flat_map(|ticker| {
            config
                .intervals
                .iter()
                .map(move |interval| (ticker.clone(), *interval))
        });
        info!(
            "Getting data for {} keys",
            config.tickers.len() * config.intervals.len()
        );
        for key in keys {
            let start_date = config.start_date()?;
            let start_time = Utc.from_utc_datetime(
                &start_date
                    .and_hms_opt(0, 0, 0)
                    .ok_or(KryptoError::InvalidDateTime)?,
            );
            let end_time = Utc::now();
            let timestamps = get_timestamps(start_time, end_time, key.1)?;
            for (start, end) in timestamps {
                let market_clone = market.clone();
                let key = key.clone();
                tasks.push(tokio::spawn(async move {
                    connection::check_and_wait(2).await;
                    get_chunk(key, start as u64, end as u64, market_clone).await
                }));
            }
        }
        let results = futures::future::join_all(tasks).await;
        let mut data = HashMap::new();
        for result in results {
            let (key, candlesticks) = result??;
            data.entry(key)
                .or_insert_with(|| DataArray { data: Vec::new() })
                .data
                .extend(candlesticks);
        }
        let mut output = Self { data };
        debug!("Sorting data");
        output.apply_function_to_data(sort);
        debug!("Removing duplicates");
        output.apply_function_to_data(dedeup);
        debug!("Computing percentage change");
        output.apply_function_to_data(compute_percentage_change);
        debug!("Computing technicals");
        output.apply_function_to_data(compute_technicals);
        Ok(output)
    }

    fn apply_function_to_data<F>(&mut self, function: F)
    where
        F: Fn(&mut DataArray),
    {
        for (_, data) in self.data.iter_mut() {
            function(data);
        }
    }

    pub async fn to_csvs(&self, folder: Option<&str>) -> Result<(), KryptoError> {
        let folder = folder.unwrap_or("./data");
        std::fs::create_dir_all(folder)?;
        for (key, data) in self.data.iter() {
            let filename = format!("{}-{}.csv", key.0, key.1);
            let path = format!("{}/{}", folder, filename);
            let file = std::fs::File::create(path)?;
            let mut writer = csv::Writer::from_writer(file);
            for candle in data.data.iter() {
                writer.serialize(candle)?;
            }
        }
        Ok(())
    }

    pub fn split(&self, split: f64) -> (Self, Self) {
        let mut train = HashMap::new();
        let mut test = HashMap::new();
        for (key, data) in self.data.iter() {
            let split_index = (data.data.len() as f64 * split) as usize;
            let (train_data, test_data) = data.data.split_at(split_index);
            train.insert(
                key.clone(),
                DataArray {
                    data: train_data.to_vec(),
                },
            );
            test.insert(
                key.clone(),
                DataArray {
                    data: test_data.to_vec(),
                },
            );
        }
        let (train, test) = normalize(Dataset { data: train }, Some(Dataset { data: test }));
        (train, test.unwrap())
    }

}

#[instrument(skip(test, train))]
pub fn normalize(mut train: Dataset, mut test: Option<Dataset>) -> (Dataset, Option<Dataset>) {
    debug!("Normalizing data");
    for ((ticker, interval), data) in train.data.iter_mut() {
        let technicals = data
            .data
            .iter()
            .map(|c| c.technicals.get_array().to_vec())
            .collect::<Vec<_>>();
        let transposed = util::transpose(&technicals);
        let normalizations = transposed
            .iter()
            .map(|row| Normalization::from(row))
            .collect::<Vec<_>>();
        let transposed_normalized = transposed
            .iter()
            .zip(normalizations.iter())
            .map(|(row, norm)| norm.normalize_vec(row))
            .collect::<Vec<_>>();
        let mut normalized = util::transpose(&transposed_normalized);
        for row in normalized.iter_mut() {
            for value in row.iter_mut() {
                if value.is_nan() || value.is_infinite() {
                    *value = 0.0;
                }
            }
        }
        for (candle, row) in data.data.iter_mut().zip(normalized.iter()) {
            candle.features = Some(row.clone());
        }
        if let Some(ref mut test_data_set) = test {
            if let Some(test_data) = test_data_set.data.get_mut(&(ticker.clone(), *interval)) {
                let test_technicals = test_data
                    .data
                    .iter()
                    .map(|c| c.technicals.get_array().to_vec())
                    .collect::<Vec<_>>();
                let test_transposed = util::transpose(&test_technicals);
                let test_normalized = test_transposed
                    .iter()
                    .zip(normalizations.iter())
                    .map(|(row, norm)| norm.normalize_vec(row))
                    .collect::<Vec<_>>();
                let test_normalized = util::transpose(&test_normalized);
                for (candle, row) in test_data.data.iter_mut().zip(test_normalized.iter()) {
                    candle.features = Some(row.clone());
                }
            }
        }
    }
    (train, test)
}

impl DataArray {
    pub fn replace(&mut self, data: Vec<Candlestick>) {
        self.data = data;
    }
}


pub fn sort(data: &mut DataArray) {
    data.data.sort_by(|a, b| a.open_time.cmp(&b.open_time));
}

pub fn dedeup(data: &mut DataArray) {
    data.data.dedup_by(|a, b| a.open_time == b.open_time);
}

pub fn compute_percentage_change(data: &mut DataArray) {
    for i in 1..data.data.len() {
        let previous = &data.data[i - 1];
        let current = &data.data[i];
        let percentage_change = (current.close - previous.close) / previous.close;
        data.data[i].percentage_change = Some(percentage_change);
    }
}

async fn get_chunk(
    key: Key,
    start_time: u64,
    end_time: u64,
    market: Market,
) -> Result<(Key, Vec<Candlestick>), KryptoError> {
    let data = market
        .get_klines(
            key.0.as_str(),
            key.1.to_string().as_str(),
            1000u16,
            Some(start_time),
            Some(end_time),
        )?;
    let candlesticks = map_to_candlesticks(data)?;
    Ok((key, candlesticks))
}

fn get_timestamps(
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    interval: Interval,
) -> Result<Vec<(i64, i64)>, KryptoError> {
    let mut timestamps = Vec::new();
    let mut current_time = start_time.timestamp_millis();
    let end_time = end_time.timestamp_millis();
    let interval_millis = interval.to_minutes() * MINS_TO_MILLIS;
    while current_time < end_time {
        let next_time = current_time + interval_millis * 1000;
        let next_time = if next_time > end_time {
            end_time
        } else {
            next_time
        };
        timestamps.push((current_time, next_time));
        current_time = next_time;
    }
    Ok(timestamps)
}

use super::{candlestick::Candlestick, interval::Interval, technicals::Technicals};
use crate::{
    algorithm::algo::AlgorithmSettings,
    config::KryptoConfig,
    error::KryptoError,
    util::{
        date_utils::{date_to_datetime, get_timestamps},
        matrix_utils::normalize_by_columns,
    },
};
use binance::market::Market;
use chrono::Utc;
use std::collections::HashMap;
use tracing::{debug, info, instrument};

pub struct Dataset {
    interval_data_map: HashMap<Interval, IntervalData>,
}

impl Dataset {
    #[instrument(skip(config))]
    pub fn load(config: &KryptoConfig) -> Result<Self, KryptoError> {
        let mut interval_data_map = HashMap::new();
        let market: Market = config.get_binance();

        for interval in &config.intervals {
            let interval = *interval;
            let interval_data = IntervalData::load(&interval, config, &market)?;
            info!("Loaded data for {}", &interval);
            interval_data_map.insert(interval, interval_data);
        }

        Ok(Self { interval_data_map })
    }

    pub fn shape(&self) -> (usize, usize, Vec<usize>) {
        let dim_1 = self.len();
        let dim_2 = self.values().next().map(|d| d.len()).unwrap_or(0);
        let mut dim_3s = Vec::new();
        for interval_data in self.values() {
            let dim_3 = interval_data.values().next().map(|d| d.len()).unwrap_or(0);
            dim_3s.push(dim_3);
        }
        (dim_1, dim_2, dim_3s)
    }

    pub fn len(&self) -> usize {
        self.interval_data_map.len()
    }

    pub fn get(&self, interval: &Interval) -> Option<&IntervalData> {
        self.interval_data_map.get(interval)
    }

    pub fn get_map(&self) -> &HashMap<Interval, IntervalData> {
        &self.interval_data_map
    }

    pub fn values(&self) -> impl Iterator<Item = &IntervalData> {
        self.interval_data_map.values()
    }

    pub fn keys(&self) -> impl Iterator<Item = &Interval> {
        self.interval_data_map.keys()
    }

    pub fn is_empty(&self) -> bool {
        self.interval_data_map.is_empty()
    }
}

pub struct IntervalData {
    symbol_data_map: HashMap<String, SymbolData>,
}

impl IntervalData {
    #[instrument(skip(config, market))]
    fn load(
        interval: &Interval,
        config: &KryptoConfig,
        market: &Market,
    ) -> Result<Self, KryptoError> {
        let mut symbol_data_map = HashMap::new();
        let end = Utc::now().timestamp_millis();

        for symbol in &config.symbols {
            let symbol = symbol.clone();
            let symbol_data = SymbolData::load(interval, &symbol, end, config, market)?;
            info!("Loaded data for {}", &symbol);
            symbol_data_map.insert(symbol, symbol_data);
        }

        Ok(Self { symbol_data_map })
    }

    pub fn len(&self) -> usize {
        self.symbol_data_map.len()
    }

    fn get(&self, symbol: &str) -> Option<&SymbolData> {
        self.symbol_data_map.get(symbol)
    }

    fn values(&self) -> impl Iterator<Item = &SymbolData> {
        self.symbol_data_map.values()
    }

    pub fn is_empty(&self) -> bool {
        self.symbol_data_map.is_empty()
    }

    // Get all the technicals for all the symbols. Each row contains all the tecnhicals for each of
    // the symbols at a given time.
    fn get_records(&self) -> Vec<Vec<f64>> {
        let mut records = Vec::new();
        for i in 0..self.values().next().unwrap().len() {
            let mut record = Vec::new();
            for symbol_data in self.values() {
                let technicals = symbol_data.get_technicals();
                record.extend(technicals[i].as_array().to_vec());
            }
            records.push(record);
        }
        records
    }

    pub fn get_labels(&self, symbol: &str) -> &Vec<f64> {
        self.get(symbol).unwrap().get_labels()
    }

    pub fn get_candles(&self, symbol: &str) -> &Vec<Candlestick> {
        self.get(symbol).unwrap().get_candles()
    }

    pub fn get_technicals(&self, symbol: &str) -> &Vec<Technicals> {
        self.get(symbol).unwrap().get_technicals()
    }

    #[instrument(skip(settings, self))]
    pub fn get_symbol_dataset(&self, settings: &AlgorithmSettings) -> SymbolDataset {
        let records = self.get_records();
        let normalized_predictors = normalize_by_columns(records)
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|v| if v.is_nan() { 0.0 } else { v })
                    .collect()
            })
            .collect::<Vec<Vec<f64>>>();

        let features = normalized_predictors
            .windows(settings.depth)
            .map(|window| window.iter().flatten().cloned().collect())
            .collect::<Vec<Vec<f64>>>();
        let features = features[..features.len() - 1].to_vec();

        let symbol_data = self
            .get(&settings.symbol)
            .expect("Symbol not found in dataset");

        let labels: Vec<f64> = symbol_data
            .get_labels()
            .iter()
            .skip(settings.depth)
            .map(|&v| if v.is_nan() { 1.0 } else { v })
            .collect();

        let candles: Vec<Candlestick> = symbol_data
            .get_candles()
            .iter()
            .skip(settings.depth)
            .cloned()
            .collect();

        debug!("Features shape: {}x{}", features.len(), features[0].len());
        debug!("Labels count: {}", labels.len());
        debug!("Candles count: {}", candles.len());

        SymbolDataset::new(features, labels, candles)
    }
}

pub struct SymbolDataset {
    features: Vec<Vec<f64>>,
    labels: Vec<f64>,
    candles: Vec<Candlestick>,
}

impl SymbolDataset {
    pub fn new(features: Vec<Vec<f64>>, labels: Vec<f64>, candles: Vec<Candlestick>) -> Self {
        Self {
            features,
            labels,
            candles,
        }
    }

    pub fn get_features(&self) -> &Vec<Vec<f64>> {
        &self.features
    }

    pub fn get_labels(&self) -> &Vec<f64> {
        &self.labels
    }

    pub fn get_candles(&self) -> &Vec<Candlestick> {
        &self.candles
    }

    pub fn len(&self) -> Result<usize, KryptoError> {
        if self.features.len() != self.labels.len() || self.features.len() != self.candles.len() {
            Err(KryptoError::InvalidDataset)
        } else {
            Ok(self.features.len())
        }
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty() || self.labels.is_empty() || self.candles.is_empty()
    }
}

struct SymbolData {
    candles: Vec<Candlestick>,
    technicals: Vec<Technicals>,
    labels: Vec<f64>,
}

impl SymbolData {
    #[instrument(skip(interval, end, config, market))]
    fn load(
        interval: &Interval,
        symbol: &str,
        end: i64,
        config: &KryptoConfig,
        market: &Market,
    ) -> Result<Self, KryptoError> {
        let mut candles = Vec::new();
        let start = date_to_datetime(&config.start_date()?)?;
        let timestamps = get_timestamps(start.timestamp_millis(), end, *interval)?;

        for (start, end) in timestamps {
            let mut chunk = Self::load_chunk(market, symbol, interval, start, end)?;
            candles.append(&mut chunk);
        }

        candles.sort_by_key(|c| c.open_time);
        candles.dedup_by_key(|c| c.open_time);

        let technicals = Technicals::get_technicals(&candles);
        let mut labels = vec![0.0];
        for i in 1..candles.len() {
            let percentage_change =
                (candles[i].close - candles[i - 1].close) / candles[i - 1].close;
            labels.push(percentage_change.signum());
        }
        debug!(
            "Loaded {} candles ({} labels | {}x{} technicals)",
            candles.len(),
            labels.len(),
            technicals.len(),
            technicals[0].as_array().len()
        );
        Ok(Self {
            candles,
            technicals,
            labels,
        })
    }

    fn load_chunk(
        market: &Market,
        symbol: &str,
        interval: &Interval,
        start: i64,
        end: i64,
    ) -> Result<Vec<Candlestick>, KryptoError> {
        let summaries = market
            .get_klines(
                symbol,
                interval.to_string(),
                1000u16,
                Some(start as u64),
                Some(end as u64),
            )
            .map_err(|e| KryptoError::BinanceApiError(e.to_string()))?;
        let candlesticks = Candlestick::map_to_candlesticks(summaries)?;
        Ok(candlesticks)
    }

    pub fn len(&self) -> usize {
        self.candles.len()
    }

    pub fn get_candles(&self) -> &Vec<Candlestick> {
        &self.candles
    }

    pub fn get_technicals(&self) -> &Vec<Technicals> {
        &self.technicals
    }

    pub fn get_labels(&self) -> &Vec<f64> {
        &self.labels
    }
}

#[cfg(test)]
mod tests {
    use tracing::info;

    use crate::{
        config::KryptoConfig,
        util::{date_utils::MINS_TO_MILLIS, test_util::setup_default_data},
    };

    #[test]
    #[ignore]
    fn test_data_load() {
        let _ = setup_default_data("data_load", None);
    }

    #[test]
    #[ignore]
    fn test_data_shape() {
        let (dataset, _gaurds) = setup_default_data("data_shape", None);
        let shape = dataset.shape();
        info!("{:?}", shape);
        assert_eq!((shape.0, shape.1), (2, 2));
        for value in dataset.values() {
            let data_lengths = value
                .values()
                .map(|d| d.get_candles().len())
                .collect::<Vec<_>>();
            let technicals_lengths = value
                .values()
                .map(|d| d.get_technicals().len())
                .collect::<Vec<_>>();
            let labels_lengths = value
                .values()
                .map(|d| d.get_labels().len())
                .collect::<Vec<_>>();
            assert!(data_lengths.iter().all(|&x| x == data_lengths[0]));
            assert!(technicals_lengths
                .iter()
                .all(|&x| x == technicals_lengths[0]));
            assert!(labels_lengths.iter().all(|&x| x == labels_lengths[0]));
        }
    }

    #[test]
    #[ignore]
    fn test_data_times_match() {
        let config = KryptoConfig {
            start_date: "2021-02-02".to_string(),
            symbols: vec![
                "BTCUSDT".to_string(),
                "ETHUSDT".to_string(),
                "BNBUSDT".to_string(),
                "ADAUSDT".to_string(),
                "XRPUSDT".to_string(),
            ],
            ..Default::default()
        };
        let (dataset, _gaurds) = setup_default_data("data_times_match", Some(config));
        for (key, value) in dataset.get_map() {
            let maximum_variance = key.to_minutes() * MINS_TO_MILLIS / 2;
            let symbol_datas = value.values();
            let times = symbol_datas
                .map(|d| {
                    d.get_candles()
                        .clone()
                        .iter()
                        .map(|v| v.close_time)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            for i in 0..times[0].len() {
                for j in 0..times.len() {
                    for k in 0..times.len() {
                        let difference = (times[j][i] - times[k][i]).abs();
                        let difference = difference.num_milliseconds();
                        assert!(difference <= maximum_variance, "Difference: {}", difference);
                    }
                }
            }
        }
    }
}

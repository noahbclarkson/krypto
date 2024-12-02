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
    /**
    Load the dataset with the given configuration from the Binance API.
    The dataset will contain all the data for the given intervals and symbols.

    ## Arguments
    * `config` - The configuration to use for loading the dataset.

    ## Returns
    The loaded dataset if successful, or a KryptoError if an error occurred.
     */
    #[instrument(skip(config))]
    pub async fn load(config: &KryptoConfig) -> Result<Self, KryptoError> {
        let mut interval_data_map = HashMap::new();
        let market: Market = config.get_binance();

        for interval in &config.intervals {
            let interval = *interval;
            let interval_data = IntervalData::load(&interval, config, &market).await?;
            info!("Loaded data for {}", &interval);
            interval_data_map.insert(interval, interval_data);
        }

        Ok(Self { interval_data_map })
    }

    /**
    Get the shape of the dataset. This will return a tuple containing the number of intervals, the
    number of symbols, and the number of candles for each symbol at each interval.
     */
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

/**
The dataset for a given interval. This contains all the data for all symbols at the given interval.
 */
pub struct IntervalData {
    symbol_data_map: HashMap<String, RawSymbolData>,
    normalized_predictors: Vec<Vec<f64>>,
}

impl IntervalData {
    #[instrument(skip(config, market))]
    async fn load(
        interval: &Interval,
        config: &KryptoConfig,
        market: &Market,
    ) -> Result<Self, KryptoError> {
        let end = Utc::now().timestamp_millis();
        let mut tasks = Vec::new();
        for symbol in &config.symbols {
            let task = RawSymbolData::load(interval, symbol, end, config, market);
            tasks.push(task);
        }
        let result = futures::future::try_join_all(tasks).await?;
        let symbol_data_map: HashMap<String, RawSymbolData> = result
            .into_iter()
            .map(|data| (data.symbol.clone(), data))
            .collect();
        let records = get_records(&symbol_data_map);
        let normalized_predictors = get_normalized_predictors(records);

        Ok(Self {
            symbol_data_map,
            normalized_predictors,
        })
    }

    pub fn len(&self) -> usize {
        self.symbol_data_map.len()
    }

    fn get(&self, symbol: &str) -> Option<&RawSymbolData> {
        self.symbol_data_map.get(symbol)
    }

    fn values(&self) -> impl Iterator<Item = &RawSymbolData> {
        self.symbol_data_map.values()
    }

    pub fn is_empty(&self) -> bool {
        self.symbol_data_map.is_empty()
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

    /**
    Get the dataset for the given symbol. This will return a dataset containing the features, labels,
    and candles for the given symbol. Each row in the features matrix will contain the normalised technical
    indicators for all symbols for the given interval for the last `depth` candles. The labels will contain
    the percentage change (signum) in the closing price for the given symbol for the next candle.

    ## Arguments
    * `settings` - The settings to use for the dataset.

    ## Returns
    The dataset for the given symbol.
     */
    #[instrument(skip(settings, self))]
    pub fn get_symbol_dataset(&self, settings: &AlgorithmSettings) -> SymbolDataset {
        let features = self
            .normalized_predictors
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

        debug!("Features: {}x{} | Labels: {} | Candles: {}", features.len(), features[0].len(), labels.len(), candles.len());

        SymbolDataset::new(features, labels, candles)
    }

    pub fn get_specific_tickers_and_technicals(&self, tickers: &[String], new_technicals: &[String]) -> Self {
        // Create a new symbol_data_map with only the specified tickers
        let mut new_symbol_data_map: HashMap<String, RawSymbolData> = HashMap::new();

        for ticker in tickers {
            if let Some(symbol_data) = self.symbol_data_map.get(ticker) {
                let mut symbol_data = symbol_data.clone();
                symbol_data.recompute_technicals(new_technicals.to_vec());
                new_symbol_data_map.insert(ticker.clone(), symbol_data);
            }
        }

        // Recompute the normalized predictors with the new symbol data
        let records = get_records(&new_symbol_data_map);
        let normalized_predictors = get_normalized_predictors(records);

        Self {
            symbol_data_map: new_symbol_data_map,
            normalized_predictors,
        }
    }
}

fn get_normalized_predictors(records: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    normalize_by_columns(records)
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|v| if v.is_nan() { 0.0 } else { v })
                .collect()
        })
        .collect::<Vec<Vec<f64>>>()
}

fn get_records(map: &HashMap<String, RawSymbolData>) -> Vec<Vec<f64>> {
    let mut records = Vec::new();
    for i in 0..map.values().next().unwrap().len() {
        let mut record = Vec::new();
        for symbol_data in map.values() {
            let technicals = symbol_data.get_technicals();
            record.extend(technicals[i].as_array().to_vec());
        }
        records.push(record);
    }
    records
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

#[derive(Debug, Clone)]
struct RawSymbolData {
    candles: Vec<Candlestick>,
    technicals: Vec<Technicals>,
    labels: Vec<f64>,
    symbol: String,
}

impl RawSymbolData {
    #[instrument(skip(interval, end, config, market))]
    async fn load(
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
            let mut chunk = Self::load_chunk(market, symbol, interval, start, end).await?;
            candles.append(&mut chunk);
        }

        candles.sort_by_key(|c| c.open_time);
        candles.dedup_by_key(|c| c.open_time);

        let technicals = Technicals::get_technicals(&candles, config.technicals.clone());
        let mut labels = vec![0.0];
        for i in 1..candles.len() {
            let percentage_change =
                (candles[i].close - candles[i - 1].close) / candles[i - 1].close;
            labels.push(percentage_change.signum());
        }
        debug!(
            "Loaded {} candles ({} labels | {}x{} technicals) for {}",
            candles.len(),
            labels.len(),
            technicals.len(),
            technicals[0].as_array().len(),
            symbol
        );
        Ok(Self {
            candles,
            technicals,
            labels,
            symbol: symbol.to_string(),
        })
    }

    async fn load_chunk(
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
            .await
            .map_err(|e| KryptoError::BinanceApiError(e.to_string()))?;
        let candlesticks = Candlestick::map_to_candlesticks(summaries)?;
        Ok(candlesticks)
    }

    fn len(&self) -> usize {
        self.candles.len()
    }

    fn get_candles(&self) -> &Vec<Candlestick> {
        &self.candles
    }

    fn get_technicals(&self) -> &Vec<Technicals> {
        &self.technicals
    }

    fn get_labels(&self) -> &Vec<f64> {
        &self.labels
    }

    fn recompute_technicals(&mut self, technical_names: Vec<String>) {
        self.technicals = Technicals::get_technicals(&self.candles, technical_names);
    }
}
use std::collections::HashMap;

use binance::market::Market;
use chrono::Utc;
use tracing::{debug, instrument};

use crate::{
    algorithm::algo::AlgorithmSettings,
    config::KryptoConfig,
    data::{candlestick::Candlestick, interval::Interval, technicals::Technicals},
    error::KryptoError,
};

use super::{
    get_normalized_predictors, get_records,
    symbol_data::{RawSymbolData, SymbolDataset},
};

/**
The dataset for a given interval. This contains all the data for all symbols at the given interval.
 */
pub struct IntervalData {
    symbol_data_map: HashMap<String, RawSymbolData>,
    normalized_predictors: Vec<Vec<f64>>,
}

impl IntervalData {
    #[instrument(skip(config))]
    pub async fn load(interval: &Interval, config: &KryptoConfig) -> Result<Self, KryptoError> {
        let market: Market = config.get_binance();
        let end = Utc::now().timestamp_millis();
        let tasks = config
            .symbols
            .iter()
            .map(|symbol| RawSymbolData::load(interval, symbol, end, config, &market));
        let result = futures::future::try_join_all(tasks).await?;
        let symbol_data_map: HashMap<String, RawSymbolData> = result
            .into_iter()
            .map(|data| (data.symbol().to_string(), data))
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

    pub fn get(&self, symbol: &str) -> Option<&RawSymbolData> {
        self.symbol_data_map.get(symbol)
    }

    pub fn values(&self) -> impl Iterator<Item = &RawSymbolData> {
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

        debug!(
            "Features: {}x{} | Labels: {} | Candles: {}",
            features.len(),
            features[0].len(),
            labels.len(),
            candles.len()
        );

        SymbolDataset::new(features, labels, candles)
    }

    pub fn get_specific_tickers_and_technicals(
        &self,
        tickers: &[String],
        new_technicals: &[String],
    ) -> Self {
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

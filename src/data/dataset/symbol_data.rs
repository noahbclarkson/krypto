use binance::market::Market;
use tracing::{debug, instrument};

use crate::{
    config::KryptoConfig,
    data::{candlestick::Candlestick, interval::Interval, technicals::Technicals},
    error::KryptoError,
    util::date_utils::{date_to_datetime, get_timestamps},
};

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
pub struct RawSymbolData {
    candles: Vec<Candlestick>,
    technicals: Vec<Technicals>,
    labels: Vec<f64>,
    symbol: String,
}

impl RawSymbolData {
    #[instrument(skip(interval, end, config, market))]
    pub async fn load(
        interval: &Interval,
        symbol: &str,
        end: i64,
        config: &KryptoConfig,
        market: &Market,
    ) -> Result<Self, KryptoError> {
        let mut candles = Vec::new();
        let start = date_to_datetime(&config.start_date)?;
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

    pub fn len(&self) -> usize {
        self.candles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candles.is_empty() || self.technicals.is_empty() || self.labels.is_empty()
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

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    pub fn recompute_technicals(&mut self, technical_names: Vec<String>) {
        self.technicals = Technicals::get_technicals(&self.candles, technical_names);
    }
}

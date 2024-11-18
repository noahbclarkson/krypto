use super::{candlestick::Candlestick, interval::Interval, technicals::Technicals};
use crate::{
    config::KryptoConfig,
    error::KryptoError,
    util::date_utils::{date_to_datetime, get_timestamps},
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

    pub fn get(&self, symbol: &str) -> Option<&SymbolData> {
        self.symbol_data_map.get(symbol)
    }

    pub fn get_map(&self) -> &HashMap<String, SymbolData> {
        &self.symbol_data_map
    }

    pub fn values(&self) -> impl Iterator<Item = &SymbolData> {
        self.symbol_data_map.values()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.symbol_data_map.keys()
    }

    pub fn is_empty(&self) -> bool {
        self.symbol_data_map.is_empty()
    }

    // Get all the technicals for all the symbols. Each row contains all the tecnhicals for each of
    // the symbols at a given time.
    pub fn get_records(&self) -> Vec<Vec<f64>> {
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
}

pub struct SymbolData {
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

    pub fn is_empty(&self) -> bool {
        self.candles.is_empty()
    }
}

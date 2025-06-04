use std::collections::HashMap;
use tracing::{error, info, instrument};

use crate::{config::KryptoConfig, data::interval::Interval, error::KryptoError};

use super::interval_data::IntervalData;

#[derive(Debug)] // Added Debug derive
pub struct Dataset {
    interval_data_map: HashMap<Interval, IntervalData>,
}

impl Dataset {
    /**
    Load the dataset with the given configuration.
    This involves fetching data (potentially from cache or API) for all configured symbols and intervals.

    ## Arguments
    * `config` - The configuration specifying symbols, intervals, date range, cache settings, etc.

    ## Returns
    The loaded dataset if successful, or a KryptoError if an error occurred.
     */
    #[instrument(skip(config))]
    pub async fn load(config: &KryptoConfig) -> Result<Self, KryptoError> {
        info!("Loading overall dataset...");
        let mut interval_data_map = HashMap::new();

        // Could potentially parallelize loading across intervals if beneficial
        for interval in &config.intervals {
            let interval = *interval; // Deref copy
            info!("Loading data for interval: {}", interval);
            match IntervalData::load(interval, config).await {
                Ok(interval_data) => {
                    info!("Successfully loaded data for interval: {}", interval);
                    interval_data_map.insert(interval, interval_data);
                }
                Err(e) => {
                    // Decide whether to error out completely or just log and skip the interval
                    error!(
                        "Failed to load data for interval {}: {}. Skipping interval.",
                        interval, e
                    );
                    // return Err(e); // Uncomment to fail fast
                }
            }
        }

        if interval_data_map.is_empty() {
            return Err(KryptoError::InsufficientData {
                got: 0,
                required: 1,
                context: "Failed to load data for any configured interval.".to_string(),
            });
        }

        info!(
            "Overall dataset loaded successfully with {} intervals.",
            interval_data_map.len()
        );
        Ok(Self { interval_data_map })
    }

    /**
    Get the shape of the dataset.
    Returns a map where keys are intervals and values are tuples of (num_symbols, num_candles).
    Note: Assumes num_candles is consistent across symbols within an interval after loading.
     */
    pub fn shape(&self) -> HashMap<Interval, (usize, usize)> {
        self.interval_data_map
            .iter()
            .map(|(interval, interval_data)| {
                let num_symbols = interval_data.len();
                // Get candle length from the first symbol's data (assuming consistency)
                let num_candles = interval_data
                    .values()
                    .next()
                    .map(|sd| sd.len())
                    .unwrap_or(0);
                (*interval, (num_symbols, num_candles))
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.interval_data_map.len()
    }

    pub fn get(&self, interval: &Interval) -> Option<&IntervalData> {
        self.interval_data_map.get(interval)
    }

    pub fn get_mut(&mut self, interval: &Interval) -> Option<&mut IntervalData> {
        self.interval_data_map.get_mut(interval)
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

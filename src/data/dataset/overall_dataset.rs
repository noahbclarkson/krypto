use std::collections::HashMap;
use tracing::{info, instrument};

use crate::{config::KryptoConfig, data::interval::Interval, error::KryptoError};

use super::interval_data::IntervalData;

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

        for interval in &config.intervals {
            let interval = *interval;
            let interval_data = IntervalData::load(&interval, config).await?;
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

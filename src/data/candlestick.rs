use binance::rest_model::{KlineSummaries, KlineSummary};
use chrono::{DateTime, TimeZone, Utc};
use derive_builder::Builder;

use crate::error::{KryptoError, When};

#[derive(Debug, Clone, Builder)]
pub struct Candlestick {
    pub open_time: DateTime<Utc>,
    pub close_time: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl Candlestick {
    // Create a new Candlestick from a KlineSummary
    //
    // # Arguments
    //
    // * `summary` - The KlineSummary to convert to a Candlestick.
    //
    // # Returns
    //
    // A Result containing the Candlestick if successful, or a KryptoError if an error occurred.
    pub fn from_summary(summary: KlineSummary) -> Result<Self, KryptoError> {
        let open_time = Utc.timestamp_millis_opt(summary.open_time).single().ok_or(
            KryptoError::InvalidCandlestickDateTime {
                when: When::Open,
                timestamp: summary.open_time,
            },
        )?;
        let close_time = Utc
            .timestamp_millis_opt(summary.close_time)
            .single()
            .ok_or(KryptoError::InvalidCandlestickDateTime {
                when: When::Close,
                timestamp: summary.close_time,
            })?;
        if let Some(std::cmp::Ordering::Greater) = open_time.partial_cmp(&close_time) {
            return Err(KryptoError::OpenTimeGreaterThanCloseTime {
                open_time: summary.open_time,
                close_time: summary.close_time,
            });
        }
        Ok(Self {
            open_time,
            close_time,
            open: summary.open,
            high: summary.high,
            low: summary.low,
            close: summary.close,
            volume: summary.volume,
        })
    }

    // Create a vector of Candlesticks from a KlineSummaries
    //
    // # Arguments
    //
    // * `summaries` - The KlineSummaries to convert to Candlesticks.
    //
    // # Returns
    //
    // A Result containing the vector of Candlesticks if successful, or a KryptoError if an error occurred.
    pub fn map_to_candlesticks(summaries: KlineSummaries) -> Result<Vec<Self>, KryptoError> {
        match summaries {
            KlineSummaries::AllKlineSummaries(summaries) => summaries
                .into_iter()
                .map(Candlestick::from_summary)
                .collect(),
        }
    }
}

impl ta::Open for Candlestick {
    fn open(&self) -> f64 {
        self.open
    }
}

impl ta::High for Candlestick {
    fn high(&self) -> f64 {
        self.high
    }
}

impl ta::Low for Candlestick {
    fn low(&self) -> f64 {
        self.low
    }
}

impl ta::Close for Candlestick {
    fn close(&self) -> f64 {
        self.close
    }
}

impl ta::Volume for Candlestick {
    fn volume(&self) -> f64 {
        self.volume
    }
}

impl PartialEq for Candlestick {
    fn eq(&self, other: &Self) -> bool {
        self.open_time == other.open_time
            && self.close_time == other.close_time
            && self.open == other.open
            && self.high == other.high
            && self.low == other.low
            && self.close == other.close
            && self.volume == other.volume
    }
}

impl PartialOrd for Candlestick {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.open_time.partial_cmp(&other.open_time)
    }
}
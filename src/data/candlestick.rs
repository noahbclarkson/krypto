use binance::rest_model::{KlineSummaries, KlineSummary};
use chrono::{DateTime, TimeZone, Utc};
use derive_builder::Builder;
use serde::{Deserialize, Serialize}; // Added for caching

use crate::{
    data::interval::Interval, // Assuming interval might be needed for context
    error::{KryptoError, When},
};

#[derive(Debug, Clone, Builder, Serialize, Deserialize, PartialEq)] // Added Serialize, Deserialize, PartialEq
pub struct Candlestick {
    pub open_time: DateTime<Utc>,
    pub close_time: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    // Optional: Add symbol and interval if needed for context in errors/cache keys
    // pub symbol: String,
    // pub interval: Interval,
}

impl Candlestick {
    /// Create a new Candlestick from a KlineSummary
    ///
    /// # Arguments
    ///
    /// * `summary` - The KlineSummary to convert.
    /// * `symbol` - The symbol this candlestick belongs to (for error context).
    /// * `interval` - The interval this candlestick belongs to (for error context).
    ///
    /// # Returns
    ///
    /// A Result containing the Candlestick if successful, or a KryptoError if an error occurred.
    pub fn from_summary(
        summary: KlineSummary,
        symbol: &str,
        interval: &Interval,
    ) -> Result<Self, KryptoError> {
        let open_time = Utc.timestamp_millis_opt(summary.open_time).single().ok_or(
            KryptoError::InvalidCandlestickDateTime {
                when: When::Open,
                timestamp: summary.open_time,
                symbol: symbol.to_string(),
                interval: interval.to_string(),
            },
        )?;
        let close_time = Utc
            .timestamp_millis_opt(summary.close_time)
            .single()
            .ok_or(KryptoError::InvalidCandlestickDateTime {
                when: When::Close,
                timestamp: summary.close_time,
                symbol: symbol.to_string(),
                interval: interval.to_string(),
            })?;

        // Validate times
        if open_time >= close_time {
            return Err(KryptoError::OpenTimeGreaterThanCloseTime {
                open_time: summary.open_time,
                close_time: summary.close_time,
                symbol: symbol.to_string(),
                interval: interval.to_string(),
            });
        }

        // Validate prices (basic check)
        if summary.low > summary.high
            || summary.low > summary.open
            || summary.low > summary.close
            || summary.high < summary.open
            || summary.high < summary.close
        {
            // Potentially log a warning here instead of erroring? Depends on strictness needed.
            // tracing::warn!(symbol, %interval, open_time = %open_time, "Inconsistent HLOC values in KlineSummary: H={}, L={}, O={}, C={}",
            //     summary.high, summary.low, summary.open, summary.close);
        }

        Ok(Self {
            open_time,
            close_time,
            open: summary.open,
            high: summary.high,
            low: summary.low,
            close: summary.close,
            volume: summary.volume,
            // symbol: symbol.to_string(), // Uncomment if added to struct
            // interval: *interval,       // Uncomment if added to struct
        })
    }

    /// Create a vector of Candlesticks from KlineSummaries
    ///
    /// # Arguments
    ///
    /// * `summaries` - The KlineSummaries to convert.
    /// * `symbol` - The symbol these candlesticks belong to.
    /// * `interval` - The interval these candlesticks belong to.
    ///
    /// # Returns
    ///
    /// A Result containing the vector of Candlesticks if successful, or a KryptoError if an error occurred.
    pub fn map_to_candlesticks(
        summaries: KlineSummaries,
        symbol: &str,
        interval: &Interval,
    ) -> Result<Vec<Self>, KryptoError> {
        match summaries {
            KlineSummaries::AllKlineSummaries(summaries) => summaries
                .into_iter()
                .map(|summary| Candlestick::from_summary(summary, symbol, interval))
                .collect(), // Collect will propagate the first error
        }
    }
}

// --- TA Crate Implementations ---
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

// --- Ordering ---
impl PartialOrd for Candlestick {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Candlestick {} // Required for Ord if PartialEq is derived

impl Ord for Candlestick {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.open_time.cmp(&other.open_time)
    }
}

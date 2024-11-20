use binance::model::{KlineSummaries, KlineSummary};
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
            open: summary.open.parse().map_err(|_| KryptoError::ParseError {
                value_name: "open".to_string(),
                timestamp: summary.open_time,
            })?,
            high: summary.high.parse().map_err(|_| KryptoError::ParseError {
                value_name: "high".to_string(),
                timestamp: summary.open_time,
            })?,
            low: summary.low.parse().map_err(|_| KryptoError::ParseError {
                value_name: "low".to_string(),
                timestamp: summary.open_time,
            })?,
            close: summary.close.parse().map_err(|_| KryptoError::ParseError {
                value_name: "close".to_string(),
                timestamp: summary.open_time,
            })?,
            volume: summary
                .volume
                .parse()
                .map_err(|_| KryptoError::ParseError {
                    value_name: "volume".to_string(),
                    timestamp: summary.open_time,
                })?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KryptoError;
    use chrono::TimeZone;

    #[test]
    fn test_candlestick_from_summary() {
        let summary = KlineSummary {
            open_time: 1618185600000,
            close_time: 1618185659999,
            open: "0.00000000".to_string(),
            high: "0.00000000".to_string(),
            low: "0.00000000".to_string(),
            close: "0.00000000".to_string(),
            volume: "0.00000000".to_string(),
            quote_asset_volume: "0.00000000".to_string(),
            number_of_trades: 0,
            taker_buy_base_asset_volume: "0.00000000".to_string(),
            taker_buy_quote_asset_volume: "0.00000000".to_string(),
        };

        let candlestick = Candlestick::from_summary(summary).unwrap();
        assert_eq!(
            candlestick.open_time,
            Utc.timestamp_millis_opt(1618185600000).single().unwrap()
        );
        assert_eq!(
            candlestick.close_time,
            Utc.timestamp_millis_opt(1618185659999).single().unwrap()
        );
        assert_eq!(candlestick.open, 0.0);
        assert_eq!(candlestick.high, 0.0);
        assert_eq!(candlestick.low, 0.0);
        assert_eq!(candlestick.close, 0.0);
        assert_eq!(candlestick.volume, 0.0);
    }

    #[test]
    fn test_candlestick_from_summary_invalid_open_time() {
        let summary = KlineSummary {
            open_time: 16181856599999998,
            close_time: 16181856599999999,
            open: "0.00000000".to_string(),
            high: "0.00000000".to_string(),
            low: "0.00000000".to_string(),
            close: "0.00000000".to_string(),
            volume: "0.00000000".to_string(),
            quote_asset_volume: "0.00000000".to_string(),
            number_of_trades: 0,
            taker_buy_base_asset_volume: "0.00000000".to_string(),
            taker_buy_quote_asset_volume: "0.00000000".to_string(),
        };

        let result = Candlestick::from_summary(summary);
        assert!(matches!(
            result,
            Err(KryptoError::InvalidCandlestickDateTime {
                when: When::Open,
                timestamp: 16181856599999998
            })
        ));
    }

    #[test]
    fn test_candlestick_from_summary_invalid_close_time() {
        let summary = KlineSummary {
            open_time: 1618185600000,
            close_time: 16181856599999999,
            open: "0.00000000".to_string(),
            high: "0.00000000".to_string(),
            low: "0.00000000".to_string(),
            close: "0.00000000".to_string(),
            volume: "0.00000000".to_string(),
            quote_asset_volume: "0.00000000".to_string(),
            number_of_trades: 0,
            taker_buy_base_asset_volume: "0.00000000".to_string(),
            taker_buy_quote_asset_volume: "0.00000000".to_string(),
        };

        let result = Candlestick::from_summary(summary);
        assert!(matches!(
            result,
            Err(KryptoError::InvalidCandlestickDateTime {
                when: When::Close,
                timestamp: 16181856599999999
            })
        ));
    }

    #[test]
    fn test_candlestick_from_summary_open_time_greater_than_close_time() {
        let summary = KlineSummary {
            open_time: 1618185659999,
            close_time: 1618185600000,
            open: "0.00000000".to_string(),
            high: "0.00000000".to_string(),
            low: "0.00000000".to_string(),
            close: "0.00000000".to_string(),
            volume: "0.00000000".to_string(),
            quote_asset_volume: "0.00000000".to_string(),
            number_of_trades: 0,
            taker_buy_base_asset_volume: "0.00000000".to_string(),
            taker_buy_quote_asset_volume: "0.00000000".to_string(),
        };

        let result = Candlestick::from_summary(summary);
        assert!(matches!(
            result,
            Err(KryptoError::OpenTimeGreaterThanCloseTime {
                open_time: 1618185659999,
                close_time: 1618185600000
            })
        ));
    }
}



use std::fmt;

use binance::model::{Kline, KlineSummaries, KlineSummary};
use chrono::{DateTime, TimeZone as _, Utc};
use serde::{ser::SerializeStruct as _, Serialize, Serializer};

use crate::{technicals::Technicals, KryptoError};

#[derive(Debug, Clone)]
pub struct Candlestick {
    pub open_time: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub close_time: DateTime<Utc>,
    pub quote_asset_volume: f64,
    pub number_of_trades: i64,
    pub taker_buy_base_asset_volume: f64,
    pub taker_buy_quote_asset_volume: f64,
    pub technicals: Technicals,
    pub percentage_change: Option<f64>,
    pub features: Option<Vec<f64>>,
}

impl Candlestick {
    pub fn from_summary(summary: KlineSummary) -> Result<Self, KryptoError> {
        Ok(Candlestick {
            open_time: Utc
                .timestamp_millis_opt(summary.open_time)
                .single()
                .ok_or(KryptoError::InvalidDateTime)?,
            open: summary.open.parse().unwrap(),
            high: summary.high.parse().unwrap(),
            low: summary.low.parse().unwrap(),
            close: summary.close.parse().unwrap(),
            volume: summary.volume.parse().unwrap(),
            close_time: Utc
                .timestamp_millis_opt(summary.close_time)
                .single()
                .ok_or(KryptoError::InvalidDateTime)?,
            quote_asset_volume: summary.quote_asset_volume.parse().unwrap(),
            number_of_trades: summary.number_of_trades,
            taker_buy_base_asset_volume: summary.taker_buy_base_asset_volume.parse().unwrap(),
            taker_buy_quote_asset_volume: summary.taker_buy_quote_asset_volume.parse().unwrap(),
            technicals: Technicals::default(),
            percentage_change: None,
            features: None,
        })
    }

    pub fn from_kline(kline: Kline) -> Result<Self, KryptoError> {
        Ok(Candlestick {
            open_time: Utc
                .timestamp_millis_opt(kline.open_time)
                .single()
                .ok_or(KryptoError::InvalidDateTime)?,
            open: kline.open.parse().unwrap(),
            high: kline.high.parse().unwrap(),
            low: kline.low.parse().unwrap(),
            close: kline.close.parse().unwrap(),
            volume: kline.volume.parse().unwrap(),
            close_time: Utc
                .timestamp_millis_opt(kline.close_time)
                .single()
                .ok_or(KryptoError::InvalidDateTime)?,
            quote_asset_volume: kline.quote_asset_volume.parse().unwrap(),
            number_of_trades: kline.number_of_trades,
            taker_buy_base_asset_volume: kline.taker_buy_base_asset_volume.parse().unwrap(),
            taker_buy_quote_asset_volume: kline.taker_buy_quote_asset_volume.parse().unwrap(),
            technicals: Technicals::default(),
            percentage_change: None,
            features: None,
        })
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

pub fn map_to_candlesticks(summaries: KlineSummaries) -> Result<Vec<Candlestick>, KryptoError> {
    match summaries {
        KlineSummaries::AllKlineSummaries(summaries) => summaries
            .into_iter()
            .map(Candlestick::from_summary)
            .collect(),
    }
}

impl Serialize for Candlestick {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let t_headers = Technicals::get_headers();
        let mut state = serializer.serialize_struct("Candlestick", 11 + t_headers.len())?;
        state.serialize_field("open_time", &self.open_time.timestamp_millis())?;
        state.serialize_field("open", &self.open)?;
        state.serialize_field("high", &self.high)?;
        state.serialize_field("low", &self.low)?;
        state.serialize_field("close", &self.close)?;
        state.serialize_field("volume", &self.volume)?;
        state.serialize_field("close_time", &self.close_time.timestamp_millis())?;
        state.serialize_field("quote_asset_volume", &self.quote_asset_volume)?;
        state.serialize_field("number_of_trades", &self.number_of_trades)?;
        state.serialize_field(
            "taker_buy_base_asset_volume",
            &self.taker_buy_base_asset_volume,
        )?;
        state.serialize_field(
            "taker_buy_quote_asset_volume",
            &self.taker_buy_quote_asset_volume,
        )?;
        state.serialize_field("percentage_change", &self.percentage_change)?;

        for (key, value) in t_headers.iter().zip(self.technicals.get_array().iter()) {
            state.serialize_field(key, value)?;
        }

        state.end()
    }
}

impl fmt::Display for Candlestick {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Open Time: {}, Open: {}, High: {}, Low: {}, Close: {}, Volume: {}, Close Time: {}, Quote Asset Volume: {}, Number of Trades: {}, Taker Buy Base Asset Volume: {}, Taker Buy Quote Asset Volume: {}",
            self.open_time,
            self.open,
            self.high,
            self.low,
            self.close,
            self.volume,
            self.close_time,
            self.quote_asset_volume,
            self.number_of_trades,
            self.taker_buy_base_asset_volume,
            self.taker_buy_quote_asset_volume
        )
    }
}
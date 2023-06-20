use std::cmp::Ordering;

use binance::rest_model::KlineSummary;
use getset::Getters;
use serde::{ser::SerializeStruct, Deserialize, Deserializer, Serialize};
use strum_macros::EnumIter;

#[derive(Debug, Getters, Clone)]
#[getset(get = "pub")]
pub struct Candlestick {
    candle: Candle,
    technicals: Vec<f64>,
}

impl Candlestick {
    pub fn new_from_summary(summary: KlineSummary) -> Self {
        let candle = Candle {
            open: summary.open,
            high: summary.high,
            low: summary.low,
            close: summary.close,
            volume: summary.volume,
            close_time: summary.close_time,
        };
        let technicals = vec![];
        Self { candle, technicals }
    }
}

impl Serialize for Candlestick {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("Candlestick", 6 + &self.technicals.len())?;
        state.serialize_field("open", &self.candle.open)?;
        state.serialize_field("high", &self.candle.high)?;
        state.serialize_field("low", &self.candle.low)?;
        state.serialize_field("close", &self.candle.close)?;
        state.serialize_field("volume", &self.candle.volume)?;
        state.serialize_field("close_time", &self.candle.close_time)?;
        let headers = vec![
            "Percentage Change",
            "Candlestick Ratio",
            "Stochastic Oscillator",
            "Relative Strength Index",
            "Commodity Channel Index",
            "Volume Change",
            "Standard Deviation",
        ];
        for (index, technical) in self.technicals.iter().enumerate() {
            state.serialize_field(headers[index], technical)?;
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for Candlestick {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct CandlestickData {
            open: f64,
            high: f64,
            low: f64,
            close: f64,
            volume: f64,
            close_time: i64,
            #[serde(rename = "Percentage Change")]
            percentage_change: Option<f64>,
            #[serde(rename = "Candlestick Ratio")]
            candlestick_ratio: Option<f64>,
            #[serde(rename = "Stochastic Oscillator")]
            stochastic_oscillator: Option<f64>,
            #[serde(rename = "Relative Strength Index")]
            relative_strength_index: Option<f64>,
            #[serde(rename = "Commodity Channel Index")]
            commodity_channel_index: Option<f64>,
            #[serde(rename = "Volume Change")]
            volume_change: Option<f64>,
            #[serde(rename = "Standard Deviation")]
            standard_deviation: Option<f64>,
        }

        let data = CandlestickData::deserialize(deserializer)?;

        let technicals = vec![
            data.percentage_change.unwrap_or_default(),
            data.candlestick_ratio.unwrap_or_default(),
            data.stochastic_oscillator.unwrap_or_default(),
            data.relative_strength_index.unwrap_or_default(),
            data.commodity_channel_index.unwrap_or_default(),
            data.volume_change.unwrap_or_default(),
            data.standard_deviation.unwrap_or_default(),
        ];

        Ok(Candlestick {
            candle: Candle {
                open: data.open,
                high: data.high,
                low: data.low,
                close: data.close,
                volume: data.volume,
                close_time: data.close_time,
            },
            technicals,
        })
    }
}

impl PartialEq for Candlestick {
    fn eq(&self, other: &Self) -> bool {
        self.candle.close_time == other.candle.close_time
    }
}

impl Eq for Candlestick {}

impl PartialOrd for Candlestick {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.candle.close_time.partial_cmp(&other.candle.close_time)
    }
}

impl Ord for Candlestick {
    fn cmp(&self, other: &Self) -> Ordering {
        self.candle.close_time.cmp(&other.candle.close_time)
    }
}

#[derive(Debug, Getters, Clone)]
#[getset(get = "pub")]
pub struct Candle {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    close_time: i64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, EnumIter)]
pub enum TechnicalType {
    PercentageChange,
    CandlestickRatio,
    StochasticOscillator,
    RelativeStrengthIndex,
    CommodityChannelIndex,
    VolumeChange,
    StandardDeviation,
}

#[cfg(test)]
pub mod tests {

    use super::*;

    #[test]
    fn test_candlestick_ordering() {
        // Create sample candlestick data
        let candlestick1 = Candlestick {
            candle: Candle {
                open: 10.0,
                high: 20.0,
                low: 5.0,
                close: 15.0,
                volume: 1000.0,
                close_time: 1624137600000,
            },
            technicals: vec![],
        };

        let candlestick2 = Candlestick {
            candle: Candle {
                open: 12.0,
                high: 25.0,
                low: 8.0,
                close: 22.0,
                volume: 1500.0,
                close_time: 1624138200000,
            },
            technicals: vec![],
        };

        // Check the ordering of candlesticks
        assert!(candlestick1 < candlestick2);
        assert!(candlestick1 <= candlestick2);
        assert!(candlestick2 > candlestick1);
        assert!(candlestick2 >= candlestick1);
    }
}

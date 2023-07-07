use binance::rest_model::KlineSummary;
use getset::{Getters, MutGetters, Setters};
use serde::{Deserialize, Serialize};
use ta::{DataItem, errors::TaError};

pub const TECHNICAL_COUNT: usize = 6;

#[derive(Debug, Getters, MutGetters, Setters)]
#[getset(get = "pub")]
pub struct Candlestick {
    open: f32,
    close: f32,
    high: f32,
    low: f32,
    volume: f32,
    #[getset(set = "pub")]
    p_change: f32,
    close_time: i64,
    #[getset(get = "pub", get_mut = "pub")]
    technicals: Box<[f32; TECHNICAL_COUNT]>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum TechnicalType {
    PercentageChange,
    CandlestickRatio,
    StochasticOscillator,
    RelativeStrengthIndex,
    CommodityChannelIndex,
    VolumeChange,
}

impl Candlestick {
    #[inline]
    pub fn new_from_summary(summary: KlineSummary) -> Self {
        Self {
            open: summary.open as f32,
            close: summary.close as f32,
            high: summary.high as f32,
            low: summary.low as f32,
            volume: summary.volume as f32,
            p_change: 0.0,
            close_time: summary.close_time,
            technicals: Default::default(),
        }
    }

    #[inline]
    pub fn to_data_item(&self) -> Result<DataItem, TaError> {
        DataItem::builder()
            .open(self.open as f64)
            .high(self.high as f64)
            .low(self.low as f64)
            .close(self.close as f64)
            .volume(self.volume as f64)
            .build()
    }
}

impl PartialOrd for Candlestick {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.close_time.partial_cmp(&other.close_time)
    }
}

impl Ord for Candlestick {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.close_time.cmp(&other.close_time)
    }
}

impl PartialEq for Candlestick {
    fn eq(&self, other: &Self) -> bool {
        self.close_time == other.close_time
    }
}

impl Eq for Candlestick {}
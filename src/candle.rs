use binance::rest_model::KlineSummary;
use getset::{Getters, MutGetters, Setters};
use ta::{errors::TaError, DataItem};

pub const TECHNICAL_COUNT: usize = 10;

#[derive(Debug, Clone, Getters, Setters, MutGetters)]
#[getset(get = "pub")]
pub struct Candle {
    open: f64,
    close: f64,
    high: f64,
    low: f64,
    volume: f64,
    #[getset(set = "pub")]
    percentage_change: f64,
    close_time: i64,
    variance_score: u8,
    #[getset(get_mut = "pub")]
    technicals: Box<[f64; TECHNICAL_COUNT]>,
}

#[derive(Debug)]
pub enum TechnicalType {
    PercentageChange,
    CandlestickRatio,
    StochasticOscillator,
    RelativeStrengthIndex,
    CommodityChannelIndex,
    MoneyFlowIndex,
    PPOscillator,
    EfficiencyRatio,
    VolumeEma,
    PCEMA,
}

impl TechnicalType {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Self::PercentageChange,
            1 => Self::CandlestickRatio,
            2 => Self::StochasticOscillator,
            3 => Self::RelativeStrengthIndex,
            4 => Self::CommodityChannelIndex,
            5 => Self::MoneyFlowIndex,
            6 => Self::PPOscillator,
            7 => Self::EfficiencyRatio,
            8 => Self::VolumeEma,
            9 => Self::PCEMA,
            _ => panic!("Invalid index for TechnicalType"),
        }
    }

    pub fn get_string(&self) -> String {
        match self {
            Self::PercentageChange => "Percentage Change",
            Self::CandlestickRatio => "Candlestick Ratio",
            Self::StochasticOscillator => "Stochastic Oscillator",
            Self::RelativeStrengthIndex => "Relative Strength Index",
            Self::CommodityChannelIndex => "Commodity Channel Index",
            Self::MoneyFlowIndex => "Money Flow Index",
            Self::PPOscillator => "Percentage Price Oscillator",
            Self::EfficiencyRatio => "Efficiency Ratio",
            Self::VolumeEma => "Volume EMA",
            Self::PCEMA => "Percentage Change EMA",
        }
        .to_string()
    }
}

impl Candle {
    #[inline]
    pub fn new_from_summary(summary: KlineSummary) -> Self {
        Self {
            open: summary.open,
            close: summary.close,
            high: summary.high,
            low: summary.low,
            volume: summary.volume,
            variance_score: variance_score(&summary),
            percentage_change: 0.0,
            close_time: summary.close_time,
            technicals: Default::default(),
        }
    }

    #[inline]
    pub fn to_data_item(&self) -> Result<DataItem, TaError> {
        DataItem::builder()
            .open(self.open)
            .high(self.high)
            .low(self.low)
            .close(self.close)
            .volume(self.volume)
            .build()
    }

    #[inline]
    pub fn serialize_to_csv_row(&self) -> String {
        let date = chrono::NaiveDateTime::from_timestamp_opt(self.close_time / 1000, 0).unwrap();
        let formatted_date = date.format("%Y-%m-%d %H:%M:%S").to_string();
        let technicals = self
            .technicals
            .iter()
            .map(|t| format!("{:.2}", t))
            .collect::<Vec<String>>()
            .join(",");
        format!(
            "{},{},{},{},{},{},{},{}",
            self.open,
            self.high,
            self.low,
            self.close,
            self.volume,
            self.percentage_change,
            formatted_date,
            technicals
        )
    }

    #[inline]
    pub fn find_nan(&self) -> Result<(), TaError> {
        self.to_data_item()?;
        Ok(())
    }
}

impl std::fmt::Display for Candle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let technicals = self
            .technicals
            .iter()
            .map(|t| format!("{:.2}", t))
            .collect::<Vec<String>>()
            .join(", ");
        let date = chrono::NaiveDateTime::from_timestamp_opt(self.close_time / 1000, 0).unwrap();
        let formatted_date = date.format("%Y-%m-%d %H:%M:%S").to_string();
        let percentage_change = format!("{:.2}%", self.percentage_change);
        write!(
            f,
            "(Open: {}, Close: {}, High: {}, Low: {}, Volume: {}, Percentage-Change: {} Date: {} | Technicals: [{}])",
            self.open, self.close, self.high, self.low, self.volume, percentage_change, formatted_date, technicals
        )
    }
}

pub fn close_time_to_date(close_time: i64) -> chrono::NaiveDateTime {
    chrono::NaiveDateTime::from_timestamp_opt(close_time / 1000, 0).unwrap()
}

fn variance_score(summary: &KlineSummary) -> u8 {
    let open = summary.open;
    let close = summary.close;
    let high = summary.high;
    let low = summary.low;
    let mut score = 0;
    if open != close {
        score += 1;
    }
    if high != open && high != close {
        score += 1;
    }
    if low != open && low != close {
        score += 1;
    }
    score
}

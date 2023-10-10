use binance::{api::Binance, market::Market, rest_model::KlineSummary};
use derive_builder::Builder;
use errors::DataError;
use getset::Getters;
use historical_data_request::HistoricalDataRequest;
use r_matrix::RMatrixId;

pub mod errors;
mod historical_data_request;
mod math;
mod technical_calulator;
pub mod test;

const CLOSE_TIME_VARIANCE: i64 = 15000;

#[derive(Debug, Getters, Builder, Clone)]
pub struct HistoricalDataConfig {
    periods: usize,
    interval: Interval,
    tickers: Vec<String>,
    api_key: Option<String>,
    api_secret: Option<String>,
}

impl HistoricalDataConfig {
    pub fn interval_minutes(&self) -> usize {
        self.interval.to_minutes()
    }

    pub fn interval_string(&self) -> &str {
        self.interval.to_string()
    }

    pub fn get_binance<T: Binance>(&self) -> T {
        T::new(self.api_key.clone(), self.api_secret.clone())
    }
}

impl Default for HistoricalDataConfig {
    fn default() -> Self {
        Self {
            periods: 100,
            interval: Interval::OneMinute,
            tickers: Vec::new(),
            api_key: None,
            api_secret: None,
        }
    }
}

#[derive(Debug, Getters)]
pub struct HistoricalData {
    data: Vec<TickerData>,
    config: HistoricalDataConfig,
}

impl HistoricalData {
    pub fn new(config: HistoricalDataConfig) -> Self {
        Self {
            data: Vec::new(),
            config,
        }
    }

    pub async fn load(&mut self) -> Result<(), DataError> {
        let request = HistoricalDataRequest::new(&self.config);
        let tasks = self.config.tickers.iter().map(|ticker| request.run(ticker));
        let tickers = futures::future::join_all(tasks).await;
        self.data = tickers.into_iter().collect::<Result<Vec<_>, _>>()?;
        self.validate()?;
        Ok(())
    }

    fn validate(&mut self) -> Result<(), DataError> {
        for t_data in self.data.iter() {
            let actual = t_data.klines.len();
            let desired = self.config.periods;
            let symbol = t_data.ticker.to_string();
            if actual < desired {
                return Err(DataError::NotEnoughData {
                    symbol,
                    desired,
                    actual,
                });
            } else if actual > desired {
                return Err(DataError::TooMuchData {
                    symbol,
                    desired,
                    actual,
                });
            }
        }
        let close_times = self.data[0].close_times().collect::<Vec<_>>();
        for t_data in self.data.iter().skip(1) {
            let close_times_2 = t_data.close_times();
            for (time_1, time_2) in close_times.iter().zip(close_times_2) {
                if (time_1 - time_2).abs() > CLOSE_TIME_VARIANCE {
                    return Err(DataError::MismatchedCloseTimes {
                        symbol: t_data.ticker.to_string(),
                        time_1: *time_1,
                        time_2,
                    });
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Getters)]
pub struct TickerData {
    klines: Box<[KlineSummary]>,
    technicals: Box<[Box<[f64]>]>,
    ticker: Box<str>,
}

impl TickerData {
    pub(crate) fn new(ticker: String, klines: Vec<KlineSummary>) -> Self {
        Self {
            ticker: ticker.into_boxed_str(),
            klines: klines.into_boxed_slice(),
            technicals: Box::new([]),
        }
    }

    fn close_times(&self) -> impl Iterator<Item = i64> + '_ {
        self.klines.iter().map(|kline| kline.close_time)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Interval {
    OneMinute,
    ThreeMinutes,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
    TwoHours,
    FourHours,
    SixHours,
    EightHours,
    TwelveHours,
    OneDay,
    ThreeDays,
    OneWeek,
    OneMonth,
}

impl Interval {
    pub fn to_string(&self) -> &str {
        match self {
            Interval::OneMinute => "1m",
            Interval::ThreeMinutes => "3m",
            Interval::FiveMinutes => "5m",
            Interval::FifteenMinutes => "15m",
            Interval::ThirtyMinutes => "30m",
            Interval::OneHour => "1h",
            Interval::TwoHours => "2h",
            Interval::FourHours => "4h",
            Interval::SixHours => "6h",
            Interval::EightHours => "8h",
            Interval::TwelveHours => "12h",
            Interval::OneDay => "1d",
            Interval::ThreeDays => "3d",
            Interval::OneWeek => "1w",
            Interval::OneMonth => "1M",
        }
    }

    pub fn to_minutes(&self) -> usize {
        match self {
            Interval::OneMinute => 1,
            Interval::ThreeMinutes => 3,
            Interval::FiveMinutes => 5,
            Interval::FifteenMinutes => 15,
            Interval::ThirtyMinutes => 30,
            Interval::OneHour => 60,
            Interval::TwoHours => 120,
            Interval::FourHours => 240,
            Interval::SixHours => 360,
            Interval::EightHours => 480,
            Interval::TwelveHours => 720,
            Interval::OneDay => 1440,
            Interval::ThreeDays => 4320,
            Interval::OneWeek => 10080,
            Interval::OneMonth => 43200,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct BinanceDataId {
    id: BinanceDataType,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BinanceDataType {
    PercentageChange,
    PercentageChangeReal,
}

impl BinanceDataId {
    pub fn new(id: BinanceDataType) -> Self {
        Self { id }
    }
}

impl RMatrixId for BinanceDataId {
    fn get_id(&self) -> &str {
        match self.id {
            BinanceDataType::PercentageChange => "Percentage Change",
            BinanceDataType::PercentageChangeReal => "Real Percentage Change",
        }
    }

    fn is_target(&self) -> bool {
        match self.id {
            BinanceDataType::PercentageChangeReal => true,
            _ => false,
        }
    }
}

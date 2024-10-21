// interval.rs

use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Represents various time intervals.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
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
    /// Returns the string representation of the interval.
    pub fn as_str(&self) -> &'static str {
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

    /// Returns the total minutes for the interval.
    pub fn to_minutes(&self) -> i64 {
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

    /// Creates an `Interval` from the number of minutes.
    ///
    /// # Arguments
    ///
    /// * `minutes` - The number of minutes to convert.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Interval` on success or a `ParseIntervalError` on failure.
    pub fn from_minutes(minutes: usize) -> Result<Self, ParseIntervalError> {
        match minutes {
            1 => Ok(Interval::OneMinute),
            3 => Ok(Interval::ThreeMinutes),
            5 => Ok(Interval::FiveMinutes),
            15 => Ok(Interval::FifteenMinutes),
            30 => Ok(Interval::ThirtyMinutes),
            60 => Ok(Interval::OneHour),
            120 => Ok(Interval::TwoHours),
            240 => Ok(Interval::FourHours),
            360 => Ok(Interval::SixHours),
            480 => Ok(Interval::EightHours),
            720 => Ok(Interval::TwelveHours),
            1440 => Ok(Interval::OneDay),
            4320 => Ok(Interval::ThreeDays),
            10080 => Ok(Interval::OneWeek),
            43200 => Ok(Interval::OneMonth),
            _ => Err(ParseIntervalError::InvalidMinutes(minutes)),
        }
    }
}

/// Custom error type for parsing intervals.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseIntervalError {
    InvalidMinutes(usize),
    InvalidString(String),
}

impl Display for ParseIntervalError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            ParseIntervalError::InvalidMinutes(m) => {
                write!(f, "Invalid minutes for Interval: {}", m)
            }
            ParseIntervalError::InvalidString(s) => write!(f, "Invalid string for Interval: {}", s),
        }
    }
}

impl std::error::Error for ParseIntervalError {}

impl FromStr for Interval {
    type Err = ParseIntervalError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1m" => Ok(Interval::OneMinute),
            "3m" => Ok(Interval::ThreeMinutes),
            "5m" => Ok(Interval::FiveMinutes),
            "15m" => Ok(Interval::FifteenMinutes),
            "30m" => Ok(Interval::ThirtyMinutes),
            "1h" => Ok(Interval::OneHour),
            "2h" => Ok(Interval::TwoHours),
            "4h" => Ok(Interval::FourHours),
            "6h" => Ok(Interval::SixHours),
            "8h" => Ok(Interval::EightHours),
            "12h" => Ok(Interval::TwelveHours),
            "1d" => Ok(Interval::OneDay),
            "3d" => Ok(Interval::ThreeDays),
            "1w" => Ok(Interval::OneWeek),
            "1M" => Ok(Interval::OneMonth),
            _ => Err(ParseIntervalError::InvalidString(s.to_string())),
        }
    }
}

impl Serialize for Interval {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Interval {
    fn deserialize<D>(deserializer: D) -> Result<Interval, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Interval::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Display for Interval {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}", self.as_str())
    }
}

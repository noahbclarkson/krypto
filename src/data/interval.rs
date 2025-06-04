use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::error::ParseIntervalError;

/// Represents various time intervals used by Binance.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")] // Use custom conversion for string representation
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
            Interval::OneMonth => 43200, // Approximation
        }
    }

    /// Returns the duration in milliseconds.
    pub fn to_milliseconds(&self) -> i64 {
        self.to_minutes() * 60 * 1000
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
    pub fn from_minutes(minutes: i64) -> Result<Self, ParseIntervalError> {
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
            _ => Err(ParseIntervalError::ParseIntError(minutes as usize)), // Keep usize for error type consistency?
        }
    }
}

// --- String Conversion ---

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
            _ => Err(ParseIntervalError::ParseStringError(s.to_string())),
        }
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
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
        };
        write!(f, "{}", s)
    }
}

// --- Serde Integration ---

impl From<Interval> for String {
    fn from(interval: Interval) -> Self {
        interval.to_string()
    }
}

impl TryFrom<String> for Interval {
    type Error = ParseIntervalError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Interval::from_str(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_string() {
        // Renamed from test_to_str
        assert_eq!(Interval::OneMinute.to_string(), "1m");
        assert_eq!(Interval::ThreeMinutes.to_string(), "3m");
        assert_eq!(Interval::FiveMinutes.to_string(), "5m");
        assert_eq!(Interval::FifteenMinutes.to_string(), "15m");
        assert_eq!(Interval::ThirtyMinutes.to_string(), "30m");
        assert_eq!(Interval::OneHour.to_string(), "1h");
        assert_eq!(Interval::TwoHours.to_string(), "2h");
        assert_eq!(Interval::FourHours.to_string(), "4h");
        assert_eq!(Interval::SixHours.to_string(), "6h");
        assert_eq!(Interval::EightHours.to_string(), "8h");
        assert_eq!(Interval::TwelveHours.to_string(), "12h");
        assert_eq!(Interval::OneDay.to_string(), "1d");
        assert_eq!(Interval::ThreeDays.to_string(), "3d");
        assert_eq!(Interval::OneWeek.to_string(), "1w");
        assert_eq!(Interval::OneMonth.to_string(), "1M");
    }

    #[test]
    fn test_to_minutes() {
        assert_eq!(Interval::OneMinute.to_minutes(), 1);
        assert_eq!(Interval::ThreeMinutes.to_minutes(), 3);
        assert_eq!(Interval::FiveMinutes.to_minutes(), 5);
        assert_eq!(Interval::FifteenMinutes.to_minutes(), 15);
        assert_eq!(Interval::ThirtyMinutes.to_minutes(), 30);
        assert_eq!(Interval::OneHour.to_minutes(), 60);
        assert_eq!(Interval::TwoHours.to_minutes(), 120);
        assert_eq!(Interval::FourHours.to_minutes(), 240);
        assert_eq!(Interval::SixHours.to_minutes(), 360);
        assert_eq!(Interval::EightHours.to_minutes(), 480);
        assert_eq!(Interval::TwelveHours.to_minutes(), 720);
        assert_eq!(Interval::OneDay.to_minutes(), 1440);
        assert_eq!(Interval::ThreeDays.to_minutes(), 4320);
        assert_eq!(Interval::OneWeek.to_minutes(), 10080);
        assert_eq!(Interval::OneMonth.to_minutes(), 43200);
    }

    #[test]
    fn test_to_milliseconds() {
        assert_eq!(Interval::OneMinute.to_milliseconds(), 60_000);
        assert_eq!(Interval::OneHour.to_milliseconds(), 3_600_000);
        assert_eq!(Interval::OneDay.to_milliseconds(), 86_400_000);
    }

    #[test]
    fn test_from_minutes() {
        assert_eq!(Interval::from_minutes(1).unwrap(), Interval::OneMinute);
        assert_eq!(Interval::from_minutes(3).unwrap(), Interval::ThreeMinutes);
        assert_eq!(Interval::from_minutes(5).unwrap(), Interval::FiveMinutes);
        assert_eq!(
            Interval::from_minutes(15).unwrap(),
            Interval::FifteenMinutes
        );
        assert_eq!(Interval::from_minutes(30).unwrap(), Interval::ThirtyMinutes);
        assert_eq!(Interval::from_minutes(60).unwrap(), Interval::OneHour);
        assert_eq!(Interval::from_minutes(120).unwrap(), Interval::TwoHours);
        assert_eq!(Interval::from_minutes(240).unwrap(), Interval::FourHours);
        assert_eq!(Interval::from_minutes(360).unwrap(), Interval::SixHours);
        assert_eq!(Interval::from_minutes(480).unwrap(), Interval::EightHours);
        assert_eq!(Interval::from_minutes(720).unwrap(), Interval::TwelveHours);
        assert_eq!(Interval::from_minutes(1440).unwrap(), Interval::OneDay);
        assert_eq!(Interval::from_minutes(4320).unwrap(), Interval::ThreeDays);
        assert_eq!(Interval::from_minutes(10080).unwrap(), Interval::OneWeek);
        assert_eq!(Interval::from_minutes(43200).unwrap(), Interval::OneMonth);
    }

    #[test]
    fn test_from_minutes_invalid() {
        let invalid_minutes = [0, 2, 10, 999];
        for &minutes in &invalid_minutes {
            let result = Interval::from_minutes(minutes);
            assert!(result.is_err());
            match result {
                Err(ParseIntervalError::ParseIntError(m)) => assert_eq!(m, minutes as usize),
                _ => panic!("Expected ParseIntError"),
            }
        }
    }

    #[test]
    fn test_from_str() {
        assert_eq!("1m".parse::<Interval>().unwrap(), Interval::OneMinute);
        assert_eq!("3m".parse::<Interval>().unwrap(), Interval::ThreeMinutes);
        assert_eq!("5m".parse::<Interval>().unwrap(), Interval::FiveMinutes);
        assert_eq!("15m".parse::<Interval>().unwrap(), Interval::FifteenMinutes);
        assert_eq!("30m".parse::<Interval>().unwrap(), Interval::ThirtyMinutes);
        assert_eq!("1h".parse::<Interval>().unwrap(), Interval::OneHour);
        assert_eq!("2h".parse::<Interval>().unwrap(), Interval::TwoHours);
        assert_eq!("4h".parse::<Interval>().unwrap(), Interval::FourHours);
        assert_eq!("6h".parse::<Interval>().unwrap(), Interval::SixHours);
        assert_eq!("8h".parse::<Interval>().unwrap(), Interval::EightHours);
        assert_eq!("12h".parse::<Interval>().unwrap(), Interval::TwelveHours);
        assert_eq!("1d".parse::<Interval>().unwrap(), Interval::OneDay);
        assert_eq!("3d".parse::<Interval>().unwrap(), Interval::ThreeDays);
        assert_eq!("1w".parse::<Interval>().unwrap(), Interval::OneWeek);
        assert_eq!("1M".parse::<Interval>().unwrap(), Interval::OneMonth);
    }

    #[test]
    fn test_from_str_invalid() {
        let invalid_strings = ["0m", "2m", "10h", "abc", ""];
        for &s in &invalid_strings {
            let result = s.parse::<Interval>();
            assert!(result.is_err());
            match result {
                Err(ParseIntervalError::ParseStringError(ref e)) => assert_eq!(e, s), // Changed error variant name
                _ => panic!("Expected ParseStringError"),
            }
        }
    }

    #[test]
    fn test_serde_serialization() {
        let interval = Interval::FourHours;
        let yaml = serde_yaml::to_string(&interval).unwrap();
        assert_eq!(yaml.trim(), "4h"); // trim to remove potential newline

        let json = serde_json::to_string(&interval).unwrap();
        assert_eq!(json, "\"4h\"");
    }

    #[test]
    fn test_serde_deserialization() {
        let yaml = "15m";
        let interval: Interval = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(interval, Interval::FifteenMinutes);

        let json = "\"1d\"";
        let interval_json: Interval = serde_json::from_str(json).unwrap();
        assert_eq!(interval_json, Interval::OneDay);

        let invalid_yaml = "10x";
        let result: Result<Interval, _> = serde_yaml::from_str(invalid_yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_ordering() {
        assert!(Interval::OneMinute < Interval::OneHour);
        assert!(Interval::OneDay > Interval::TwelveHours);
        assert_eq!(Interval::FourHours, Interval::FourHours);
    }
}

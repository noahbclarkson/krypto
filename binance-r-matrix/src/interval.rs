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


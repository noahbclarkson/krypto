use chrono::{DateTime, NaiveDate, TimeZone as _, Utc};

use crate::{data::interval::Interval, error::KryptoError};

// 1 minute in milliseconds
pub const MINS_TO_MILLIS: i64 = 60 * 1000;

pub fn date_to_datetime(date: &NaiveDate) -> Result<DateTime<Utc>, KryptoError> {
    Ok(Utc.from_utc_datetime(
        &date
            .and_hms_opt(0, 0, 0)
            .ok_or(KryptoError::DateConversionError(date.to_string()))?,
    ))
}

pub fn get_timestamps(
    start_time: i64,
    end_time: i64,
    interval: Interval,
) -> Result<Vec<(i64, i64)>, KryptoError> {
    let mut timestamps = Vec::new();
    let mut current_time = start_time;
    let interval_millis = interval.to_minutes() * MINS_TO_MILLIS;
    while current_time < end_time {
        let next_time = current_time + interval_millis * 1000;
        let next_time = if next_time > end_time {
            end_time
        } else {
            next_time
        };
        timestamps.push((current_time, next_time));
        current_time = next_time;
    }
    Ok(timestamps)
}

pub fn days_between(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    (end - start).num_days()
}

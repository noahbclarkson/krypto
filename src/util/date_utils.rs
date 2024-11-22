use chrono::{DateTime, NaiveDate, TimeZone as _, Utc};

use crate::{data::interval::Interval, error::KryptoError};

// 1 minute in milliseconds
pub const MINS_TO_MILLIS: i64 = 60 * 1000;

/**
Convert a DateTime<Utc> to a NaiveDate.

## Arguments
* `datetime` - The DateTime<Utc> to convert to a NaiveDate.

## Returns
The NaiveDate representation of the given DateTime<Utc>.
 */
pub fn date_to_datetime(date: &NaiveDate) -> Result<DateTime<Utc>, KryptoError> {
    Ok(Utc.from_utc_datetime(
        &date
            .and_hms_opt(0, 0, 0)
            .ok_or(KryptoError::DateConversionError(date.to_string()))?,
    ))
}

/**
Get the timestamps for the given start and end times with the given interval.
The timestamps will be in the form of (start_time, end_time) pairs.

## Arguments
* `start_time` - The start time in milliseconds.
* `end_time` - The end time in milliseconds.
* `interval` - The interval to use for the timestamps.

## Returns
A Result containing the timestamps if successful, or a KryptoError if an error occurred.
 */
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

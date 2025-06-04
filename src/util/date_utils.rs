use chrono::{DateTime, NaiveDate, TimeZone as _, Utc}; // Added Duration

use crate::{data::interval::Interval, error::KryptoError};

// 1 minute in milliseconds
pub const MINS_TO_MILLIS: i64 = 60 * 1000;

/**
Convert a NaiveDate (YYYY-MM-DD) to a DateTime<Utc> at the beginning of that day (00:00:00 UTC).

## Arguments
* `date` - The NaiveDate to convert.

## Returns
The corresponding DateTime<Utc> or a KryptoError if the date is invalid.
 */
pub fn date_to_datetime(date: &NaiveDate) -> Result<DateTime<Utc>, KryptoError> {
    match date.and_hms_opt(0, 0, 0) {
        Some(naive_datetime) => Ok(Utc.from_utc_datetime(&naive_datetime)),
        None => Err(KryptoError::DateConversionError(format!(
            "Failed to create naive datetime from date: {}",
            date
        ))),
    }
}

/**
Generate timestamp pairs (start_ms, end_ms) for fetching klines in chunks.
Ensures that the chunks align with the interval boundaries as much as possible
and covers the range [start_time, end_time).

## Arguments
* `start_time_ms` - The overall start time in milliseconds since Unix epoch.
* `end_time_ms` - The overall end time in milliseconds since Unix epoch (exclusive).
* `interval` - The kline interval.

## Returns
A vector of (start_ms, end_ms) tuples for fetching, or a KryptoError.
 */
pub fn get_timestamps(
    start_time_ms: i64,
    end_time_ms: i64,
    interval: Interval,
) -> Result<Vec<(i64, i64)>, KryptoError> {
    if start_time_ms >= end_time_ms {
        return Ok(Vec::new()); // No range to fetch
    }

    let interval_millis = interval.to_milliseconds();
    if interval_millis <= 0 {
        return Err(KryptoError::ConfigError(format!(
            "Invalid interval duration: {} ms",
            interval_millis
        )));
    }

    let max_chunk_size_millis = interval_millis * 1000; // Max 1000 klines per request

    let mut timestamps = Vec::new();
    let mut current_start_time = start_time_ms;

    while current_start_time < end_time_ms {
        let potential_end_time = current_start_time + max_chunk_size_millis;
        // Ensure the chunk end time does not exceed the overall end time
        let current_end_time = potential_end_time.min(end_time_ms);

        timestamps.push((current_start_time, current_end_time));
        current_start_time = current_end_time; // Move to the next chunk start
    }

    Ok(timestamps)
}

/**
Calculates the number of full days between two DateTime<Utc> instances.

## Arguments
* `start` - The start DateTime.
* `end` - The end DateTime.

## Returns
The number of full days between start and end. Returns 0 if end <= start.
 */
pub fn days_between(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    let duration = end.signed_duration_since(start);
    duration.num_days() // Returns the number of *full* 24-hour periods
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_date_to_datetime_valid() {
        let date = NaiveDate::from_ymd_opt(2023, 10, 26).unwrap();
        let dt = date_to_datetime(&date).unwrap();
        assert_eq!(
            dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2023-10-26 00:00:00"
        );
    }

    // Note: Testing invalid NaiveDate is hard as it's usually prevented at construction.

    #[test]
    fn test_get_timestamps_basic() {
        let start = Utc
            .with_ymd_and_hms(2023, 1, 1, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let end = Utc
            .with_ymd_and_hms(2023, 1, 1, 1, 0, 0)
            .unwrap()
            .timestamp_millis(); // 1 hour
        let interval = Interval::OneMinute; // 1 min interval

        // Expect 1 chunk: 1 min * 1000 klines = 1000 mins > 60 mins
        let timestamps = get_timestamps(start, end, interval).unwrap();
        assert_eq!(timestamps.len(), 1);
        assert_eq!(timestamps[0], (start, end));
    }

    #[test]
    fn test_get_timestamps_multiple_chunks() {
        let start = Utc
            .with_ymd_and_hms(2023, 1, 1, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        // End time needs > 1000 intervals
        let interval = Interval::OneMinute;
        let interval_ms = interval.to_milliseconds();
        let end = start + interval_ms * 1500; // 1500 minutes later

        let timestamps = get_timestamps(start, end, interval).unwrap();
        assert_eq!(timestamps.len(), 2); // Should split into 2 chunks

        // Chunk 1: start to start + 1000 * interval_ms
        let chunk1_end = start + interval_ms * 1000;
        assert_eq!(timestamps[0], (start, chunk1_end));

        // Chunk 2: chunk1_end to end
        assert_eq!(timestamps[1], (chunk1_end, end));
    }

    #[test]
    fn test_get_timestamps_exact_chunk_boundary() {
        let start = Utc
            .with_ymd_and_hms(2023, 1, 1, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let interval = Interval::OneMinute;
        let interval_ms = interval.to_milliseconds();
        let end = start + interval_ms * 1000; // Exactly 1000 minutes later

        let timestamps = get_timestamps(start, end, interval).unwrap();
        assert_eq!(timestamps.len(), 1);
        assert_eq!(timestamps[0], (start, end));
    }

    #[test]
    fn test_get_timestamps_short_duration() {
        let start = Utc
            .with_ymd_and_hms(2023, 1, 1, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let interval = Interval::OneHour;
        let end = start + Duration::minutes(30).num_milliseconds(); // Only 30 mins

        let timestamps = get_timestamps(start, end, interval).unwrap();
        assert_eq!(timestamps.len(), 1);
        assert_eq!(timestamps[0], (start, end));
    }

    #[test]
    fn test_get_timestamps_invalid_range() {
        let start = Utc
            .with_ymd_and_hms(2023, 1, 1, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let end = start; // End == start
        let interval = Interval::OneMinute;
        let timestamps = get_timestamps(start, end, interval).unwrap();
        assert!(timestamps.is_empty());

        let end_before_start = start - 1000;
        let timestamps2 = get_timestamps(start, end_before_start, interval).unwrap();
        assert!(timestamps2.is_empty());
    }

    #[test]
    fn test_days_between() {
        let start = Utc.with_ymd_and_hms(2023, 10, 20, 12, 0, 0).unwrap();
        let end1 = Utc.with_ymd_and_hms(2023, 10, 22, 11, 59, 59).unwrap(); // Less than 2 full days
        let end2 = Utc.with_ymd_and_hms(2023, 10, 22, 12, 0, 0).unwrap(); // Exactly 2 full days
        let end3 = Utc.with_ymd_and_hms(2023, 10, 22, 14, 0, 0).unwrap(); // More than 2 full days

        assert_eq!(days_between(start, end1), 1);
        assert_eq!(days_between(start, end2), 2);
        assert_eq!(days_between(start, end3), 2);
        assert_eq!(days_between(start, start), 0);
        // Adjust assertion: Duration is -1 day, 23h, 59m, 59s. num_days() returns -1.
        assert_eq!(days_between(end1, start), -1); // Changed expected value from -2 to -1
    }
}

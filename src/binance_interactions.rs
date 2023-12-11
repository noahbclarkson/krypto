use std::time::Duration;

use binance_r_matrix::interval::Interval;
use chrono::Utc;

const MINS_TO_MILLIS: usize = 60000;
const WAIT_WINDOW: usize = 5000;

#[inline]
pub async fn wait(interval: &Interval, periods: usize) {
    for _ in 0..periods {
        loop {
            let now = Utc::now().timestamp_millis() as usize;
            let millis = interval.to_minutes() * MINS_TO_MILLIS;
            let next_interval = (now / millis) * millis + millis;
            let wait_time = next_interval - now - WAIT_WINDOW;
            if wait_time > WAIT_WINDOW {
                tokio::time::sleep(Duration::from_millis(wait_time as u64)).await;
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(WAIT_WINDOW as u64 + 1)).await;
            }
        }
    }
}


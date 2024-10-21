// connection.rs

use chrono::{Duration, NaiveDateTime, Utc};
use once_cell::sync::Lazy;
use std::fmt::{Display, Formatter, Result as FmtResult};
use tokio::sync::Mutex;
use tracing::{debug, instrument};

/// The maximum allowed weight before it's considered invalid, in accordance with Binance API limits but with a buffer.
const MAX_WEIGHT: u32 = 5900;

/// A global asynchronous mutex protecting the WeightInformation.
pub static REQUESTS: Lazy<Mutex<WeightInformation>> =
    Lazy::new(|| Mutex::new(WeightInformation::default()));

/// Whether we have logged the weight limit exceeded message.
static LOGGED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));

/// Struct to keep track of weight and the last reset time.
#[derive(Debug, Clone)]
pub struct WeightInformation {
    pub weight: u32,
    pub last_reset: NaiveDateTime,
}

impl Default for WeightInformation {
    fn default() -> Self {
        WeightInformation {
            weight: 100, // To account for possible initial or prior requests
            last_reset: Utc::now().naive_utc(),
        }
    }
}

/// Enum representing the result of a weight check.
#[derive(Debug, PartialEq, Eq)]
pub enum WeightInformationData {
    /// The current operation is valid and does not exceed the weight limit.
    Valid,
    /// The current operation is invalid as it exceeds the weight limit.
    /// Contains the current weight and the last reset time.
    Invalid(u32, NaiveDateTime),
}

impl WeightInformation {
    /// Updates the weight by adding the provided `weight`.
    /// If more than one minute has passed since the last reset, it resets the weight.
    ///
    /// # Arguments
    ///
    /// * `weight` - The weight to add.
    #[instrument(level = "debug")]
    pub async fn update(&mut self, weight: u32) {
        let current_time = Utc::now().naive_utc();
        if self.last_reset + Duration::seconds(61) < current_time {
            debug!(?self, "Resetting weight");
            self.weight = 0;
            self.last_reset = current_time;
            *LOGGED.lock().await = false;
        }
        self.weight = self.weight.saturating_add(weight); // Prevents potential overflow
    }

    /// Checks if adding the provided `weight` would exceed the `MAX_WEIGHT`.
    ///
    /// # Arguments
    ///
    /// * `weight` - The weight to check.
    ///
    /// # Returns
    ///
    /// * `WeightInformationData::Valid` if the operation is within the limit.
    /// * `WeightInformationData::Invalid` with current weight and last reset time if it exceeds.
    pub async fn check(&mut self, weight: u32) -> WeightInformationData {
        self.update(0).await;
        if self.weight.saturating_add(weight) >= MAX_WEIGHT {
            WeightInformationData::Invalid(self.weight, self.last_reset)
        } else {
            WeightInformationData::Valid
        }
    }
}

impl Display for WeightInformationData {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            WeightInformationData::Valid => write!(f, "Valid"),
            WeightInformationData::Invalid(weight, reset_time) => {
                write!(f, "Invalid: weight={}, last_reset={}", weight, reset_time)
            }
        }
    }
}

pub async fn check_and_wait(weight: u32) {
    let mut requests = REQUESTS.lock().await;
    while let WeightInformationData::Invalid(_, _) = requests.check(weight).await {
        if !*LOGGED.lock().await {
            debug!("Weight limit exceeded. Waiting for reset...");
            *LOGGED.lock().await = true;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
    requests.update(weight).await;
}
use serde::{Deserialize, Serialize}; // Added for caching
use ta::{indicators::*, Next};
use tracing::warn;

use super::candlestick::Candlestick;
use crate::error::KryptoError; // Import KryptoError

#[derive(Debug, Clone, Serialize, Deserialize)] // Added Serialize, Deserialize
pub struct Technicals {
    // Store as a map for easier access and serialization? Or keep Vec for order?
    // Using Vec assumes the order matches the requested `technical_names`
    technicals: Vec<Technical>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)] // Added Serialize, Deserialize, PartialEq
pub enum Technical {
    RSI(f64),
    FastStochastic(f64),
    SlowStochastic(f64),
    CCI(f64),
    MFI(f64),
    EfficiencyRatio(f64),
    PercentageChangeEMA(f64),
    VolumePercentageChangeEMA(f64),
    BollingerBands(f64), // Represents %B ((Price - Lower) / (Upper - Lower))
    CandlestickRatio(f64),
    // Add more indicators here
}

impl Technicals {
    /// Calculates technical indicators for a series of candlesticks.
    /// Handles NaN/Infinity values gracefully by replacing them with 0.0 or previous valid value.
    pub fn get_technicals(
        data: &[Candlestick],
        technical_names: &[String],
    ) -> Result<Vec<Self>, KryptoError> {
        if data.is_empty() {
            return Ok(Vec::new());
        }
        if technical_names.is_empty() {
            return Err(KryptoError::ConfigError(
                "Technical names cannot be empty for calculation.".to_string(),
            ));
        }

        // Initialize indicators - consider parameterizing periods from config/GA
        let mut rsi_ind = RelativeStrengthIndex::default(); // Default period 14
        let mut fast_stoch_ind = FastStochastic::default(); // Default k=14, d=3
        let mut slow_stoch_ind = SlowStochastic::default(); // Default k=14, d=3
        let mut cci_ind = CommodityChannelIndex::default(); // Default period 20
        let mut mfi_ind = MoneyFlowIndex::default(); // Default period 14
        let mut er_ind = EfficiencyRatio::default(); // Default period 10
        let mut pc_ema_ind = PercentageChangeEMA::default(); // Default period 14
        let mut vol_pc_ema_ind = PercentageChangeEMA::default(); // Default period 14
        let mut bb_ind = BollingerBands::default(); // Default period 20, std_dev 2

        let mut results = Vec::with_capacity(data.len());

        for candle in data {
            // Calculate raw indicator values
            let rsi_val = rsi_ind.next(candle);
            let fast_stoch_val = fast_stoch_ind.next(candle);
            let slow_stoch_val = slow_stoch_ind.next(candle);
            let cci_val = cci_ind.next(candle);
            let mfi_val = mfi_ind.next(candle);
            let er_val = er_ind.next(candle);
            let pc_ema_val = pc_ema_ind.next(candle.close);
            let vol_pc_ema_val = vol_pc_ema_ind.next(candle.volume);
            let bb_val = bb_ind.next(candle.close);
            let cr_val = candlestick_ratio(candle);

            // Calculate Bollinger Bands %B
            let bb_pct = if (bb_val.upper - bb_val.lower).abs() < f64::EPSILON {
                0.5 // Avoid division by zero, assume midpoint
            } else {
                (candle.close - bb_val.lower) / (bb_val.upper - bb_val.lower)
            };

            let mut current_technicals = Vec::with_capacity(technical_names.len());
            for name in technical_names {
                let value = match name.as_str() {
                    "RSI" => rsi_val,
                    "Fast Stochastic" => fast_stoch_val,
                    "Slow Stochastic" => slow_stoch_val,
                    "CCI" => cci_val,
                    "MFI" => mfi_val,
                    "Efficiency Ratio" => er_val,
                    "Percentage Change EMA" => pc_ema_val,
                    "Volume Percentage Change EMA" => vol_pc_ema_val,
                    "Bollinger Bands" => bb_pct,
                    "Candlestick Ratio" => cr_val,
                    _ => {
                        // Return error instead of panicking
                        return Err(KryptoError::TechnicalIndicatorError {
                            indicator: name.clone(),
                            reason: "Unknown technical indicator name requested".to_string(),
                        });
                    }
                };

                // Sanitize value (replace NaN/Infinity with 0.0)
                let sanitized_value = if value.is_nan() || value.is_infinite() {
                    // warn!("NaN or Infinity detected for {} at {}: Replaced with 0.0", name, candle.open_time);
                    0.0 // Or consider using the last valid value if available
                } else {
                    value
                };

                // Create Technical enum variant
                let technical = Technical::from_name(name, sanitized_value)?; // Use Result-based creation
                current_technicals.push(technical);
            }
            results.push(Technicals {
                technicals: current_technicals,
            });
        }
        Ok(results)
    }

    /// Returns the technical indicator values as a flat vector of f64.
    /// The order matches the internal `technicals` vector.
    pub fn as_array(&self) -> Vec<f64> {
        self.technicals.iter().map(|t| t.value()).collect()
    }

    /// Returns the number of indicators stored.
    pub fn len(&self) -> usize {
        self.technicals.len()
    }

    /// Checks if there are any indicators stored.
    pub fn is_empty(&self) -> bool {
        self.technicals.is_empty()
    }
}

impl Technical {
    /// Gets the underlying f64 value of the indicator.
    pub fn value(&self) -> f64 {
        match self {
            Technical::RSI(v)
            | Technical::FastStochastic(v)
            | Technical::SlowStochastic(v)
            | Technical::CCI(v)
            | Technical::MFI(v)
            | Technical::EfficiencyRatio(v)
            | Technical::PercentageChangeEMA(v)
            | Technical::VolumePercentageChangeEMA(v)
            | Technical::BollingerBands(v)
            | Technical::CandlestickRatio(v) => *v,
        }
    }

    /// Gets the standard name of the indicator.
    pub fn name(&self) -> &'static str {
        match self {
            Technical::RSI(_) => "RSI",
            Technical::FastStochastic(_) => "Fast Stochastic",
            Technical::SlowStochastic(_) => "Slow Stochastic",
            Technical::CCI(_) => "CCI",
            Technical::MFI(_) => "MFI",
            Technical::EfficiencyRatio(_) => "Efficiency Ratio",
            Technical::PercentageChangeEMA(_) => "Percentage Change EMA",
            Technical::VolumePercentageChangeEMA(_) => "Volume Percentage Change EMA",
            Technical::BollingerBands(_) => "Bollinger Bands",
            Technical::CandlestickRatio(_) => "Candlestick Ratio",
        }
    }

    /// Creates a Technical enum variant from its name and value.
    /// Returns an error if the name is unknown.
    pub fn from_name(name: &str, value: f64) -> Result<Self, KryptoError> {
        match name {
            "RSI" => Ok(Technical::RSI(value)),
            "Fast Stochastic" => Ok(Technical::FastStochastic(value)),
            "Slow Stochastic" => Ok(Technical::SlowStochastic(value)),
            "CCI" => Ok(Technical::CCI(value)),
            "MFI" => Ok(Technical::MFI(value)),
            "Efficiency Ratio" => Ok(Technical::EfficiencyRatio(value)),
            "Percentage Change EMA" => Ok(Technical::PercentageChangeEMA(value)),
            "Volume Percentage Change EMA" => Ok(Technical::VolumePercentageChangeEMA(value)),
            "Bollinger Bands" => Ok(Technical::BollingerBands(value)),
            "Candlestick Ratio" => Ok(Technical::CandlestickRatio(value)),
            _ => Err(KryptoError::TechnicalIndicatorError {
                indicator: name.to_string(),
                reason: "Unknown technical indicator name provided".to_string(),
            }),
        }
    }
}

// --- Custom Indicator Implementations ---

/// Calculates the EMA of the percentage change between consecutive values.
#[derive(Debug, Clone)]
pub struct PercentageChangeEMA {
    pub period: usize,
    pub ema: ExponentialMovingAverage,
    last: Option<f64>,
}

impl Default for PercentageChangeEMA {
    fn default() -> Self {
        Self::new(14).expect("Default EMA period should be valid") // Default period 14
    }
}

impl Next<f64> for PercentageChangeEMA {
    type Output = f64;

    fn next(&mut self, value: f64) -> Self::Output {
        if value.is_nan() || value.is_infinite() {
            return self.ema.next(0.0);
        }

        let percentage_change = match self.last {
            Some(last_val) if last_val.abs() > f64::EPSILON => (value - last_val) / last_val,
            _ => 0.0, // No previous value or previous value was zero
        };

        self.last = Some(value); // Update last value *after* calculating change

        // Sanitize percentage change before feeding to EMA
        let sanitized_change = if percentage_change.is_nan() || percentage_change.is_infinite() {
            0.0
        } else {
            percentage_change
        };

        self.ema.next(sanitized_change)
    }
}

impl PercentageChangeEMA {
    /// Creates a new PercentageChangeEMA indicator.
    /// Returns an error if the period is invalid for EMA.
    pub fn new(period: usize) -> Result<Self, KryptoError> {
        let ema = ExponentialMovingAverage::new(period).map_err(|e| {
            KryptoError::TechnicalIndicatorError {
                indicator: "PercentageChangeEMA".to_string(),
                reason: format!("Invalid period {} for EMA: {}", period, e),
            }
        })?;
        Ok(PercentageChangeEMA {
            period,
            ema,
            last: None,
        })
    }
}

/// Calculates the candlestick ratio: tanh((upper_wick / body) - (lower_wick / body))
/// Represents the relative strength of wicks compared to the body.
/// Ranges from -1 (strong lower wick) to +1 (strong upper wick). 0 means balanced wicks or no body.
fn candlestick_ratio(candle: &Candlestick) -> f64 {
    let top = candle.close.max(candle.open);
    let bottom = candle.close.min(candle.open);
    let upper_wick = candle.high - top;
    let lower_wick = bottom - candle.low;
    let body = top - bottom;

    // Handle zero body case (Doji or near-Doji)
    if body.abs() < f64::EPSILON {
        // Could return 0.0, or analyze wick difference directly?
        // Returning 0.0 indicates no body dominance.
        return 0.0;
    }

    // Ensure wicks are non-negative (can happen with bad data)
    let upper_wick = upper_wick.max(0.0);
    let lower_wick = lower_wick.max(0.0);

    let ratio = (upper_wick / body) - (lower_wick / body);

    // Use tanh to squash the ratio into the [-1, 1] range
    let result = ratio.tanh();

    // Final sanity check for NaN/Infinity, though unlikely after tanh
    if result.is_nan() || result.is_infinite() {
        warn!(
            "NaN or Infinity in candlestick_ratio calculation for candle at {}",
            candle.open_time
        );
        0.0
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn create_test_candle(o: f64, h: f64, l: f64, c: f64, time_offset_secs: i64) -> Candlestick {
        let now = Utc::now().timestamp_millis();
        Candlestick {
            open_time: Utc
                .timestamp_millis_opt(now + time_offset_secs * 1000)
                .single()
                .unwrap(),
            close_time: Utc
                .timestamp_millis_opt(now + (time_offset_secs + 60) * 1000)
                .single()
                .unwrap(),
            open: o,
            high: h,
            low: l,
            close: c,
            volume: 100.0,
        }
    }

    #[test]
    fn test_candlestick_ratio_bullish() {
        // Strong upper wick, small lower wick
        let candle = create_test_candle(100.0, 110.0, 99.0, 105.0, 0);
        // body = 5, upper_wick = 5, lower_wick = 1
        // ratio = (5/5) - (1/5) = 1 - 0.2 = 0.8
        let expected = 0.8f64.tanh();
        assert!((candlestick_ratio(&candle) - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn test_candlestick_ratio_bearish() {
        // Small upper wick, strong lower wick
        let candle = create_test_candle(105.0, 106.0, 95.0, 100.0, 0);
        // body = 5, upper_wick = 1, lower_wick = 5
        // ratio = (1/5) - (5/5) = 0.2 - 1 = -0.8
        let expected = (-0.8f64).tanh();
        assert!((candlestick_ratio(&candle) - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn test_candlestick_ratio_doji() {
        // No body
        let candle = create_test_candle(100.0, 105.0, 95.0, 100.0, 0);
        let expected = 0.0;
        assert!((candlestick_ratio(&candle) - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn test_candlestick_ratio_balanced() {
        // Equal wicks
        let candle = create_test_candle(100.0, 110.0, 90.0, 105.0, 0);
        // body = 5, upper_wick = 5, lower_wick = 10 (mistake here, low is 90, bottom is 100 -> lower wick = 10)
        // ratio = (5/5) - (10/5) = 1 - 2 = -1.0 -> Let's re-evaluate
        // Candle: O=100, H=110, L=90, C=105 -> Top=105, Bottom=100, Body=5
        // Upper Wick = H - Top = 110 - 105 = 5
        // Lower Wick = Bottom - L = 100 - 90 = 10
        // Ratio = (5/5) - (10/5) = 1 - 2 = -1.0
        let expected = (-1.0f64).tanh();
        assert!((candlestick_ratio(&candle) - expected).abs() < f64::EPSILON);

        // Truly balanced wicks relative to body
        let candle2 = create_test_candle(100.0, 110.0, 95.0, 105.0, 120); // Upper 5, Lower 5
                                                                          // Ratio = (5/5) - (5/5) = 0
        let expected2 = 0.0f64.tanh();
        assert!((candlestick_ratio(&candle2) - expected2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_percentage_change_ema() {
        let mut pc_ema = PercentageChangeEMA::new(3).unwrap(); // Short period for testing
        let values = [100.0, 101.0, 103.03, 102.0, 104.04]; // Approx +1%, +2%, -1%, +2%

        let changes = [
            0.0,                       // First value, change is 0
            (101.0 - 100.0) / 100.0,   // 0.01
            (103.03 - 101.0) / 101.0,  // 0.020099... ~ 0.02
            (102.0 - 103.03) / 103.03, // -0.0100... ~ -0.01
            (104.04 - 102.0) / 102.0,  // 0.02
        ];

        let mut expected_ema_values = Vec::new();
        let mut ema = ExponentialMovingAverage::new(3).unwrap();
        for change in changes {
            expected_ema_values.push(ema.next(change));
        }

        let mut actual_ema_values = Vec::new();
        for value in values {
            actual_ema_values.push(pc_ema.next(value));
        }

        println!("Expected EMA: {:?}", expected_ema_values);
        println!("Actual EMA: {:?}", actual_ema_values);

        assert_eq!(actual_ema_values.len(), expected_ema_values.len());
        for (actual, expected) in actual_ema_values.iter().zip(expected_ema_values.iter()) {
            assert!(
                (actual - expected).abs() < 1e-9,
                "Actual: {}, Expected: {}",
                actual,
                expected
            );
        }
    }

    #[test]
    fn test_get_technicals_unknown_name() {
        let candles = vec![create_test_candle(100.0, 110.0, 90.0, 105.0, 0)];
        let technical_names = vec!["RSI".to_string(), "UnknownIndicator".to_string()];
        let result = Technicals::get_technicals(&candles, &technical_names);
        assert!(result.is_err());
        match result {
            Err(KryptoError::TechnicalIndicatorError { indicator, .. }) => {
                assert_eq!(indicator, "UnknownIndicator");
            }
            _ => panic!("Expected TechnicalIndicatorError"),
        }
    }

    #[test]
    fn test_get_technicals_empty_input() {
        let candles = vec![];
        let technical_names = vec!["RSI".to_string()];
        let result = Technicals::get_technicals(&candles, &technical_names).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_technicals_valid() {
        let candles = vec![
            create_test_candle(100.0, 101.0, 99.0, 100.5, 0),
            create_test_candle(100.5, 102.0, 100.0, 101.5, 60),
            // Add more candles for better indicator calculation
        ];
        let technical_names = vec!["RSI".to_string(), "Candlestick Ratio".to_string()];
        let result = Technicals::get_technicals(&candles, &technical_names);
        assert!(result.is_ok());
        let technicals_vec = result.unwrap();
        assert_eq!(technicals_vec.len(), candles.len());
        assert_eq!(technicals_vec[0].len(), technical_names.len());
        assert_eq!(technicals_vec[0].technicals[0].name(), "RSI");
        assert_eq!(technicals_vec[0].technicals[1].name(), "Candlestick Ratio");
    }
}

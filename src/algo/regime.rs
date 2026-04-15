//! Market regime detection for adaptive strategy selection.
//!
//! Regime detection allows strategies to adapt to current market conditions:
//! - Low volatility → Mean-reversion strategies work better
//! - High volatility → Trend-following strategies work better
//! - Sideways → Reduce position size or avoid trading
//!
//! Based on research from arXiv:2601.19504 and QuantInsti regime-adaptive trading guide.

use polars::prelude::*;

/// Market regime classification for strategy selection.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MarketRegime {
    /// Strong uptrend with high momentum - use trend-following
    TrendingBull,
    /// Strong downtrend with high momentum - use trend-following or avoid
    TrendingBear,
    /// Low volatility, range-bound - use mean-reversion
    Sideways,
    /// High volatility with no clear direction - reduce exposure
    Volatile,
}

/// Volatility regime for strategy selection.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum VolatilityRegime {
    /// Low ATR relative to recent history - mean-reversion works
    LowVol,
    /// Normal volatility
    NormalVol,
    /// High ATR relative to recent history - trend-following works
    HighVol,
}

/// Combined regime information for strategy selection.
#[derive(Debug, Clone)]
pub struct RegimeInfo {
    pub market: MarketRegime,
    pub volatility: VolatilityRegime,
    /// ATR ratio (current ATR / 20-day MA of ATR)
    pub atr_ratio: f64,
    /// Trend strength (0-100, from ADX-like calculation)
    pub trend_strength: f64,
    /// Regime confidence (0.0-1.0)
    pub confidence: f64,
}

/// Rule-based regime detector using ATR ratio and trend filters.
///
/// This is a simpler alternative to HMM-based regime detection that
/// doesn't require ML dependencies.
pub struct RegimeDetector;

impl RegimeDetector {
    /// Detect market regime from DataFrame with standard indicators.
    ///
    /// Requires: ema_50, ema_200, atr, bb_width, close
    pub fn detect(df: &DataFrame) -> MarketRegime {
        let info = Self::detect_full(df);
        info.market
    }

    /// Detect full regime information including volatility and confidence.
    pub fn detect_full(df: &DataFrame) -> RegimeInfo {
        let ema_50 = df.column("ema_50").ok().and_then(|s| s.f64().ok());
        let ema_200 = df.column("ema_200").ok().and_then(|s| s.f64().ok());
        let bb_width = df.column("bb_width").ok().and_then(|s| s.f64().ok());
        let atr = df.column("atr").ok().and_then(|s| s.f64().ok());
        let close = df.column("close").ok().and_then(|s| s.f64().ok());

        // Default values
        let mut atr_ratio = 1.0;
        let mut trend_strength = 50.0;
        let mut market = MarketRegime::Sideways;
        let mut confidence = 0.5;

        if let (Some(e50), Some(e200), Some(bb)) = (ema_50, ema_200, bb_width) {
            let last_50 = e50.last().unwrap_or(0.0);
            let last_200 = e200.last().unwrap_or(0.0);
            let last_bb = bb.last().unwrap_or(0.0);

            // Calculate trend strength based on EMA separation
            let ema_sep = (last_50 - last_200).abs() / last_200;
            trend_strength = (ema_sep * 1000.0).min(100.0);

            // Bollinger Band width indicates volatility
            // Low BB width = sideways consolidation
            if last_bb < 0.03 {
                market = MarketRegime::Sideways;
                confidence = 0.7;
            } else if last_bb > 0.10 {
                // High BB width = high volatility
                market = MarketRegime::Volatile;
                confidence = 0.6;
            } else if last_50 > last_200 {
                market = MarketRegime::TrendingBull;
                confidence = 0.5 + (trend_strength / 200.0);
            } else {
                market = MarketRegime::TrendingBear;
                confidence = 0.5 + (trend_strength / 200.0);
            }
        }

        // Calculate ATR ratio for volatility regime
        if let (Some(atr_series), Some(close_series)) = (atr, close) {
            let len = atr_series.len();
            if len >= 20 {
                // Calculate ATR as percentage of price
                let current_atr_pct = atr_series.get(len - 1).unwrap_or(0.0)
                    / close_series.get(len - 1).unwrap_or(1.0);

                // 20-day MA of ATR%
                let atr_pct_ma: f64 = (0..20)
                    .map(|i| {
                        let atr_val = atr_series.get(len - 1 - i).unwrap_or(0.0);
                        let close_val = close_series.get(len - 1 - i).unwrap_or(1.0);
                        atr_val / close_val
                    })
                    .sum::<f64>()
                    / 20.0;

                if atr_pct_ma > 0.0 {
                    atr_ratio = current_atr_pct / atr_pct_ma;
                }
            }
        }

        // Determine volatility regime from ATR ratio
        let volatility = if atr_ratio < 0.7 {
            VolatilityRegime::LowVol
        } else if atr_ratio > 1.5 {
            VolatilityRegime::HighVol
        } else {
            VolatilityRegime::NormalVol
        };

        // Adjust market regime based on volatility
        if volatility == VolatilityRegime::HighVol && market == MarketRegime::Sideways {
            market = MarketRegime::Volatile;
        }

        RegimeInfo {
            market,
            volatility,
            atr_ratio,
            trend_strength,
            confidence: confidence.clamp(0.0, 1.0),
        }
    }

    /// Get recommended strategy type for current regime.
    ///
    /// Returns "mean_reversion" or "trend_following" based on regime.
    pub fn recommended_strategy_type(regime: &RegimeInfo) -> &'static str {
        match (&regime.market, &regime.volatility) {
            // Mean-reversion works in low-vol sideways markets
            (MarketRegime::Sideways, VolatilityRegime::LowVol) => "mean_reversion",
            (MarketRegime::Sideways, VolatilityRegime::NormalVol) => "mean_reversion",

            // Trend-following works in trending markets
            (MarketRegime::TrendingBull, _) => "trend_following",
            (MarketRegime::TrendingBear, _) => "trend_following",

            // High volatility - trend-following can work but with caution
            (MarketRegime::Volatile, VolatilityRegime::HighVol) => "trend_following",
            (MarketRegime::Volatile, _) => "reduce_exposure",

            // Default to mean-reversion
            _ => "mean_reversion",
        }
    }

    /// Get position size multiplier for current regime.
    ///
    /// Returns 0.0-1.5 based on regime confidence and volatility.
    pub fn position_size_multiplier(regime: &RegimeInfo) -> f64 {
        let base = match regime.volatility {
            VolatilityRegime::LowVol => 1.2,    // Larger positions in low-vol
            VolatilityRegime::NormalVol => 1.0, // Normal sizing
            VolatilityRegime::HighVol => 0.7,   // Smaller positions in high-vol
        };

        // Adjust by confidence
        base * regime.confidence
    }

    /// Calculate ATR percentile rank (0-100) relative to lookback period.
    ///
    /// This is more robust than absolute ATR thresholds because it adapts to
    /// each asset's typical volatility range. A percentile of 80 means current
    /// ATR is higher than 80% of historical values in the lookback period.
    ///
    /// # Arguments
    /// * `df` - DataFrame with "atr" and "close" columns
    /// * `lookback` - Number of bars to compare against (default: 100)
    ///
    /// # Returns
    /// Percentile rank (0-100), or 50.0 if insufficient data
    pub fn atr_percentile(df: &DataFrame, lookback: usize) -> f64 {
        let atr = match df.column("atr").ok().and_then(|s| s.f64().ok()) {
            Some(a) => a,
            None => return 50.0,
        };
        let close = match df.column("close").ok().and_then(|s| s.f64().ok()) {
            Some(c) => c,
            None => return 50.0,
        };

        let len = atr.len();
        if len < 2 {
            return 50.0;
        }

        // Calculate ATR% for each bar
        let current_atr_pct = atr.get(len - 1).unwrap_or(0.0) / close.get(len - 1).unwrap_or(1.0);

        // Collect historical ATR% values
        let start = len.saturating_sub(lookback);
        let historical: Vec<f64> = (start..len.saturating_sub(1))
            .filter_map(|i| {
                let a = atr.get(i).unwrap_or(0.0);
                let c = close.get(i).unwrap_or(1.0);
                if c > 0.0 {
                    Some(a / c)
                } else {
                    None
                }
            })
            .collect();

        if historical.is_empty() {
            return 50.0;
        }

        // Count how many historical values are below current
        let below = historical.iter().filter(|&&h| h < current_atr_pct).count();
        let total = historical.len();

        (below as f64 / total as f64) * 100.0
    }

    /// Determine volatility regime from ATR percentile.
    ///
    /// Uses percentile ranking rather than absolute thresholds for
    /// cross-asset consistency.
    pub fn volatility_regime_from_percentile(percentile: f64) -> VolatilityRegime {
        if percentile < 30.0 {
            VolatilityRegime::LowVol // Bottom 30% of historical volatility
        } else if percentile > 70.0 {
            VolatilityRegime::HighVol // Top 30% of historical volatility
        } else {
            VolatilityRegime::NormalVol // Middle 40%
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_df() -> DataFrame {
        let n = 100;
        let close: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.1)).collect();
        let ema_50: Vec<f64> = close.iter().map(|c| c * 0.99).collect();
        let ema_200: Vec<f64> = close.iter().map(|c| c * 0.98).collect();
        let atr: Vec<f64> = close.iter().map(|_| 2.0).collect();
        let bb_width: Vec<f64> = vec![0.05; n];

        df! [
            "close" => close,
            "ema_50" => ema_50,
            "ema_200" => ema_200,
            "atr" => atr,
            "bb_width" => bb_width,
        ]
        .unwrap()
    }

    #[test]
    fn test_detect_trending_bull() {
        let df = create_test_df();
        let regime = RegimeDetector::detect(&df);
        assert_eq!(regime, MarketRegime::TrendingBull);
    }

    #[test]
    fn test_detect_full_returns_info() {
        let df = create_test_df();
        let info = RegimeDetector::detect_full(&df);
        assert!(info.confidence > 0.0);
        assert!(info.confidence <= 1.0);
        assert!(info.atr_ratio > 0.0);
    }

    #[test]
    fn test_recommended_strategy_bull_trend() {
        let info = RegimeInfo {
            market: MarketRegime::TrendingBull,
            volatility: VolatilityRegime::NormalVol,
            atr_ratio: 1.0,
            trend_strength: 60.0,
            confidence: 0.7,
        };
        assert_eq!(
            RegimeDetector::recommended_strategy_type(&info),
            "trend_following"
        );
    }

    #[test]
    fn test_recommended_strategy_sideways_lowvol() {
        let info = RegimeInfo {
            market: MarketRegime::Sideways,
            volatility: VolatilityRegime::LowVol,
            atr_ratio: 0.6,
            trend_strength: 20.0,
            confidence: 0.7,
        };
        assert_eq!(
            RegimeDetector::recommended_strategy_type(&info),
            "mean_reversion"
        );
    }

    #[test]
    fn test_position_size_high_vol() {
        let info = RegimeInfo {
            market: MarketRegime::Volatile,
            volatility: VolatilityRegime::HighVol,
            atr_ratio: 2.0,
            trend_strength: 40.0,
            confidence: 0.6,
        };
        let mult = RegimeDetector::position_size_multiplier(&info);
        assert!(mult < 1.0); // Should reduce position in high vol
    }

    #[test]
    fn test_position_size_low_vol() {
        let info = RegimeInfo {
            market: MarketRegime::Sideways,
            volatility: VolatilityRegime::LowVol,
            atr_ratio: 0.5,
            trend_strength: 30.0,
            confidence: 0.9, // High confidence needed for larger positions
        };
        let mult = RegimeDetector::position_size_multiplier(&info);
        assert!(mult > 1.0); // Should increase position in low vol with high confidence
    }

    #[test]
    fn test_atr_percentile_high_vol() {
        // Create df with rising ATR (current is highest)
        let n = 100;
        let close: Vec<f64> = vec![100.0; n];
        let atr: Vec<f64> = (0..n).map(|i| 1.0 + (i as f64 * 0.1)).collect();

        let df = df! [
            "close" => close,
            "atr" => atr,
        ]
        .unwrap();

        let percentile = RegimeDetector::atr_percentile(&df, 50);
        assert!(percentile > 90.0); // Current ATR should be near 100th percentile
    }

    #[test]
    fn test_atr_percentile_low_vol() {
        // Create df with falling ATR (current is lowest)
        let n = 100;
        let close: Vec<f64> = vec![100.0; n];
        let atr: Vec<f64> = (0..n).map(|i| 10.0 - (i as f64 * 0.1)).collect();

        let df = df! [
            "close" => close,
            "atr" => atr,
        ]
        .unwrap();

        let percentile = RegimeDetector::atr_percentile(&df, 50);
        assert!(percentile < 10.0); // Current ATR should be near 0th percentile
    }

    #[test]
    fn test_volatility_regime_from_percentile() {
        assert_eq!(
            RegimeDetector::volatility_regime_from_percentile(20.0),
            VolatilityRegime::LowVol
        );
        assert_eq!(
            RegimeDetector::volatility_regime_from_percentile(50.0),
            VolatilityRegime::NormalVol
        );
        assert_eq!(
            RegimeDetector::volatility_regime_from_percentile(80.0),
            VolatilityRegime::HighVol
        );
    }
}

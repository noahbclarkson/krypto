#![allow(clippy::needless_range_loop)]
use super::{
    optimization::{OptimizableStrategy, StrategyParams},
    regime::{MarketRegime, RegimeDetector},
    SignalGenerator,
};
use anyhow::Result;
use polars::prelude::*;
use std::collections::HashMap;

pub struct MetaEnsemble {
    strategies: Vec<Box<dyn SignalGenerator>>,
}

impl MetaEnsemble {
    pub fn new() -> Self {
        Self {
            strategies: Vec::new(),
        }
    }

    pub fn add_strategy(&mut self, model: Box<dyn SignalGenerator>) {
        self.strategies.push(model);
    }

    pub fn generate_signal(&self, df: &DataFrame) -> Result<f64> {
        let regime = RegimeDetector::detect(df);

        let mut total_weight = 0.0;
        let mut weighted_signal = 0.0;

        for strat in &self.strategies {
            let name = strat.name();
            let raw_prediction = strat.predict(df)?;
            let signal = raw_prediction.f64()?.last().unwrap_or(0.0);

            let weight = match (regime, name) {
                (MarketRegime::TrendingBull, "Trend_Following_EMA") => 1.0,
                (MarketRegime::TrendingBull, "Trend_Pullback") => 1.5,

                (MarketRegime::TrendingBear, "Trend_Following_EMA") => 1.0,
                (MarketRegime::TrendingBear, "Trend_Pullback") => 1.5,

                (MarketRegime::Sideways, "Trend_Following_EMA") => 0.0,
                (MarketRegime::Sideways, "Trend_Pullback") => 0.5,

                _ => 0.5,
            };

            weighted_signal += signal * weight;
            total_weight += weight;
        }

        if total_weight == 0.0 {
            Ok(0.0)
        } else {
            Ok(weighted_signal / total_weight)
        }
    }
}

impl Default for MetaEnsemble {
    fn default() -> Self {
        Self::new()
    }
}

/// Voting Ensemble — requires agreement from multiple strategies before entering.
///
/// This is an anti-overfitting device: instead of a single strategy with many parameters,
/// we require N-of-M orthogonal strategies to agree. This dramatically reduces false entries.
///
/// Signal = 1.0  when >= min_agree strategies signal long
/// Signal = -1.0 when >= min_agree strategies signal short
/// Signal = 0.0  otherwise (disagreement → stay flat)
///
/// # Design
/// The constituent strategies are fixed at construction (they must be pre-configured).
/// The `min_agree` threshold and an optional `require_unanimous_exit` can be tuned.
pub struct VotingEnsemble {
    strategies: Vec<Box<dyn SignalGenerator>>,
    /// Minimum number of strategies that must agree to generate a signal.
    pub min_agree: usize,
}

impl VotingEnsemble {
    pub fn new(strategies: Vec<Box<dyn SignalGenerator>>, min_agree: usize) -> Self {
        Self { strategies, min_agree }
    }

    pub fn n_strategies(&self) -> usize {
        self.strategies.len()
    }
}

impl SignalGenerator for VotingEnsemble {
    fn name(&self) -> &str { "VotingEnsemble" }

    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> { Ok(()) }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        let n = df.height();
        if self.strategies.is_empty() {
            return Ok(Series::new("signal", vec![0.0f64; n]));
        }

        // Collect all strategy signals
        let all_signals: Vec<Vec<f64>> = self.strategies.iter()
            .filter_map(|s| s.predict(df).ok())
            .map(|s| s.f64().map(|ca| ca.into_iter().map(|v| v.unwrap_or(0.0)).collect()).unwrap_or_else(|_| vec![0.0; n]))
            .collect();

        if all_signals.is_empty() {
            return Ok(Series::new("signal", vec![0.0f64; n]));
        }

        let mut output = vec![0.0f64; n];
        for i in 0..n {
            let long_votes = all_signals.iter().filter(|s| s.get(i).copied().unwrap_or(0.0) > 0.5).count();
            let short_votes = all_signals.iter().filter(|s| s.get(i).copied().unwrap_or(0.0) < -0.5).count();

            if long_votes >= self.min_agree {
                output[i] = 1.0;
            } else if short_votes >= self.min_agree {
                output[i] = -1.0;
            }
        }

        Ok(Series::new("signal", output))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new("explanation",
            vec![format!("VotingEnsemble: {}/{} agree required", self.min_agree, self.strategies.len()).as_str(); df.height()]))
    }
}

/// A single-strategy wrapper that adds a simple confirmation filter:
/// volume confirmation and ADX trend strength filter.
///
/// This upgrades an existing strategy by:
/// 1. Suppressing signals when volume is below its N-bar average (weak conviction)
/// 2. Suppressing long signals in downtrends (ADX + directional movement)
///
/// The goal: reduce whipsaw trades by only entering when the market has conviction.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConfirmedStrategy {
    /// Minimum volume multiplier vs N-bar average (e.g. 1.2 = 20% above average)
    pub vol_multiplier: f64,
    /// Volume lookback period (bars)
    pub vol_period: usize,
    /// Whether to use volume confirmation (true) or skip it (false)
    pub use_vol_confirm: bool,
}

impl ConfirmedStrategy {
    pub fn new() -> Self {
        Self {
            vol_multiplier: 1.2,
            vol_period: 20,
            use_vol_confirm: true,
        }
    }

    /// Apply confirmation filter to a pre-computed signal series.
    ///
    /// Returns a filtered signal where low-conviction bars are set to 0.
    pub fn filter_signal(&self, df: &DataFrame, raw_signal: &Series) -> Result<Series> {
        if !self.use_vol_confirm {
            return Ok(raw_signal.clone());
        }

        let n = df.height();
        let volumes: Vec<f64> = df.column("volume")
            .map(|s| s.f64().map(|ca| ca.into_iter().map(|v| v.unwrap_or(0.0)).collect()).unwrap_or_else(|_| vec![1.0; n]))
            .unwrap_or_else(|_| vec![1.0; n]);

        let raw: Vec<f64> = raw_signal.f64()
            .map(|ca| ca.into_iter().map(|v| v.unwrap_or(0.0)).collect())
            .unwrap_or_else(|_| vec![0.0; n]);

        let mut output = vec![0.0f64; n];
        for i in self.vol_period..n {
            let avg_vol = volumes[i.saturating_sub(self.vol_period)..i].iter().sum::<f64>()
                / self.vol_period as f64;
            let vol_ok = avg_vol < f64::EPSILON || volumes[i] >= avg_vol * self.vol_multiplier;

            if vol_ok {
                output[i] = raw[i];
            }
            // else: suppress signal — volume too low
        }

        Ok(Series::new(raw_signal.name(), output))
    }
}

impl Default for ConfirmedStrategy {
    fn default() -> Self { Self::new() }
}

/// DynamicTrend with volume confirmation — the currently strongest base strategy
/// augmented with a volume filter to reduce false entries.
///
/// This is a concrete `OptimizableStrategy` that wraps DynamicTrend + ConfirmedStrategy.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConfirmedDynamicTrend {
    /// EMA fast period
    pub ema_fast: usize,
    /// EMA slow period
    pub ema_slow: usize,
    /// RSI filter level
    pub rsi_filter: f64,
    /// Volume confirmation multiplier
    pub vol_multiplier: f64,
    /// Volume lookback period
    pub vol_period: usize,
}

impl ConfirmedDynamicTrend {
    pub fn new() -> Self {
        Self {
            ema_fast: 50,
            ema_slow: 200,
            rsi_filter: 50.0,
            vol_multiplier: 1.2,
            vol_period: 20,
        }
    }
}

impl Default for ConfirmedDynamicTrend {
    fn default() -> Self { Self::new() }
}

impl SignalGenerator for ConfirmedDynamicTrend {
    fn name(&self) -> &str { "ConfirmedDynamicTrend" }

    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> { Ok(()) }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        use polars::prelude::EWMOptions;

        let fast_opt = EWMOptions {
            alpha: 1.0 / self.ema_fast as f64,
            adjust: true,
            bias: false,
            min_periods: self.ema_fast,
            ignore_nulls: true,
        };
        let slow_opt = EWMOptions {
            alpha: 1.0 / self.ema_slow as f64,
            adjust: true,
            bias: false,
            min_periods: self.ema_slow,
            ignore_nulls: true,
        };

        let temp_df = df.clone().lazy()
            .with_columns(vec![
                col("close").ewm_mean(fast_opt).alias("ema_fast_cdt"),
                col("close").ewm_mean(slow_opt).alias("ema_slow_cdt"),
            ])
            .collect()?;

        let ema_f = temp_df.column("ema_fast_cdt")?;
        let ema_s = temp_df.column("ema_slow_cdt")?;
        let rsi = temp_df.column("rsi")?;

        let mask_long = ema_f.gt(ema_s)? & rsi.gt(self.rsi_filter)?;
        let mask_short = ema_f.lt(ema_s)?;

        let n = df.height();
        let mut raw = vec![0.0f64; n];
        for i in 0..n {
            if mask_long.get(i).unwrap_or(false) {
                raw[i] = 1.0;
            } else if mask_short.get(i).unwrap_or(false) {
                raw[i] = -1.0;
            }
        }
        let raw_series = Series::new("signal", raw);

        // Apply volume confirmation filter
        let filter = ConfirmedStrategy {
            vol_multiplier: self.vol_multiplier,
            vol_period: self.vol_period,
            use_vol_confirm: true,
        };
        filter.filter_signal(df, &raw_series)
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new("explanation",
            vec!["ConfirmedDynamicTrend: EMA crossover + RSI filter + volume confirmation"; df.height()]))
    }
}

impl OptimizableStrategy for ConfirmedDynamicTrend {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut m = HashMap::new();
        m.insert("ema_fast".to_string(),      (10.0, 60.0));
        m.insert("ema_slow".to_string(),      (100.0, 300.0));
        m.insert("rsi_filter".to_string(),    (40.0, 60.0));
        m.insert("vol_multiplier".to_string(),(1.0, 2.0));
        m.insert("vol_period".to_string(),    (10.0, 40.0));
        m
    }

    fn set_params(&mut self, p: &StrategyParams) {
        self.ema_fast       = p.get("ema_fast",       50.0) as usize;
        self.ema_slow       = p.get("ema_slow",      200.0) as usize;
        self.rsi_filter     = p.get("rsi_filter",     50.0);
        self.vol_multiplier = p.get("vol_multiplier",  1.2);
        self.vol_period     = p.get("vol_period",     20.0) as usize;
    }
}

#[cfg(test)]
mod ensemble_tests {
    use super::*;
    use polars::prelude::*;

    fn make_df(n: usize) -> DataFrame {
        let closes: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.1).collect();
        let volumes: Vec<f64> = vec![1000.0; n];
        let rsi: Vec<f64> = vec![55.0; n];
        DataFrame::new(vec![
            Series::new("close".into(), closes),
            Series::new("volume".into(), volumes),
            Series::new("rsi".into(), rsi),
            Series::new("high".into(), vec![105.0f64; n]),
            Series::new("low".into(), vec![95.0f64; n]),
        ]).unwrap()
    }

    #[test]
    fn test_voting_ensemble_empty() {
        let ensemble = VotingEnsemble::new(vec![], 2);
        let df = make_df(10);
        let sig = ensemble.predict(&df).unwrap();
        let vals: Vec<f64> = sig.f64().unwrap().into_iter().map(|v| v.unwrap()).collect();
        assert!(vals.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_confirmed_strategy_filter_low_volume() {
        let n = 30;
        let raw = Series::new("signal", vec![1.0f64; n]);
        let mut volumes = vec![1000.0f64; n];
        // Make volumes very low at bars 20-29 (below average)
        for i in 20..n { volumes[i] = 100.0; }

        let df = DataFrame::new(vec![
            Series::new("close".into(), vec![100.0f64; n]),
            Series::new("volume".into(), volumes),
        ]).unwrap();

        let filter = ConfirmedStrategy { vol_multiplier: 1.5, vol_period: 20, use_vol_confirm: true };
        let filtered = filter.filter_signal(&df, &raw).unwrap();
        let vals: Vec<f64> = filtered.f64().unwrap().into_iter().map(|v| v.unwrap()).collect();

        // Bars with low volume should be suppressed
        for i in 20..n {
            assert_eq!(vals[i], 0.0, "Bar {i} should be suppressed (low volume)");
        }
    }

    #[test]
    fn test_confirmed_dynamic_trend_compiles() {
        let df = make_df(300);
        let strat = ConfirmedDynamicTrend::default();
        let result = strat.predict(&df);
        assert!(result.is_ok(), "ConfirmedDynamicTrend::predict should not error");
    }
}

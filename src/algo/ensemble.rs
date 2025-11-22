use super::{
    regime::{MarketRegime, RegimeDetector},
    SignalGenerator,
};
use anyhow::Result;
use polars::prelude::*;

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

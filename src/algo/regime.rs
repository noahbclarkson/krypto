use polars::prelude::*;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MarketRegime {
    TrendingBull,
    TrendingBear,
    Sideways,
}

pub struct RegimeDetector;

impl RegimeDetector {
    pub fn detect(df: &DataFrame) -> MarketRegime {
        let ema_50 = df.column("ema_50").ok().and_then(|s| s.f64().ok());
        let ema_200 = df.column("ema_200").ok().and_then(|s| s.f64().ok());
        let bb_width = df.column("bb_width").ok().and_then(|s| s.f64().ok());

        if let (Some(e50), Some(e200), Some(bb)) = (ema_50, ema_200, bb_width) {
            let last_50 = e50.last().unwrap_or(0.0);
            let last_200 = e200.last().unwrap_or(0.0);
            let last_bb = bb.last().unwrap_or(0.0);

            if last_bb < 0.05 {
                return MarketRegime::Sideways;
            }

            if last_50 > last_200 {
                return MarketRegime::TrendingBull;
            } else {
                return MarketRegime::TrendingBear;
            }
        }
        MarketRegime::Sideways
    }
}

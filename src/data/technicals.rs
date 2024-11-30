use super::candlestick::Candlestick;

use ta::{indicators::*, Next};

#[derive(Debug, Clone)]
pub struct Technicals {
    technicals: Vec<Technical>,
}

#[derive(Debug, Clone)]
pub enum Technical {
    RSI(f64),
    FastStochastic(f64),
    SlowStochastic(f64),
    CCI(f64),
    MFI(f64),
    EfficiencyRatio(f64),
    PercentageChangeEMA(f64),
    VolumePercentageChangeEMA(f64),
    BollingerBands(f64),
    CandlestickRatio(f64),
}

impl Technicals {
    pub fn get_technicals(data: &[Candlestick], technical_names: Vec<String>) -> Vec<Self> {
        let mut rsi = RelativeStrengthIndex::default();
        let mut fast_stochastic = FastStochastic::default();
        let mut slow_stochastic = SlowStochastic::default();
        let mut cci = CommodityChannelIndex::default();
        let mut mfi = MoneyFlowIndex::default();
        let mut efficiency_ratio = EfficiencyRatio::default();
        let mut pc_ema = PercentageChangeEMA::default();
        let mut volume_pc_ema = PercentageChangeEMA::default();
        let mut bollinger_bands = BollingerBands::default();

        let mut result = Vec::new();

        for candle in data {
            let bb = bollinger_bands.next(candle.close);
            let bb_pct = (candle.close - bb.lower) / (bb.upper - bb.lower);
            let bb_pct = match bb_pct.is_nan() || bb_pct.is_infinite() {
                true => 0.0,
                false => bb_pct,
            };
            let rsi = rsi.next(candle);
            let fast_stochastic = fast_stochastic.next(candle);
            let slow_stochastic = slow_stochastic.next(candle);
            let cci = cci.next(candle);
            let mfi = mfi.next(candle);
            let efficiency_ratio = efficiency_ratio.next(candle);
            let percentage_change_ema = pc_ema.next(candle.close);
            let volume_percentage_change_ema = volume_pc_ema.next(candle.volume);
            let candlestick_ratio = candlestick_ratio(candle);
            let mut technicals = Vec::new();
            for name in &technical_names {
                let technical = Technical::from_name(name, match name.as_str() {
                    "RSI" => rsi,
                    "Fast Stochastic" => fast_stochastic,
                    "Slow Stochastic" => slow_stochastic,
                    "CCI" => cci,
                    "MFI" => mfi,
                    "Efficiency Ratio" => efficiency_ratio,
                    "Percentage Change EMA" => percentage_change_ema,
                    "Volume Percentage Change EMA" => volume_percentage_change_ema,
                    "Bollinger Bands" => bb_pct,
                    "Candlestick Ratio" => candlestick_ratio,
                    _ => panic!("Unknown technical name: {}", name),
                });
                technicals.push(technical);
            }
            result.push(Technicals { technicals });
        }
        result
    }

    pub fn as_array(&self) -> Vec<f64> {
        self.technicals.iter().map(|t| t.value()).collect()
    }
}

impl Technical {
    pub fn value(&self) -> f64 {
        match self {
            Technical::RSI(value) => *value,
            Technical::FastStochastic(value) => *value,
            Technical::SlowStochastic(value) => *value,
            Technical::CCI(value) => *value,
            Technical::MFI(value) => *value,
            Technical::EfficiencyRatio(value) => *value,
            Technical::PercentageChangeEMA(value) => *value,
            Technical::VolumePercentageChangeEMA(value) => *value,
            Technical::BollingerBands(value) => *value,
            Technical::CandlestickRatio(value) => *value,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Technical::RSI(_) => "RSI".to_string(),
            Technical::FastStochastic(_) => "Fast Stochastic".to_string(),
            Technical::SlowStochastic(_) => "Slow Stochastic".to_string(),
            Technical::CCI(_) => "CCI".to_string(),
            Technical::MFI(_) => "MFI".to_string(),
            Technical::EfficiencyRatio(_) => "Efficiency Ratio".to_string(),
            Technical::PercentageChangeEMA(_) => "Percentage Change EMA".to_string(),
            Technical::VolumePercentageChangeEMA(_) => "Volume Percentage Change EMA".to_string(),
            Technical::BollingerBands(_) => "Bollinger Bands".to_string(),
            Technical::CandlestickRatio(_) => "Candlestick Ratio".to_string(),
        }
    }

    pub fn from_name(name: &str, value: f64) -> Self {
        match name {
            "RSI" => Technical::RSI(value),
            "Fast Stochastic" => Technical::FastStochastic(value),
            "Slow Stochastic" => Technical::SlowStochastic(value),
            "CCI" => Technical::CCI(value),
            "MFI" => Technical::MFI(value),
            "Efficiency Ratio" => Technical::EfficiencyRatio(value),
            "Percentage Change EMA" => Technical::PercentageChangeEMA(value),
            "Volume Percentage Change EMA" => Technical::VolumePercentageChangeEMA(value),
            "Bollinger Bands" => Technical::BollingerBands(value),
            "Candlestick Ratio" => Technical::CandlestickRatio(value),
            _ => panic!("Unknown technical name: {}", name),
        }
    }
}


pub struct PercentageChangeEMA {
    pub period: usize,
    pub ema: ExponentialMovingAverage,
    last: Option<f64>,
}

impl Default for PercentageChangeEMA {
    fn default() -> Self {
        PercentageChangeEMA {
            period: 14,
            ema: ExponentialMovingAverage::default(),
            last: None,
        }
    }
}

impl Next<f64> for PercentageChangeEMA {
    type Output = f64;

    fn next(&mut self, close: f64) -> Self::Output {
        if close.is_nan() || close.is_infinite() {
            return self.ema.next(0.0);
        }
        let value = match self.last {
            Some(last) => (close - last) / last,
            None => 0.0,
        };
        self.last = Some(close);
        self.ema.next(value)
    }
}

impl PercentageChangeEMA {
    pub fn new(period: usize) -> Self {
        PercentageChangeEMA {
            period,
            ema: ExponentialMovingAverage::new(period).unwrap(),
            last: None,
        }
    }
}

/**
Calculates the candlestick ratio for a given candlestick.

The formula is: tanh((upper_wick / body) - (lower_wick / body))

## Arguments
   - `candle`: A Candlestick struct.

## Returns
The candlestick ratio.
*/
fn candlestick_ratio(candle: &Candlestick) -> f64 {
    let top = candle.close.max(candle.open);
    let bottom = candle.close.min(candle.open);
    let upper_wick = candle.high - top;
    let lower_wick = bottom - candle.low;
    let body = top - bottom;
    if body == 0.0 {
        return 0.0;
    }
    let ratio = (upper_wick / body) - (lower_wick / body);
    let result = ratio.tanh();
    match result.is_nan() || result.is_infinite() {
        true => 0.0,
        false => result,
    }
}

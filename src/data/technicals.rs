use super::candlestick::Candlestick;

use ta::{indicators::*, Next};

pub const TECHNICAL_COUNT: usize = 10;

#[derive(Debug, Clone)]
pub struct Technicals {
    rsi: f64,
    fast_stochastic: f64,
    slow_stochastic: f64,
    cci: f64,
    mfi: f64,
    efficiency_ratio: f64,
    percentage_change_ema: f64,
    volume_percentage_change_ema: f64,
    bb_pct: f64,
    candlestick_ratio: f64,
}

impl Technicals {
    pub fn get_technicals(data: &[Candlestick]) -> Vec<Self> {
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
            let technicals = Self {
                rsi: rsi.next(candle),
                fast_stochastic: fast_stochastic.next(candle),
                slow_stochastic: slow_stochastic.next(candle),
                cci: cci.next(candle),
                mfi: mfi.next(candle),
                efficiency_ratio: efficiency_ratio.next(candle),
                percentage_change_ema: pc_ema.next(candle.close),
                volume_percentage_change_ema: volume_pc_ema.next(candle.volume),
                bb_pct: (candle.close - bb.lower) / (bb.upper - bb.lower),
                candlestick_ratio: candlestick_ratio(candle),
            };
            result.push(technicals);
        }
        result
    }

    pub fn as_array(&self) -> [f64; TECHNICAL_COUNT] {
        [
            self.rsi,
            self.fast_stochastic,
            self.slow_stochastic,
            self.cci,
            self.mfi,
            self.efficiency_ratio,
            self.percentage_change_ema,
            self.volume_percentage_change_ema,
            self.bb_pct,
            self.candlestick_ratio,
        ]
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
    ratio.tanh()
}

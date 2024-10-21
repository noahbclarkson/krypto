use ta::{indicators::*, Next};

use crate::{candlestick::Candlestick, dataset::DataArray, KryptoError};

const TECHNICALS_SIZE: usize = 8;

#[derive(Debug, Clone, Default)]
pub struct Technicals {
    rsi: Option<f64>,
    fast_stochastic: Option<f64>,
    slow_stochastic: Option<f64>,
    cci: Option<f64>,
    mfi: Option<f64>,
    efficiency_ratio: Option<f64>,
    pc_ema: Option<f64>,
    volume_pc_ema: Option<f64>,
}

impl Technicals {
    pub fn get_array(&self) -> [f64; TECHNICALS_SIZE] {
        [
            self.rsi.unwrap_or_default(),
            self.fast_stochastic.unwrap_or_default(),
            self.slow_stochastic.unwrap_or_default(),
            self.cci.unwrap_or_default(),
            self.mfi.unwrap_or_default(),
            self.efficiency_ratio.unwrap_or_default(),
            self.pc_ema.unwrap_or_default(),
            self.volume_pc_ema.unwrap_or_default(),
            // self.macd_pc_ema.unwrap_or_default(),
            // self.ppo_pc_ema.unwrap_or_default(),
        ]
    }

    pub fn get_headers() -> [&'static str; TECHNICALS_SIZE] {
        [
            "rsi",
            "fast_stochastic",
            "slow_stochastic",
            "cci",
            "mfi",
            "efficiency_ratio",
            "pc_ema",
            "volume_pc_ema",
            // "macd_pc_ema",
            // "ppo_pc_ema",
        ]
    }
}

pub fn compute_technicals(data: &mut DataArray) {
    let data = &mut data.data;
    let mut rsi = RelativeStrengthIndex::default();
    let mut fast_stochastic = FastStochastic::default();
    let mut slow_stochastic = SlowStochastic::default();
    let mut cci = CommodityChannelIndex::default();
    let mut mfi = MoneyFlowIndex::default();
    let mut efficiency_ratio = EfficiencyRatio::default();
    let mut pc_ema = PercentageChangeEMA::default();
    let mut volume_pc_ema = PercentageChangeEMA::default();

    for candle in data.iter_mut() {
        let rsi_value = rsi.next(&*candle);
        let fast_stochastic_value = fast_stochastic.next(&*candle);
        let slow_stochastic_value = slow_stochastic.next(&*candle);
        let cci_value = cci.next(&*candle);
        let mfi_value = mfi.next(&*candle);
        let efficiency_ratio_value = efficiency_ratio.next(&*candle);
        let pc_ema_value = pc_ema.next(&*candle);
        let volume_pc_ema_value = volume_pc_ema.next(candle.volume);

        let technicals = Technicals {
            rsi: Some(rsi_value),
            fast_stochastic: Some(fast_stochastic_value),
            slow_stochastic: Some(slow_stochastic_value),
            cci: Some(cci_value),
            mfi: Some(mfi_value),
            efficiency_ratio: Some(efficiency_ratio_value),
            pc_ema: Some(pc_ema_value),
            volume_pc_ema: Some(volume_pc_ema_value),
        };
        candle.technicals = technicals;
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

impl Next<&Candlestick> for PercentageChangeEMA {
    type Output = f64;

    fn next(&mut self, candle: &Candlestick) -> Self::Output {
        self.next(candle.close)
    }
}

impl PercentageChangeEMA {
    pub fn new(period: usize) -> Result<Self, KryptoError> {
        Ok(PercentageChangeEMA {
            period,
            ema: ExponentialMovingAverage::new(period)?,
            last: None,
        })
    }
}

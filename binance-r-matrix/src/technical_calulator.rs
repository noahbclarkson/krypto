use binance::rest_model::KlineSummary;
use ta::{errors::TaError, indicators::*, DataItem, Next};

use crate::{errors::DataError, math::percentage_change};

const TECHNICALS: usize = 10;

pub(crate) struct TechnicalCalulator {
    stoch: SlowStochastic,
    rsi: RelativeStrengthIndex,
    cci: CommodityChannelIndex,
    mfi: MoneyFlowIndex,
    ppo: PercentagePriceOscillator,
    efficiency: EfficiencyRatio,
    pc_ema: ExponentialMovingAverage,
    volume_ema: ExponentialMovingAverage,
}

pub enum TechnicalType {
    PercentageChangeReal,
    PercentageChange,
    VolumeEma30,
    CandlestickRatio,
    StochasticOscillator,
    RelativeStrengthIndex,
    CommodityChannelIndex,
    MoneyFlowIndex,
    PercentagePriceOscillator,
    EfficiencyRatio,
    PercentageChangeEma,
}

impl TechnicalCalulator {
    pub fn new() -> Self {
        Self {
            stoch: SlowStochastic::default(),
            rsi: RelativeStrengthIndex::default(),
            cci: CommodityChannelIndex::default(),
            mfi: MoneyFlowIndex::default(),
            ppo: PercentagePriceOscillator::default(),
            efficiency: EfficiencyRatio::default(),
            pc_ema: ExponentialMovingAverage::default(),
            volume_ema: ExponentialMovingAverage::new(30).unwrap(),
        }
    }

    // pub(crate) fn calculate_technicals(
    //     klines: Vec<KlineSummary>,
    // ) -> Result<Vec<Box<[f64]>>, DataError> {
    //     let mut previous_close = klines[0].close;
    //     for kline in klines.iter() {
    //         Self::process_candle(kline, &mut previous_close)?;
    //     }
    //     Ok(())
    // }

    // #[inline]
    // fn process_candle(
    //     candle: &KlineSummary,
    //     previous_close: &mut f64,
    // ) -> Result<Vec<f64>, DataError> {
    //     let pc = normalized_pc(*previous_close, candle.close);
    //     let mut technicals = Vec::with_capacity(TECHNICALS);

    //     Ok(())
    // }
}

#[inline(always)]
fn normalized_pc(previous: f64, current: f64) -> f64 {
    let pc = percentage_change(previous, current);
    match pc.is_nan() {
        true => 0.0,
        false => pc,
    }
}

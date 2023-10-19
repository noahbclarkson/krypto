use binance::rest_model::KlineSummary;
use ta::{errors::TaError, indicators::*, DataItem, Next};

use crate::{errors::DataError, math::{percentage_change, cr_ratio}, BinanceDataType};

pub const TECHNICALS: usize = 11;

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

    pub(crate) fn calculate_technicals(
        &mut self,
        klines: &Box<[KlineSummary]>,
    ) -> Result<Vec<Box<[f64]>>, DataError> {
        let mut previous_close = klines[0].close;
        let mut results = Vec::with_capacity(klines.len());
        for kline in klines.iter() {
            let technicals = self.process_candle(kline, &mut previous_close)?;
            results.push(technicals.into_boxed_slice());
        }
        Ok(results)
    }

    #[inline]
    fn process_candle(
        &mut self,
        candle: &KlineSummary,
        previous_close: &mut f64,
    ) -> Result<Vec<f64>, DataError> {
        let pc = normalized_pc(*previous_close, candle.close);
        *previous_close = candle.close;
        let mut technicals = [0.0; TECHNICALS];
        let item = &candle_to_item(candle)?;
        technicals[BinanceDataType::PercentageChangeReal as usize] = pc;
        technicals[BinanceDataType::PercentageChange as usize] = pc;
        technicals[BinanceDataType::VolumeEma30 as usize] = self.volume_ema.next(candle.volume);
        technicals[BinanceDataType::CandlestickRatio as usize] = cr_ratio(candle);
        technicals[BinanceDataType::StochasticOscillator as usize] = self.stoch.next(item);
        technicals[BinanceDataType::RelativeStrengthIndex as usize] = self.rsi.next(item);
        technicals[BinanceDataType::CommodityChannelIndex as usize] = self.cci.next(item);
        technicals[BinanceDataType::MoneyFlowIndex as usize] = self.mfi.next(item);
        technicals[BinanceDataType::PercentagePriceOscillator as usize] = self.ppo.next(item).ppo;
        technicals[BinanceDataType::EfficiencyRatio as usize] = self.efficiency.next(item);
        technicals[BinanceDataType::PercentageChangeEma as usize] = self.pc_ema.next(pc);
        Ok(technicals.to_vec())
    }
}

#[inline(always)]
fn normalized_pc(previous: f64, current: f64) -> f64 {
    let pc = percentage_change(previous, current);
    match pc.is_nan() {
        true => 0.0,
        false => pc,
    }
}

fn candle_to_item(kline: &KlineSummary) -> Result<DataItem, TaError> {
    DataItem::builder()
            .open(kline.open)
            .high(kline.high)
            .low(kline.low)
            .close(kline.close)
            .volume(kline.volume)
            .build()
}

use ta::{errors::TaError, indicators::*, DataItem, Next};

use crate::{
    candle::{Candle, TechnicalType, TECHNICAL_COUNT},
    math::{cr_ratio, percentage_change},
    ticker_data::TickerData,
};

pub struct TechnicalCalculator {
    stoch: SlowStochastic,
    rsi: RelativeStrengthIndex,
    cci: CommodityChannelIndex,
    mfi: MoneyFlowIndex,
    ppo: PercentagePriceOscillator,
    efficiency: EfficiencyRatio,
    ema: ExponentialMovingAverage,
    volume_ema: ExponentialMovingAverage,
    means: Option<Vec<[f64; TECHNICAL_COUNT]>>,
    standard_deviations: Option<Vec<[f64; TECHNICAL_COUNT]>>,
}

impl TechnicalCalculator {
    pub fn new() -> Self {
        Self {
            stoch: SlowStochastic::default(),
            rsi: RelativeStrengthIndex::default(),
            cci: CommodityChannelIndex::default(),
            mfi: MoneyFlowIndex::default(),
            ppo: PercentagePriceOscillator::default(),
            efficiency: EfficiencyRatio::default(),
            ema: ExponentialMovingAverage::default(),
            volume_ema: ExponentialMovingAverage::new(30).unwrap(),
            means: None,
            standard_deviations: None,
        }
    }

    pub fn calculate_technicals(
        &mut self,
        mut candles: Box<[TickerData]>,
    ) -> Result<Box<[TickerData]>, TaError> {
        for ticker in candles.iter_mut() {
            self.process_ticker(ticker)?;
        }

        self.calculate_means(&candles);
        self.calculate_stddevs(&candles);
        self.normalize(&mut candles);
        self.remove_technical_nan(&mut candles);

        Ok(candles)
    }

    fn process_ticker(&mut self, ticker: &mut TickerData) -> Result<(), TaError> {
        let mut previous_close = *ticker.candles()[0].close();
        for candle in ticker.candles_mut().iter_mut() {
            previous_close = self.process_candle(candle, previous_close)?;
        }
        Ok(())
    }

    #[inline]
    fn process_candle(&mut self, candle: &mut Candle, previous_close: f64) -> Result<f64, TaError> {
        let p_change = normalize_pc(percentage_change(previous_close, *candle.close()));
        let volume = *candle.volume();
        candle.set_percentage_change(p_change);
        let item = candle.to_data_item()?;

        let technicals = candle.technicals_mut();
        self.populate_technicals(technicals.as_mut(), &item, volume, p_change);
        Ok(*candle.close())
    }

    #[inline(always)]
    fn populate_technicals(
        &mut self,
        technicals: &mut [f64],
        item: &DataItem,
        volume: f64,
        p_change: f64,
    ) {
        technicals[TechnicalType::PercentageChange as usize] = p_change;
        technicals[TechnicalType::VolumeEma as usize] = self.volume_ema.next(volume);
        technicals[TechnicalType::CandlestickRatio as usize] = cr_ratio(item);
        technicals[TechnicalType::StochasticOscillator as usize] = self.stoch.next(item);
        technicals[TechnicalType::RelativeStrengthIndex as usize] = self.rsi.next(item);
        technicals[TechnicalType::CommodityChannelIndex as usize] = self.cci.next(item);
        technicals[TechnicalType::MoneyFlowIndex as usize] = self.mfi.next(item);
        technicals[TechnicalType::PPOscillator as usize] = self.ppo.next(item).ppo;
        technicals[TechnicalType::EfficiencyRatio as usize] = self.efficiency.next(item);
        technicals[TechnicalType::PCEMA as usize] = self.ema.next(p_change);
    }

    fn calculate_means(&mut self, candles: &[TickerData]) {
        let mut means = Vec::with_capacity(candles.len());
        let length = candles[0].candles().len();
        for ticker in candles.iter() {
            let mut ticker_means = [0.0; TECHNICAL_COUNT];
            for candle in ticker.candles().iter() {
                let technicals = candle.technicals();
                for i in 0..TECHNICAL_COUNT {
                    ticker_means[i] += technicals[i];
                }
            }
            for i in 0..TECHNICAL_COUNT {
                ticker_means[i] /= length as f64;
            }
            means.push(ticker_means);
        }
        self.means = Some(means);
    }

    fn calculate_stddevs(&mut self, candles: &[TickerData]) {
        let mut stddevs = Vec::with_capacity(candles.len());
        let length = candles[0].candles().len();
        for (i, ticker_data) in candles.iter().enumerate() {
            let mut ticker_stddevs = [0.0; TECHNICAL_COUNT];
            for candle in ticker_data.candles().iter() {
                let techs = candle.technicals();
                let means = self.means.as_ref().unwrap()[i];
                for j in 0..TECHNICAL_COUNT {
                    ticker_stddevs[j] += (techs[j] - means[j]).powi(2);
                }
            }
            for j in 0..TECHNICAL_COUNT {
                ticker_stddevs[j] = (ticker_stddevs[j] / length as f64).sqrt();
            }
            stddevs.push(ticker_stddevs);
        }
        self.standard_deviations = Some(stddevs);
    }

    fn normalize(&mut self, candles: &mut [TickerData]) {
        let means = self.means.as_ref().unwrap();
        let stddevs = self.standard_deviations.as_ref().unwrap();
        for (i, ticker_data) in candles.iter_mut().enumerate() {
            for candle in ticker_data.candles_mut().iter_mut() {
                let techs = candle.technicals_mut();
                for j in 0..TECHNICAL_COUNT {
                    techs[j] = (techs[j] - means[i][j]) / stddevs[i][j];
                }
            }
        }
    }

    fn remove_technical_nan(&mut self, candles: &mut [TickerData]) {
        for ticker in candles.iter_mut() {
            for candle in ticker.candles_mut().iter_mut() {
                let techs = candle.technicals_mut();
                for j in 0..TECHNICAL_COUNT {
                    if techs[j].is_nan() {
                        techs[j] = 0.0;
                    }
                }
            }
        }
    }
}

#[inline(always)]
fn normalize_pc(pc: f64) -> f64 {
    if pc.is_nan() {
        return 0.0;
    }
    pc / 100.0
}

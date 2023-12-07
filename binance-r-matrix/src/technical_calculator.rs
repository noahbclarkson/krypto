use binance::rest_model::KlineSummary;
use statrs::statistics::Statistics as _;
use ta::{errors::TaError, indicators::*, DataItem, Next};

use crate::{
    error::BinanceDataError,
    math::{cr_ratio, percentage_change},
};

pub const TECHNICALS: usize = 10;

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
        klines: &[KlineSummary],
    ) -> Result<Vec<Vec<f64>>, BinanceDataError> {
        let mut previous_close = klines[0].close;
        let mut results = Vec::with_capacity(klines.len());
        for kline in klines.iter() {
            let technicals = self.process_candle(kline, &mut previous_close)?;
            results.push(technicals);
        }
        let results = normalize(results);
        Ok(results)
    }

    #[inline]
    fn process_candle(
        &mut self,
        candle: &KlineSummary,
        previous_close: &mut f64,
    ) -> Result<Vec<f64>, BinanceDataError> {
        let pc = normalized_pc(*previous_close, candle.close);
        *previous_close = candle.close;
        let mut technicals = [0.0; TECHNICALS];
        let item = &candle_to_item(candle)?;
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

fn normalize(values: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    let mut normalized_values = vec![vec![0.0; values[0].len()]; values.len()];

    // Iterate over each technical indicator
    for i in 0..TECHNICALS {
        let mut technicals: Vec<f64> = values.iter().map(|v| v[i]).collect();

        // Calculate mean and standard deviation for each technical indicator
        let mean = technicals.clone().mean();
        let stddev = technicals.clone().std_dev();

        // Normalize values for this technical
        for (j, value) in technicals.iter_mut().enumerate() {
            normalized_values[j][i] = if stddev == 0.0 {
                0.0
            } else {
                (*value - mean) / stddev
            };
        }
    }

    normalized_values
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BinanceDataType {
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

impl BinanceDataType {
    pub fn from_usize(i: usize) -> Self {
        match i {
            0 => BinanceDataType::PercentageChange,
            1 => BinanceDataType::VolumeEma30,
            2 => BinanceDataType::CandlestickRatio,
            3 => BinanceDataType::StochasticOscillator,
            4 => BinanceDataType::RelativeStrengthIndex,
            5 => BinanceDataType::CommodityChannelIndex,
            6 => BinanceDataType::MoneyFlowIndex,
            7 => BinanceDataType::PercentagePriceOscillator,
            8 => BinanceDataType::EfficiencyRatio,
            9 => BinanceDataType::PercentageChangeEma,
            _ => panic!("Invalid index"),
        }
    }

    pub fn get_feature_names() -> Vec<&'static str> {
        vec![
            "Percentage Change",
            "Volume EMA 30",
            "Candlestick Ratio",
            "Stochastic Oscillator",
            "Relative Strength Index",
            "Commodity Channel Index",
            "Money Flow Index",
            "Percentage Price Oscillator",
            "Efficiency Ratio",
            "Percentage Change EMA",
        ]
    }
}

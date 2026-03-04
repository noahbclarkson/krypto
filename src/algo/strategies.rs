use crate::algo::optimization::{OptimizableStrategy, StrategyParams};
use crate::algo::SignalGenerator;
use anyhow::Result;
use polars::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynamicTrend {
    pub ema_fast: usize,
    pub ema_slow: usize,
    pub rsi_filter: f64,
}

impl DynamicTrend {
    pub fn new() -> Self {
        Self {
            ema_fast: 50,
            ema_slow: 200,
            rsi_filter: 50.0,
        }
    }
}

impl Default for DynamicTrend {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for DynamicTrend {
    fn name(&self) -> &str {
        "Dynamic_Trend"
    }

    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
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

        let temp_df = df
            .clone()
            .lazy()
            .with_columns(vec![
                col("close").ewm_mean(fast_opt).alias("ema_fast_dyn"),
                col("close").ewm_mean(slow_opt).alias("ema_slow_dyn"),
            ])
            .collect()?;

        let ema_f = temp_df.column("ema_fast_dyn")?;
        let ema_s = temp_df.column("ema_slow_dyn")?;
        let rsi = temp_df.column("rsi")?;

        let mask_long = ema_f.gt(ema_s)? & rsi.gt(self.rsi_filter)?;
        let mask_short = ema_f.lt(ema_s)?;

        let mut signals = vec![0.0; ema_f.len()];
        for i in 0..ema_f.len() {
            if mask_long.get(i).unwrap_or(false) {
                signals[i] = 1.0;
            } else if mask_short.get(i).unwrap_or(false) {
                signals[i] = -1.0;
            }
        }
        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
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

        let temp_df = df
            .clone()
            .lazy()
            .with_columns(vec![
                col("close").ewm_mean(fast_opt).alias("ema_fast_dyn"),
                col("close").ewm_mean(slow_opt).alias("ema_slow_dyn"),
            ])
            .collect()?;

        let ema_f = temp_df.column("ema_fast_dyn")?.f64()?;
        let ema_s = temp_df.column("ema_slow_dyn")?.f64()?;
        let rsi = temp_df.column("rsi")?.f64()?;

        let mut reasons = Vec::with_capacity(df.height());

        for i in 0..df.height() {
            let f = ema_f.get(i).unwrap_or(0.0);
            let s = ema_s.get(i).unwrap_or(0.0);
            let r = rsi.get(i).unwrap_or(50.0);

            if f > s && r > self.rsi_filter {
                reasons.push(format!(
                    "LONG: EMA_Fast({:.2}) > EMA_Slow({:.2}) AND RSI({:.1}) > {:.1}",
                    f, s, r, self.rsi_filter
                ));
            } else if f < s {
                reasons.push(format!("SHORT: EMA_Fast({f:.2}) < EMA_Slow({s:.2})"));
            } else {
                reasons.push(format!(
                    "FLAT: EMA_Fast({:.2}) > EMA_Slow({:.2}) but RSI({:.1}) <= {:.1}",
                    f, s, r, self.rsi_filter
                ));
            }
        }
        Ok(Series::new("explanation", reasons))
    }
}

impl OptimizableStrategy for DynamicTrend {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("ema_fast".to_string(), (10.0, 60.0));
        map.insert("ema_slow".to_string(), (100.0, 300.0));
        map.insert("rsi_filter".to_string(), (40.0, 60.0));
        map
    }

    fn set_params(&mut self, params: &StrategyParams) {
        self.ema_fast = params.get("ema_fast", 50.0) as usize;
        self.ema_slow = params.get("ema_slow", 200.0) as usize;
        self.rsi_filter = params.get("rsi_filter", 50.0);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelativeStrengthStrat {
    pub rs_ema_period: usize,
    pub rsi_entry: f64,
}

impl RelativeStrengthStrat {
    pub fn new() -> Self {
        Self {
            rs_ema_period: 50,
            rsi_entry: 30.0,
        }
    }
}

impl Default for RelativeStrengthStrat {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for RelativeStrengthStrat {
    fn name(&self) -> &str {
        "Relative_Strength"
    }

    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        if df.column("rs_ratio").is_err() {
            return Ok(Series::new("signal", vec![0.0; df.height()]));
        }

        let opt = EWMOptions {
            alpha: 1.0 / self.rs_ema_period as f64,
            adjust: true,
            bias: false,
            min_periods: self.rs_ema_period,
            ignore_nulls: true,
        };

        let temp_df = df
            .clone()
            .lazy()
            .with_column(col("rs_ratio").ewm_mean(opt).alias("rs_ema_dyn"))
            .collect()?;

        let rs_ratio = temp_df.column("rs_ratio")?;
        let rs_ema = temp_df.column("rs_ema_dyn")?;
        let rsi = temp_df.column("rsi")?;

        let mask_long = rs_ratio.gt(rs_ema)? & rsi.lt(self.rsi_entry)?;
        let mask_short = rs_ratio.lt(rs_ema)?;

        let mut signals = vec![0.0; temp_df.height()];
        for i in 0..temp_df.height() {
            if mask_long.get(i).unwrap_or(false) {
                signals[i] = 1.0;
            } else if mask_short.get(i).unwrap_or(false) {
                signals[i] = -1.0;
            }
        }
        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new(
            "explanation",
            vec!["RelativeStrength strategy signal"; df.height()],
        ))
    }
}

impl OptimizableStrategy for RelativeStrengthStrat {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("rs_ema_period".to_string(), (20.0, 100.0));
        map.insert("rsi_entry".to_string(), (20.0, 50.0));
        map
    }

    fn set_params(&mut self, params: &StrategyParams) {
        self.rs_ema_period = params.get("rs_ema_period", 50.0) as usize;
        self.rsi_entry = params.get("rsi_entry", 30.0);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BollingerReversion {
    pub bb_period: usize,
    pub bb_std: f64,
    pub rsi_filter: f64,
}

impl BollingerReversion {
    pub fn new() -> Self {
        Self {
            bb_period: 20,
            bb_std: 2.0,
            rsi_filter: 30.0,
        }
    }
}

impl Default for BollingerReversion {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for BollingerReversion {
    fn name(&self) -> &str {
        "Bollinger_Reversion"
    }

    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        let close = df.column("close")?.f64()?;
        let rsi = df.column("rsi")?.f64()?;

        let mut queue: VecDeque<f64> = VecDeque::with_capacity(self.bb_period + 1);
        let mut sum = 0.0;
        let mut sum_sq = 0.0;

        let mut upper = vec![f64::NAN; df.height()];
        let mut lower = vec![f64::NAN; df.height()];

        for i in 0..df.height() {
            let price = close.get(i).unwrap_or(0.0);
            queue.push_back(price);
            sum += price;
            sum_sq += price * price;

            if queue.len() > self.bb_period {
                if let Some(old) = queue.pop_front() {
                    sum -= old;
                    sum_sq -= old * old;
                }
            }

            if queue.len() == self.bb_period {
                let n = self.bb_period as f64;
                let mean = sum / n;
                let var = (sum_sq / n) - (mean * mean);
                let std = var.max(0.0).sqrt();

                upper[i] = mean + std * self.bb_std;
                lower[i] = mean - std * self.bb_std;
            }
        }

        let mut signals = vec![0.0; df.height()];
        for i in 0..df.height() {
            let c = close.get(i).unwrap_or(0.0);
            let l = lower[i];
            let u = upper[i];
            let r = rsi.get(i).unwrap_or(50.0);

            if l.is_finite() && c < l && r < self.rsi_filter {
                signals[i] = 1.0;
            } else if u.is_finite() && c > u {
                signals[i] = -1.0;
            }
        }

        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new(
            "explanation",
            vec!["BollingerReversion strategy signal"; df.height()],
        ))
    }
}

impl OptimizableStrategy for BollingerReversion {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("bb_period".to_string(), (10.0, 50.0));
        map.insert("bb_std".to_string(), (1.5, 3.0));
        map.insert("rsi_filter".to_string(), (20.0, 45.0));
        map
    }

    fn set_params(&mut self, params: &StrategyParams) {
        self.bb_period = params.get("bb_period", 20.0) as usize;
        self.bb_std = params.get("bb_std", 2.0);
        self.rsi_filter = params.get("rsi_filter", 30.0);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AtrBreakout {
    pub atr_mult: f64,
    pub rsi_filter: f64,
    pub trend_ema: usize,
}

impl AtrBreakout {
    pub fn new() -> Self {
        Self {
            atr_mult: 1.5,
            rsi_filter: 50.0,
            trend_ema: 50,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VolatilitySqueeze {
    pub bb_mult: f64,
    pub kc_mult: f64,
    pub period: usize,
}

impl VolatilitySqueeze {
    pub fn new() -> Self {
        Self {
            bb_mult: 2.0,
            kc_mult: 1.5,
            period: 20,
        }
    }
}

impl Default for VolatilitySqueeze {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for VolatilitySqueeze {
    fn name(&self) -> &str {
        "Volatility_Squeeze"
    }

    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        let close = df.column("close")?.f64()?;
        let atr = df.column("atr")?.f64()?;

        let mut queue: VecDeque<f64> = VecDeque::with_capacity(self.period + 1);
        let mut sum = 0.0;
        let mut sum_sq = 0.0;

        let mut signals = vec![0.0; df.height()];
        let mut in_squeeze = false;

        for i in 0..df.height() {
            let price = close.get(i).unwrap_or(0.0);
            queue.push_back(price);
            sum += price;
            sum_sq += price * price;

            if queue.len() > self.period {
                if let Some(old) = queue.pop_front() {
                    sum -= old;
                    sum_sq -= old * old;
                }
            }

            if queue.len() == self.period {
                let n = self.period as f64;
                let mean = sum / n;
                let var = (sum_sq / n) - (mean * mean);
                let std = var.max(0.0).sqrt();

                let bb_upper = mean + std * self.bb_mult;
                let bb_lower = mean - std * self.bb_mult;

                let atr_val = atr.get(i).unwrap_or(0.0);
                let kc_upper = mean + atr_val * self.kc_mult;
                let kc_lower = mean - atr_val * self.kc_mult;

                // Check squeeze condition: BB inside KC
                if bb_upper < kc_upper && bb_lower > kc_lower {
                    in_squeeze = true;
                } else if in_squeeze {
                    // breakout
                    let prev_price = if i > 0 {
                        close.get(i - 1).unwrap_or(price)
                    } else {
                        price
                    };

                    if bb_upper >= kc_upper && price > prev_price {
                        signals[i] = 1.0;
                        in_squeeze = false;
                    } else if bb_lower <= kc_lower && price < prev_price {
                        signals[i] = -1.0;
                        in_squeeze = false;
                    }
                }
            }
        }

        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new(
            "explanation",
            vec!["VolatilitySqueeze strategy signal"; df.height()],
        ))
    }
}

impl OptimizableStrategy for VolatilitySqueeze {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("bb_mult".to_string(), (1.8, 2.5));
        map.insert("kc_mult".to_string(), (1.3, 1.7));
        map
    }

    fn set_params(&mut self, params: &StrategyParams) {
        self.bb_mult = params.get("bb_mult", 2.0);
        self.kc_mult = params.get("kc_mult", 1.5);
        self.period = 20;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeadLagStrategy {
    pub min_bench_move: f64,
    pub max_self_move: f64,
    pub decay: f64,
}

impl LeadLagStrategy {
    pub fn new() -> Self {
        Self {
            min_bench_move: 0.015,
            max_self_move: 0.005,
            decay: 3.0,
        }
    }
}

impl Default for LeadLagStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for LeadLagStrategy {
    fn name(&self) -> &str {
        "Lead_Lag_Arb"
    }

    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        if df.column("bench_ret_lag1").is_err() {
            return Ok(Series::new("signal", vec![0.0; df.height()]));
        }

        let bench_ret = df.column("bench_ret_lag1")?.f64()?;
        let self_close = df.column("close")?;
        let self_close_ca = self_close.f64()?;
        let mut self_ret = vec![0.0; df.height()];
        for i in 1..df.height() {
            let prev = self_close_ca.get(i - 1).unwrap_or(0.0);
            let curr = self_close_ca.get(i).unwrap_or(prev);
            self_ret[i] = if prev.abs() > f64::EPSILON {
                (curr - prev) / prev
            } else {
                0.0
            };
        }
        let mut self_ret_shifted = vec![0.0; df.height()];
        for i in 1..df.height() {
            self_ret_shifted[i] = self_ret[i - 1];
        }

        let mut signals = vec![0.0; df.height()];
        let mut holding_period = 0;
        let mut position = 0.0;

        for i in 0..df.height() {
            let b_ret = bench_ret.get(i).unwrap_or(0.0);
            let s_ret = *self_ret_shifted.get(i).unwrap_or(&0.0);

            if position != 0.0 {
                holding_period += 1;
                if holding_period >= self.decay as i32 {
                    signals[i] = 0.0;
                    position = 0.0;
                    holding_period = 0;
                } else {
                    signals[i] = position;
                }
            }

            if position == 0.0 {
                if b_ret > self.min_bench_move && s_ret < self.max_self_move {
                    signals[i] = 1.0;
                    position = 1.0;
                    holding_period = 0;
                } else if b_ret < -self.min_bench_move && s_ret > -self.max_self_move {
                    signals[i] = -1.0;
                    position = -1.0;
                    holding_period = 0;
                }
            }
        }

        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new(
            "explanation",
            vec!["LeadLag strategy signal"; df.height()],
        ))
    }
}

impl OptimizableStrategy for LeadLagStrategy {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("min_bench_move".to_string(), (0.01, 0.04));
        map.insert("max_self_move".to_string(), (0.001, 0.01));
        map.insert("decay".to_string(), (1.0, 5.0));
        map
    }

    fn set_params(&mut self, params: &StrategyParams) {
        self.min_bench_move = params.get("min_bench_move", 0.015);
        self.max_self_move = params.get("max_self_move", 0.005);
        self.decay = params.get("decay", 3.0);
    }
}

impl Default for AtrBreakout {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for AtrBreakout {
    fn name(&self) -> &str {
        "ATR_Breakout"
    }

    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        // Requires atr + rsi from FeatureEngine
        if df.column("atr").is_err() || df.column("rsi").is_err() {
            return Ok(Series::new("signal", vec![0.0; df.height()]));
        }

        let opt = EWMOptions {
            alpha: 1.0 / self.trend_ema as f64,
            adjust: true,
            bias: false,
            min_periods: self.trend_ema,
            ignore_nulls: true,
        };

        let temp_df = df
            .clone()
            .lazy()
            .with_column(col("close").ewm_mean(opt).alias("trend_ma"))
            .collect()?;

        let close = temp_df.column("close")?.f64()?;
        let trend = temp_df.column("trend_ma")?.f64()?;
        let atr = temp_df.column("atr")?.f64()?;
        let rsi = temp_df.column("rsi")?.f64()?;

        let mut signals = vec![0.0; temp_df.height()];
        for i in 0..temp_df.height() {
            let c = close.get(i).unwrap_or(0.0);
            let t = trend.get(i).unwrap_or(0.0);
            let a = atr.get(i).unwrap_or(0.0);
            let r = rsi.get(i).unwrap_or(50.0);

            let upper = t + self.atr_mult * a;
            let lower = t - self.atr_mult * a;

            if c > upper && r > self.rsi_filter {
                signals[i] = 1.0;
            } else if c < lower && r < (100.0 - self.rsi_filter) {
                signals[i] = -1.0;
            }
        }

        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new(
            "explanation",
            vec!["AtrBreakout strategy signal"; df.height()],
        ))
    }
}

impl OptimizableStrategy for AtrBreakout {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("atr_mult".to_string(), (0.8, 3.0));
        map.insert("rsi_filter".to_string(), (45.0, 60.0));
        map.insert("trend_ema".to_string(), (20.0, 120.0));
        map
    }

    fn set_params(&mut self, params: &StrategyParams) {
        self.atr_mult = params.get("atr_mult", 1.5);
        self.rsi_filter = params.get("rsi_filter", 50.0);
        self.trend_ema = params.get("trend_ema", 50.0) as usize;
    }
}

// -----------------------------------------------------------------------------
// NEW STRATEGY 1: On-Balance Volume Trend (ObvTrend)
// -----------------------------------------------------------------------------
// Uses volume flow to confirm trend direction.
// Logic: Calculate OBV, then apply Fast/Slow EMAs on the OBV line.
// Signal: OBV_Fast > OBV_Slow -> Long, OBV_Fast < OBV_Slow -> Short.

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObvTrend {
    pub obv_fast: usize,
    pub obv_slow: usize,
}

impl ObvTrend {
    pub fn new() -> Self {
        Self {
            obv_fast: 20,
            obv_slow: 50,
        }
    }
}

impl Default for ObvTrend {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for ObvTrend {
    fn name(&self) -> &str {
        "OBV_Trend"
    }

    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        // 1. Calculate OBV via cumulative signed volume
        let close = df.column("close")?.f64()?;
        let volume = df.column("volume")?.f64()?;
        let mut obv_vals = vec![0.0; df.height()];

        for i in 1..df.height() {
            let prev_close = close.get(i - 1).unwrap_or(0.0);
            let curr_close = close.get(i).unwrap_or(prev_close);
            let vol = volume.get(i).unwrap_or(0.0);

            obv_vals[i] = if curr_close > prev_close {
                obv_vals[i - 1] + vol
            } else if curr_close < prev_close {
                obv_vals[i - 1] - vol
            } else {
                obv_vals[i - 1]
            };
        }

        let obv_series = Series::new("obv", obv_vals);

        // 2. Apply EMAs on OBV
        // Note: We must convert Series to DataFrame to use existing ewm_mean nicely or use expressions
        let fast_opt = EWMOptions {
            alpha: 1.0 / self.obv_fast as f64,
            adjust: true,
            bias: false,
            min_periods: self.obv_fast,
            ignore_nulls: true,
        };
        let slow_opt = EWMOptions {
            alpha: 1.0 / self.obv_slow as f64,
            adjust: true,
            bias: false,
            min_periods: self.obv_slow,
            ignore_nulls: true,
        };

        let ema_df = DataFrame::new(vec![obv_series.clone()])?
            .lazy()
            .with_columns(vec![
                col("obv").ewm_mean(fast_opt).alias("obv_fast"),
                col("obv").ewm_mean(slow_opt).alias("obv_slow"),
            ])
            .collect()?;

        let fast = ema_df.column("obv_fast")?.f64()?;
        let slow = ema_df.column("obv_slow")?.f64()?;

        let mut signals = vec![0.0; df.height()];
        for i in 0..df.height() {
            let f = fast.get(i).unwrap_or(0.0);
            let s = slow.get(i).unwrap_or(0.0);

            if f > s {
                signals[i] = 1.0;
            } else if f < s {
                signals[i] = -1.0;
            }
        }

        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new(
            "explanation",
            vec!["ObvTrend strategy signal"; df.height()],
        ))
    }
}

impl OptimizableStrategy for ObvTrend {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("obv_fast".to_string(), (5.0, 30.0));
        map.insert("obv_slow".to_string(), (30.0, 100.0));
        map
    }

    fn set_params(&mut self, params: &StrategyParams) {
        self.obv_fast = params.get("obv_fast", 20.0) as usize;
        self.obv_slow = params.get("obv_slow", 50.0) as usize;
    }
}

// -----------------------------------------------------------------------------
// NEW STRATEGY 2: MACD Trend (MacdTrend)
// -----------------------------------------------------------------------------
// Classic momentum strategy using MACD Histogram.

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MacdTrend {
    pub fast: usize,
    pub slow: usize,
    pub signal: usize,
}

impl MacdTrend {
    pub fn new() -> Self {
        Self {
            fast: 12,
            slow: 26,
            signal: 9,
        }
    }
}

impl Default for MacdTrend {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for MacdTrend {
    fn name(&self) -> &str {
        "MACD_Trend"
    }
    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        // Recalculate MACD based on dynamic params
        let fast_opt = EWMOptions {
            alpha: 1.0 / self.fast as f64,
            adjust: true,
            bias: false,
            min_periods: self.fast,
            ignore_nulls: true,
        };
        let slow_opt = EWMOptions {
            alpha: 1.0 / self.slow as f64,
            adjust: true,
            bias: false,
            min_periods: self.slow,
            ignore_nulls: true,
        };
        let sig_opt = EWMOptions {
            alpha: 1.0 / self.signal as f64,
            adjust: true,
            bias: false,
            min_periods: self.signal,
            ignore_nulls: true,
        };

        let temp = df
            .clone()
            .lazy()
            .with_columns(vec![
                col("close").ewm_mean(fast_opt).alias("ema_f"),
                col("close").ewm_mean(slow_opt).alias("ema_s"),
            ])
            .with_column((col("ema_f") - col("ema_s")).alias("macd_line"))
            .with_column(col("macd_line").ewm_mean(sig_opt).alias("macd_sig"))
            .with_column((col("macd_line") - col("macd_sig")).alias("hist"))
            .collect()?;

        let hist = temp.column("hist")?.f64()?;
        let mut signals = vec![0.0; df.height()];

        for i in 0..df.height() {
            let h = hist.get(i).unwrap_or(0.0);
            if h > 0.0 {
                signals[i] = 1.0;
            } else if h < 0.0 {
                signals[i] = -1.0;
            }
        }
        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new(
            "explanation",
            vec!["MacdTrend strategy signal"; df.height()],
        ))
    }
}

impl OptimizableStrategy for MacdTrend {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("fast".to_string(), (8.0, 20.0));
        map.insert("slow".to_string(), (21.0, 40.0));
        map.insert("signal".to_string(), (5.0, 15.0));
        map
    }
    fn set_params(&mut self, params: &StrategyParams) {
        self.fast = params.get("fast", 12.0) as usize;
        self.slow = params.get("slow", 26.0) as usize;
        self.signal = params.get("signal", 9.0) as usize;
    }
}

// -----------------------------------------------------------------------------
// NEW STRATEGY 3: RSI Mean Reversion (RsiMeanReversion)
// -----------------------------------------------------------------------------
// Contrarian strategy: Buy oversold, Sell overbought.

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RsiMeanReversion {
    pub rsi_lower: f64,
    pub rsi_upper: f64,
}

impl RsiMeanReversion {
    pub fn new() -> Self {
        Self {
            rsi_lower: 30.0,
            rsi_upper: 70.0,
        }
    }
}

impl Default for RsiMeanReversion {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for RsiMeanReversion {
    fn name(&self) -> &str {
        "RSI_Reversion"
    }
    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        let rsi = df.column("rsi")?.f64()?;
        let mut signals = vec![0.0; df.height()];

        // Simple state machine to hold position until opposite signal
        let mut current_pos = 0.0;

        for i in 0..df.height() {
            let r = rsi.get(i).unwrap_or(50.0);
            if r < self.rsi_lower {
                current_pos = 1.0;
            } else if r > self.rsi_upper {
                current_pos = -1.0;
            }
            signals[i] = current_pos;
        }
        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        let rsi = df.column("rsi")?.f64()?;
        let mut reasons = Vec::with_capacity(df.height());

        let mut current_pos = 0.0;

        for i in 0..df.height() {
            let r = rsi.get(i).unwrap_or(50.0);

            let prev_pos = current_pos;

            if r < self.rsi_lower {
                current_pos = 1.0;
                reasons.push(format!(
                    "LONG SIGNAL: RSI ({:.2}) crossed below lower band ({:.2})",
                    r, self.rsi_lower
                ));
            } else if r > self.rsi_upper {
                current_pos = -1.0;
                reasons.push(format!(
                    "SHORT SIGNAL: RSI ({:.2}) crossed above upper band ({:.2})",
                    r, self.rsi_upper
                ));
            } else if prev_pos > 0.0 {
                reasons.push(format!(
                    "HOLD LONG: RSI ({:.2}) has not reached upper target ({:.2})",
                    r, self.rsi_upper
                ));
            } else if prev_pos < 0.0 {
                reasons.push(format!(
                    "HOLD SHORT: RSI ({:.2}) has not reached lower target ({:.2})",
                    r, self.rsi_lower
                ));
            } else {
                reasons.push(format!(
                    "WAIT: RSI ({:.2}) is in neutral zone ({:.2}-{:.2})",
                    r, self.rsi_lower, self.rsi_upper
                ));
            }
        }
        Ok(Series::new("explanation", reasons))
    }
}

impl OptimizableStrategy for RsiMeanReversion {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("rsi_lower".to_string(), (15.0, 40.0));
        map.insert("rsi_upper".to_string(), (60.0, 85.0));
        map
    }
    fn set_params(&mut self, params: &StrategyParams) {
        self.rsi_lower = params.get("rsi_lower", 30.0);
        self.rsi_upper = params.get("rsi_upper", 70.0);
    }
}

// -----------------------------------------------------------------------------
// NEW STRATEGY 4: Price Momentum (PriceMomentum)
// -----------------------------------------------------------------------------
// Rate of Change (ROC) based strategy.

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceMomentum {
    pub roc_period: usize,
    pub threshold: f64,
}

impl PriceMomentum {
    pub fn new() -> Self {
        Self {
            roc_period: 14,
            threshold: 0.0,
        }
    }
}

impl Default for PriceMomentum {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for PriceMomentum {
    fn name(&self) -> &str {
        "Price_Momentum"
    }
    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        let roc = df
            .clone()
            .lazy()
            .with_column(
                col("close")
                    .pct_change(lit(self.roc_period as u64))
                    .alias("roc"),
            )
            .collect()?;

        let roc_vals = roc.column("roc")?.f64()?;
        let mut signals = vec![0.0; df.height()];

        for i in 0..df.height() {
            let val = roc_vals.get(i).unwrap_or(0.0);
            if val > self.threshold {
                signals[i] = 1.0;
            } else if val < -self.threshold {
                signals[i] = -1.0;
            }
        }
        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new(
            "explanation",
            vec!["PriceMomentum strategy signal"; df.height()],
        ))
    }
}

impl OptimizableStrategy for PriceMomentum {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("roc_period".to_string(), (5.0, 50.0));
        map.insert("threshold".to_string(), (0.0, 0.05));
        map
    }
    fn set_params(&mut self, params: &StrategyParams) {
        self.roc_period = params.get("roc_period", 14.0) as usize;
        self.threshold = params.get("threshold", 0.0);
    }
}

// -----------------------------------------------------------------------------
// NEW STRATEGY 5: Adaptive MA Crossover (AdaptiveMaCrossover)
// -----------------------------------------------------------------------------
// Two EMAs with optimizable periods.

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveMaCrossover {
    pub fast_period: usize,
    pub slow_period: usize,
}

impl AdaptiveMaCrossover {
    pub fn new() -> Self {
        Self {
            fast_period: 20,
            slow_period: 50,
        }
    }
}

impl Default for AdaptiveMaCrossover {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for AdaptiveMaCrossover {
    fn name(&self) -> &str {
        "Adaptive_MA_Cross"
    }
    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        let fast_opt = EWMOptions {
            alpha: 1.0 / self.fast_period as f64,
            adjust: true,
            bias: false,
            min_periods: self.fast_period,
            ignore_nulls: true,
        };
        let slow_opt = EWMOptions {
            alpha: 1.0 / self.slow_period as f64,
            adjust: true,
            bias: false,
            min_periods: self.slow_period,
            ignore_nulls: true,
        };

        let temp = df
            .clone()
            .lazy()
            .with_columns(vec![
                col("close").ewm_mean(fast_opt).alias("ema_f"),
                col("close").ewm_mean(slow_opt).alias("ema_s"),
            ])
            .collect()?;

        let f = temp.column("ema_f")?;
        let s = temp.column("ema_s")?;

        let mask = f.gt(s)?;
        let mut signals = vec![0.0; df.height()];

        for i in 0..df.height() {
            if mask.get(i).unwrap_or(false) {
                signals[i] = 1.0;
            } else {
                signals[i] = -1.0;
            }
        }
        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new(
            "explanation",
            vec!["AdaptiveMaCrossover strategy signal"; df.height()],
        ))
    }
}

impl OptimizableStrategy for AdaptiveMaCrossover {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("fast_period".to_string(), (5.0, 50.0));
        map.insert("slow_period".to_string(), (51.0, 200.0));
        map
    }
    fn set_params(&mut self, params: &StrategyParams) {
        self.fast_period = params.get("fast_period", 20.0) as usize;
        self.slow_period = params.get("slow_period", 50.0) as usize;
    }
}

// -----------------------------------------------------------------------------
// NEW STRATEGY 6: Funding Rate Mean Reversion (FundingRateReversion)
// -----------------------------------------------------------------------------
// Uses perpetual futures funding rate as a contrarian signal:
//   - Extreme positive funding → market is overheated long → SHORT
//   - Extreme negative funding → market is overheated short → LONG
//
// The funding_rate_z column (z-score vs rolling 30-period mean/std) must
// be present in the DataFrame. Use FundingRateLoader::align_to_ohlcv() first.
//
// Parameters:
//   entry_z: z-score threshold to enter (default: 1.5 — 1.5 std devs from mean)
//   exit_z:  z-score level at which to exit (default: 0.5 — near normal)
//   confirm_rsi: RSI filter to avoid fighting strong trends (default: 40/60)

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundingRateReversion {
    /// Enter when |funding_rate_z| > entry_z
    pub entry_z: f64,
    /// Exit when |funding_rate_z| < exit_z (funding reverted to normal)
    pub exit_z: f64,
    /// RSI threshold: only go long when RSI > confirm_rsi_low (avoid catching falling knife)
    pub confirm_rsi_low: f64,
    /// RSI threshold: only go short when RSI < confirm_rsi_high
    pub confirm_rsi_high: f64,
}

impl FundingRateReversion {
    pub fn new() -> Self {
        Self {
            entry_z: 1.5,
            exit_z: 0.5,
            confirm_rsi_low: 30.0,
            confirm_rsi_high: 70.0,
        }
    }
}

impl Default for FundingRateReversion {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalGenerator for FundingRateReversion {
    fn name(&self) -> &str {
        "FundingRate_Reversion"
    }

    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> {
        Ok(())
    }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        // Try to get funding_rate_z; fall back to funding_rate if z not present
        let has_z = df.column("funding_rate_z").is_ok();
        let has_fr = df.column("funding_rate").is_ok();

        if !has_z && !has_fr {
            // No funding data at all — return neutral
            return Ok(Series::new("signal", vec![0.0f64; df.height()]));
        }

        let fr_z: Vec<f64> = if has_z {
            df.column("funding_rate_z")?
                .f64()?
                .into_iter()
                .map(|v| v.unwrap_or(0.0))
                .collect()
        } else {
            // Compute simple z-score on the fly from raw funding rate
            let raw: Vec<f64> = df.column("funding_rate")?
                .f64()?
                .into_iter()
                .map(|v| v.unwrap_or(0.0))
                .collect();
            let n = raw.len();
            let window = 30usize;
            let mut z = vec![0.0f64; n];
            for i in 0..n {
                let start = if i >= window { i - window + 1 } else { 0 };
                let slice = &raw[start..=i];
                let mean = slice.iter().sum::<f64>() / slice.len() as f64;
                let std = (slice.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / slice.len() as f64).sqrt();
                z[i] = if std > f64::EPSILON { (raw[i] - mean) / std } else { 0.0 };
            }
            z
        };

        let rsi_opt: Option<Vec<f64>> = df.column("rsi").ok().and_then(|s| {
            Some(s.f64().ok()?.into_iter().map(|v| v.unwrap_or(50.0)).collect())
        });

        let n = df.height();
        let mut signals = vec![0.0f64; n];
        let mut position = 0.0f64; // track current position for exit logic

        for i in 0..n {
            let z = fr_z[i];
            let rsi = rsi_opt.as_ref().map(|r| r[i]).unwrap_or(50.0);

            // Check exit condition first
            if position != 0.0 && z.abs() < self.exit_z {
                position = 0.0;
            }

            // Entry conditions
            if position == 0.0 {
                if z > self.entry_z && rsi < self.confirm_rsi_high {
                    // Extreme positive funding → overheated longs → SHORT
                    position = -1.0;
                } else if z < -self.entry_z && rsi > self.confirm_rsi_low {
                    // Extreme negative funding → overheated shorts → LONG
                    position = 1.0;
                }
            }

            signals[i] = position;
        }

        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new(
            "explanation",
            vec!["FundingRateReversion: contrarian entry on extreme funding rates"; df.height()],
        ))
    }
}

impl OptimizableStrategy for FundingRateReversion {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut map = HashMap::new();
        map.insert("entry_z".to_string(), (1.0, 3.0));
        map.insert("exit_z".to_string(), (0.2, 1.0));
        map.insert("confirm_rsi_low".to_string(), (20.0, 45.0));
        map.insert("confirm_rsi_high".to_string(), (55.0, 80.0));
        map
    }

    fn set_params(&mut self, params: &StrategyParams) {
        self.entry_z = params.get("entry_z", 1.5);
        self.exit_z = params.get("exit_z", 0.5);
        self.confirm_rsi_low = params.get("confirm_rsi_low", 30.0);
        self.confirm_rsi_high = params.get("confirm_rsi_high", 70.0);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// STRATEGY 7: Volatility-Adjusted Momentum
// -----------------------------------------------------------------------------
// Pure momentum with volatility normalization:
//   - Compute N-bar return, divide by ATR to normalize for volatility
//   - Rank by vol-adjusted momentum within the bar's own history
//   - Long when recent momentum is top-decile, short when bottom-decile
//   - Add a trend filter: only take signals aligned with the medium-term trend
//
// Unlike cross-sectional momentum (which requires multi-asset data at runtime),
// this works on a single asset — volume-adjusted momentum that only fires
// when momentum is extreme relative to this asset's own history.
//
// Parameters:
//   mom_period: look-back bars for return (default: 20)
//   vol_period: ATR look-back (default: 14)
//   rank_threshold: fractile threshold to enter (default: 0.8 = top/bottom 20%)
//   trend_period: medium-term EMA period for trend filter (default: 50)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VolAdjustedMomentum {
    pub mom_period: usize,
    pub vol_period: usize,
    pub rank_threshold: f64,
    pub trend_period: usize,
}

impl VolAdjustedMomentum {
    pub fn new() -> Self {
        Self {
            mom_period: 20,
            vol_period: 14,
            rank_threshold: 0.8,
            trend_period: 50,
        }
    }
}

impl Default for VolAdjustedMomentum {
    fn default() -> Self { Self::new() }
}

impl SignalGenerator for VolAdjustedMomentum {
    fn name(&self) -> &str { "VolAdjMomentum" }
    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> { Ok(()) }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        let closes: Vec<f64> = df.column("close")?.f64()?.into_iter()
            .map(|v| v.unwrap_or(0.0)).collect();
        let n = closes.len();
        let mp = self.mom_period;
        let vp = self.vol_period;
        let tp = self.trend_period;

        // Compute N-bar return
        let mut returns = vec![0.0f64; n];
        for i in mp..n {
            if closes[i - mp] > 0.0 {
                returns[i] = (closes[i] - closes[i - mp]) / closes[i - mp];
            }
        }

        // Compute ATR (approximation using high/low if available, else price std)
        let atr: Vec<f64> = if df.column("high").is_ok() && df.column("low").is_ok() {
            let highs: Vec<f64> = df.column("high")?.f64()?.into_iter()
                .map(|v| v.unwrap_or(0.0)).collect();
            let lows: Vec<f64> = df.column("low")?.f64()?.into_iter()
                .map(|v| v.unwrap_or(0.0)).collect();
            let mut atr_vals = vec![0.0f64; n];
            for i in 1..n {
                let tr = (highs[i] - lows[i])
                    .max((highs[i] - closes[i - 1]).abs())
                    .max((lows[i] - closes[i - 1]).abs());
                let start = if i >= vp { i - vp + 1 } else { 1 };
                // Simple rolling mean of TR
                let slice_len = (i - start + 1) as f64;
                atr_vals[i] = atr_vals[i - 1] * (slice_len - 1.0) / slice_len + tr / slice_len;
            }
            atr_vals
        } else {
            // Fallback: rolling std of returns
            let mut atr_vals = vec![0.01f64; n];
            for i in vp..n {
                let slice = &returns[(i - vp)..i];
                let mean = slice.iter().sum::<f64>() / slice.len() as f64;
                let var = slice.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / slice.len() as f64;
                atr_vals[i] = var.sqrt().max(1e-8);
            }
            atr_vals
        };

        // Vol-adjusted momentum
        let mut mom_adj = vec![0.0f64; n];
        for i in 0..n {
            let a = if closes[i] > 0.0 { atr[i] / closes[i] } else { 0.0 };
            mom_adj[i] = if a > 1e-8 { returns[i] / a } else { 0.0 };
        }

        // Compute EMA for trend filter
        let alpha = 2.0 / (tp as f64 + 1.0);
        let mut ema = vec![closes[0]; n];
        for i in 1..n {
            ema[i] = closes[i] * alpha + ema[i - 1] * (1.0 - alpha);
        }

        // Rolling rank of mom_adj over look-back window (same as mom_period)
        let rank_window = (mp * 3).max(60);
        let mut signals = vec![0.0f64; n];

        for i in rank_window..n {
            let start = i - rank_window + 1;
            let window: Vec<f64> = mom_adj[start..=i].to_vec();
            let current = mom_adj[i];

            // Rank: what fraction of window values is current value above?
            let rank = window.iter().filter(|&&v| v < current).count() as f64
                / window.len() as f64;

            let uptrend = closes[i] > ema[i];

            if rank >= self.rank_threshold && uptrend {
                signals[i] = 1.0; // strong momentum + uptrend → long
            } else if rank <= (1.0 - self.rank_threshold) && !uptrend {
                signals[i] = -1.0; // weak momentum + downtrend → short
            }
        }

        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new("explanation",
            vec!["VolAdjMomentum: long on top-decile vol-adjusted momentum + uptrend"; df.height()]))
    }
}

impl OptimizableStrategy for VolAdjustedMomentum {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut m = HashMap::new();
        m.insert("mom_period".to_string(),    (10.0, 50.0));
        m.insert("vol_period".to_string(),    (7.0, 28.0));
        m.insert("rank_threshold".to_string(),(0.70, 0.92));
        m.insert("trend_period".to_string(),  (20.0, 100.0));
        m
    }

    fn set_params(&mut self, p: &StrategyParams) {
        self.mom_period    = p.get("mom_period",    20.0) as usize;
        self.vol_period    = p.get("vol_period",    14.0) as usize;
        self.rank_threshold= p.get("rank_threshold", 0.8);
        self.trend_period  = p.get("trend_period",  50.0) as usize;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// STRATEGY 8: Regime-Filtered Trend Following
// -----------------------------------------------------------------------------
// Detects the current market regime (trending vs ranging) using ATR percentile
// and applies different logic in each:
//   - Trending regime: follow EMA crossover with momentum confirmation
//   - Ranging regime: mean-revert using Bollinger Band z-score
//
// This is an adaptive strategy — it doesn't try to force one style in all markets.
//
// Parameters:
//   atr_lookback: bars to compute ATR percentile (default: 100)
//   atr_trend_pct: percentile above which ATR = trending (default: 0.6)
//   ema_fast, ema_slow: for trend-following leg (default: 10, 30)
//   bb_period, bb_std: for mean-reversion leg (default: 20, 2.0)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegimeAdaptive {
    pub atr_lookback: usize,
    pub atr_trend_pct: f64,
    pub ema_fast: usize,
    pub ema_slow: usize,
    pub bb_period: usize,
    pub bb_std: f64,
}

impl RegimeAdaptive {
    pub fn new() -> Self {
        Self {
            atr_lookback: 100,
            atr_trend_pct: 0.60,
            ema_fast: 10,
            ema_slow: 30,
            bb_period: 20,
            bb_std: 2.0,
        }
    }
}

impl Default for RegimeAdaptive {
    fn default() -> Self { Self::new() }
}

impl SignalGenerator for RegimeAdaptive {
    fn name(&self) -> &str { "RegimeAdaptive" }
    fn train(&mut self, _: &DataFrame, _: &Series) -> Result<()> { Ok(()) }

    fn predict(&self, df: &DataFrame) -> Result<Series> {
        let closes: Vec<f64> = df.column("close")?.f64()?.into_iter()
            .map(|v| v.unwrap_or(0.0)).collect();
        let n = closes.len();

        // ATR (true range approximation)
        let atr_raw: Vec<f64> = if df.column("high").is_ok() && df.column("low").is_ok() {
            let highs: Vec<f64> = df.column("high")?.f64()?.into_iter()
                .map(|v| v.unwrap_or(0.0)).collect();
            let lows: Vec<f64> = df.column("low")?.f64()?.into_iter()
                .map(|v| v.unwrap_or(0.0)).collect();
            let mut v = vec![0.0f64; n];
            for i in 1..n {
                v[i] = (highs[i] - lows[i])
                    .max((highs[i] - closes[i-1]).abs())
                    .max((lows[i] - closes[i-1]).abs());
            }
            v
        } else {
            // Use candle body as proxy
            (0..n).map(|i| if i == 0 { 0.0 } else { (closes[i] - closes[i-1]).abs() }).collect()
        };

        // Normalize ATR by close price
        let atr_pct: Vec<f64> = atr_raw.iter().enumerate()
            .map(|(i, &a)| if closes[i] > 0.0 { a / closes[i] } else { 0.0 })
            .collect();

        // EMA helper
        let ema = |period: usize| -> Vec<f64> {
            let alpha = 2.0 / (period as f64 + 1.0);
            let mut e = vec![closes[0]; n];
            for i in 1..n { e[i] = closes[i] * alpha + e[i-1] * (1.0 - alpha); }
            e
        };

        let ema_f = ema(self.ema_fast);
        let ema_s = ema(self.ema_slow);

        // Bollinger bands
        let bb_mean: Vec<f64> = (0..n).map(|i| {
            let s = if i >= self.bb_period { i - self.bb_period + 1 } else { 0 };
            closes[s..=i].iter().sum::<f64>() / (i - s + 1) as f64
        }).collect();
        let bb_std_v: Vec<f64> = (0..n).map(|i| {
            let s = if i >= self.bb_period { i - self.bb_period + 1 } else { 0 };
            let m = bb_mean[i];
            let var = closes[s..=i].iter().map(|c| (c-m).powi(2)).sum::<f64>() / (i-s+1) as f64;
            var.sqrt()
        }).collect();

        let mut signals = vec![0.0f64; n];
        let lb = self.atr_lookback;

        for i in lb.max(self.ema_slow)..n {
            // Determine regime: is current ATR in top X% of last N bars?
            let window = &atr_pct[(i - lb)..=i];
            let rank = window.iter().filter(|&&a| a < atr_pct[i]).count() as f64
                / window.len() as f64;
            let is_trending = rank >= self.atr_trend_pct;

            if is_trending {
                // Trend-following: EMA crossover
                if ema_f[i] > ema_s[i] && ema_f[i-1] <= ema_s[i-1] {
                    signals[i] = 1.0;
                } else if ema_f[i] < ema_s[i] && ema_f[i-1] >= ema_s[i-1] {
                    signals[i] = -1.0;
                } else {
                    signals[i] = signals[i-1]; // hold
                }
            } else {
                // Ranging regime: Bollinger Band mean-reversion
                let bbs = bb_std_v[i];
                if bbs < 1e-8 {
                    signals[i] = 0.0;
                    continue;
                }
                let z = (closes[i] - bb_mean[i]) / (bbs * self.bb_std);
                if z < -1.0 {
                    signals[i] = 1.0;  // below lower band → long
                } else if z > 1.0 {
                    signals[i] = -1.0; // above upper band → short
                } else {
                    signals[i] = 0.0;  // inside bands → flat
                }
            }
        }

        Ok(Series::new("signal", signals))
    }

    fn explain(&self, df: &DataFrame) -> Result<Series> {
        Ok(Series::new("explanation",
            vec!["RegimeAdaptive: trend-follow in high-ATR, mean-revert in low-ATR"; df.height()]))
    }
}

impl OptimizableStrategy for RegimeAdaptive {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)> {
        let mut m = HashMap::new();
        m.insert("atr_lookback".to_string(),  (50.0, 200.0));
        m.insert("atr_trend_pct".to_string(), (0.50, 0.75));
        m.insert("ema_fast".to_string(),       (5.0, 20.0));
        m.insert("ema_slow".to_string(),       (20.0, 60.0));
        m.insert("bb_period".to_string(),      (10.0, 30.0));
        m.insert("bb_std".to_string(),         (1.5, 3.0));
        m
    }

    fn set_params(&mut self, p: &StrategyParams) {
        self.atr_lookback  = p.get("atr_lookback",  100.0) as usize;
        self.atr_trend_pct = p.get("atr_trend_pct",   0.6);
        self.ema_fast      = p.get("ema_fast",        10.0) as usize;
        self.ema_slow      = p.get("ema_slow",        30.0) as usize;
        self.bb_period     = p.get("bb_period",       20.0) as usize;
        self.bb_std        = p.get("bb_std",           2.0);
    }
}

#[cfg(test)]
mod funding_rate_tests {
    use super::*;
    use polars::prelude::*;

    fn make_df_with_funding(fr_z: Vec<f64>, rsi: Vec<f64>) -> DataFrame {
        let n = fr_z.len();
        DataFrame::new(vec![
            Series::new("funding_rate_z".into(), fr_z),
            Series::new("rsi".into(), rsi),
            Series::new("close".into(), vec![100.0f64; n]),
        ])
        .unwrap()
    }

    #[test]
    fn test_no_funding_data_returns_neutral() {
        let strat = FundingRateReversion::default();
        let df = DataFrame::new(vec![
            Series::new("close".into(), vec![100.0f64; 5]),
        ])
        .unwrap();
        let signals = strat.predict(&df).unwrap();
        let vals: Vec<f64> = signals.f64().unwrap().into_iter().flatten().collect();
        assert!(vals.iter().all(|&v| v == 0.0), "Should be all neutral without funding data");
    }

    #[test]
    fn test_extreme_positive_funding_goes_short() {
        let strat = FundingRateReversion {
            entry_z: 1.5,
            exit_z: 0.5,
            confirm_rsi_low: 30.0,
            confirm_rsi_high: 70.0,
        };
        // fr_z = 2.0 (extreme positive) → short; rsi = 60 (below 70 → confirms short)
        let fr_z = vec![0.0, 0.0, 2.0, 2.0, 2.0];
        let rsi  = vec![50.0, 50.0, 60.0, 60.0, 60.0];
        let df = make_df_with_funding(fr_z, rsi);
        let signals = strat.predict(&df).unwrap();
        let vals: Vec<f64> = signals.f64().unwrap().into_iter().flatten().collect();
        assert_eq!(vals[2], -1.0, "Extreme positive funding + RSI<70 should short");
        assert_eq!(vals[3], -1.0, "Should hold short");
    }

    #[test]
    fn test_extreme_negative_funding_goes_long() {
        let strat = FundingRateReversion::default();
        // fr_z = -2.0 (extreme negative) → long; rsi = 40 (above 30 → confirms long)
        let fr_z = vec![0.0, 0.0, -2.0, -2.0, -2.0];
        let rsi  = vec![50.0, 50.0, 40.0, 40.0, 40.0];
        let df = make_df_with_funding(fr_z, rsi);
        let signals = strat.predict(&df).unwrap();
        let vals: Vec<f64> = signals.f64().unwrap().into_iter().flatten().collect();
        assert_eq!(vals[2], 1.0, "Extreme negative funding + RSI>30 should go long");
    }

    #[test]
    fn test_rsi_filter_blocks_short_in_strong_uptrend() {
        let strat = FundingRateReversion {
            confirm_rsi_high: 70.0,
            ..Default::default()
        };
        // Strong uptrend (RSI=85 > 70) should block the short even with extreme funding
        let fr_z = vec![2.5, 2.5, 2.5];
        let rsi  = vec![85.0, 85.0, 85.0];
        let df = make_df_with_funding(fr_z, rsi);
        let signals = strat.predict(&df).unwrap();
        let vals: Vec<f64> = signals.f64().unwrap().into_iter().flatten().collect();
        assert!(vals.iter().all(|&v| v == 0.0), "RSI filter should block short in strong uptrend");
    }

    #[test]
    fn test_rsi_filter_blocks_long_in_strong_downtrend() {
        let strat = FundingRateReversion {
            confirm_rsi_low: 30.0,
            ..Default::default()
        };
        // Strong downtrend (RSI=15 < 30) should block the long
        let fr_z = vec![-2.5, -2.5, -2.5];
        let rsi  = vec![15.0, 15.0, 15.0];
        let df = make_df_with_funding(fr_z, rsi);
        let signals = strat.predict(&df).unwrap();
        let vals: Vec<f64> = signals.f64().unwrap().into_iter().flatten().collect();
        assert!(vals.iter().all(|&v| v == 0.0), "RSI filter should block long in strong downtrend");
    }

    #[test]
    fn test_exit_when_funding_normalises() {
        let strat = FundingRateReversion {
            entry_z: 1.5,
            exit_z: 0.5,
            ..Default::default()
        };
        // Enter short at bar 1 (z=2.0), funding normalises at bar 3 (z=0.3 < 0.5)
        let fr_z = vec![0.0, 2.0, 1.8, 0.3, 0.2];
        let rsi  = vec![50.0, 60.0, 60.0, 55.0, 55.0];
        let df = make_df_with_funding(fr_z, rsi);
        let signals = strat.predict(&df).unwrap();
        let vals: Vec<f64> = signals.f64().unwrap().into_iter().flatten().collect();
        assert_eq!(vals[1], -1.0, "Should be short at bar 1");
        assert_eq!(vals[2], -1.0, "Should hold short at bar 2 (z=1.8 > exit_z=0.5)");
        assert_eq!(vals[3],  0.0, "Should exit at bar 3 (z=0.3 < exit_z=0.5)");
        assert_eq!(vals[4],  0.0, "Should stay flat at bar 4");
    }

    #[test]
    fn test_optimizable_param_ranges() {
        let strat = FundingRateReversion::default();
        let ranges = strat.param_ranges();
        assert!(ranges.contains_key("entry_z"));
        assert!(ranges.contains_key("exit_z"));
        let (min, max) = ranges["entry_z"];
        assert!(min < max, "entry_z range must be valid");
        assert!(min > 0.0, "entry_z must be positive");
    }

    #[test]
    fn test_set_params_updates_fields() {
        use crate::algo::optimization::StrategyParams;
        let mut strat = FundingRateReversion::default();
        let mut p = StrategyParams::new();
        p.params.insert("entry_z".to_string(), 2.5);
        p.params.insert("exit_z".to_string(), 0.8);
        strat.set_params(&p);
        assert_eq!(strat.entry_z, 2.5);
        assert_eq!(strat.exit_z, 0.8);
    }
}

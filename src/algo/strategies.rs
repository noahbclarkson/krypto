use crate::algo::optimization::{OptimizableStrategy, StrategyParams};
use crate::algo::SignalGenerator;
use anyhow::Result;
use polars::prelude::*;
use std::collections::{HashMap, VecDeque};

#[derive(Clone)]
pub struct DynamicTrend {
    ema_fast: usize,
    ema_slow: usize,
    rsi_filter: f64,
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

#[derive(Clone)]
pub struct RelativeStrengthStrat {
    rs_ema_period: usize,
    rsi_entry: f64,
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

#[derive(Clone)]
pub struct BollingerReversion {
    bb_period: usize,
    bb_std: f64,
    rsi_filter: f64,
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

#[derive(Clone)]
pub struct AtrBreakout {
    atr_mult: f64,
    rsi_filter: f64,
    trend_ema: usize,
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

#[derive(Clone)]
pub struct VolatilitySqueeze {
    bb_mult: f64,
    kc_mult: f64,
    period: usize,
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

#[derive(Clone)]
pub struct LeadLagStrategy {
    min_bench_move: f64,
    max_self_move: f64,
    decay: f64,
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

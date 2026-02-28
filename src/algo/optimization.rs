use super::SignalGenerator;
use crate::backtest::engine::{BacktestResult, Backtester};
use polars::prelude::*;
use rand::Rng;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct StrategyParams {
    pub params: HashMap<String, f64>,
}

impl StrategyParams {
    pub fn new() -> Self {
        Self {
            params: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str, default: f64) -> f64 {
        *self.params.get(key).unwrap_or(&default)
    }
}

impl Default for StrategyParams {
    fn default() -> Self {
        Self::new()
    }
}

pub trait OptimizableStrategy: SignalGenerator {
    fn param_ranges(&self) -> HashMap<String, (f64, f64)>;
    fn set_params(&mut self, params: &StrategyParams);
}

pub struct Optimizer {
    iterations: usize,
    train_split: f64,
    trailing_sl: f64,
}

impl Optimizer {
    pub fn new(iterations: usize, train_split: f64) -> Self {
        Self {
            iterations,
            train_split,
            trailing_sl: 0.05,
        }
    }

    pub fn optimize<S: OptimizableStrategy + Clone>(
        &self,
        strategy: &mut S,
        df: &DataFrame,
    ) -> (StrategyParams, Option<BacktestResult>) {
        let len = df.height();
        let train_len = (len as f64 * self.train_split) as usize;
        let train_df = df.slice(0, train_len);

        let mut best_score = -999.0;
        let mut best_params = StrategyParams::new();
        let mut best_result: Option<BacktestResult> = None;
        let mut rng = rand::thread_rng();
        let ranges = strategy.param_ranges();
        let backtester = Backtester::new(10_000.0, 0.0, 10.0); // 10 bps slippage
        let trailing = self.trailing_sl;

        // Grid-guided search: evaluate corner/mid points for each param
        let mut grid_points: Vec<(String, Vec<f64>)> = Vec::new();
        for (key, (min, max)) in &ranges {
            let mid = (min + max) / 2.0;
            grid_points.push((key.clone(), vec![*min, mid, *max]));
        }

        // Generate combinations up to iteration budget
        fn build_params(
            idx: usize,
            grid: &[(String, Vec<f64>)],
            current: &mut StrategyParams,
            out: &mut Vec<StrategyParams>,
            budget: usize,
        ) {
            if out.len() >= budget {
                return;
            }
            if idx == grid.len() {
                out.push(current.clone());
                return;
            }
            let (ref key, ref values) = grid[idx];
            for v in values {
                current.params.insert(key.clone(), *v);
                build_params(idx + 1, grid, current, out, budget);
                if out.len() >= budget {
                    break;
                }
            }
        }

        let mut candidate_params: Vec<StrategyParams> = Vec::new();
        build_params(
            0,
            &grid_points,
            &mut StrategyParams::new(),
            &mut candidate_params,
            self.iterations,
        );

        for _ in 0..self.iterations {
            let mut current_params = StrategyParams::new();
            for (key, (min, max)) in &ranges {
                let val = rng.gen_range(*min..=*max);
                current_params.params.insert(key.clone(), val);
            }
            candidate_params.push(current_params);
            if candidate_params.len() >= self.iterations * 2 {
                break;
            }
        }

        for current_params in candidate_params.into_iter().take(self.iterations) {
            let mut test_strat = strategy.clone();
            test_strat.set_params(&current_params);

            if let Ok(signals) = test_strat.predict(&train_df) {
                if let Ok(result) = backtester.run(&train_df, &signals, trailing, 0.0) {
                    if result.total_return_pct > 0.0 && result.total_trades > 5 {
                        let score = result.sharpe_ratio * result.profit_factor.min(3.0);
                        if score > best_score {
                            best_score = score;
                            best_params = current_params;
                            best_result = Some(result);
                        }
                    }
                }
            }
        }

        strategy.set_params(&best_params);
        (best_params, best_result)
    }
}

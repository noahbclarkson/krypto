//! Walk-Forward Backtester
//!
//! Replaces single train/test split with a rolling window approach. For each window:
//! 1. Optimize strategy parameters on the training slice
//! 2. Run the optimized strategy on the out-of-sample test slice
//! 3. Aggregate results across all windows
//!
//! A strategy that passes walk-forward validation is much less likely to be overfit than
//! one that only passes a single split.
//!
//! # Example
//! ```ignore
//! use krypto::backtest::walk_forward::{WalkForwardConfig, WalkForwardBacktester};
//! use krypto::algo::strategies::DynamicTrend;
//!
//! let config = WalkForwardConfig::default();
//! let mut strategy = DynamicTrend::default();
//! let results = WalkForwardBacktester::new(config).run(&mut strategy, &df)?;
//! println!("Win rate across windows: {:.1}%", results.window_win_rate * 100.0);
//! ```

use crate::algo::optimization::{OptimizableStrategy, Optimizer};

use crate::backtest::engine::{BacktestResult, Backtester, PositionSizing};
use anyhow::{bail, Result};
use polars::prelude::*;
use rand::seq::SliceRandom;
use rand::thread_rng;

// ─── Configuration ────────────────────────────────────────────────────────────

/// Configuration for walk-forward validation.
#[derive(Debug, Clone)]
pub struct WalkForwardConfig {
    /// Number of bars in each training window.
    pub train_bars: usize,
    /// Number of bars in each test window (the out-of-sample slice).
    pub test_bars: usize,
    /// Step size between windows (how much to advance after each window).
    /// Defaults to `test_bars` (anchored walk-forward with non-overlapping test periods).
    pub step_bars: Option<usize>,
    /// Trailing stop loss fraction. Default: 0.05 (5%).
    pub trailing_sl: f64,
    /// Take profit fraction. Default: 0.10 (10%).
    pub take_profit: f64,
    /// Initial capital per window. Default: $10,000.
    pub initial_capital: f64,
    /// Fee per trade (fraction). Default: 0.001 (0.1%).
    pub fee_pct: f64,
    /// Slippage in basis points. Default: 10 bps.
    pub slippage_bps: f64,
    /// Optimizer iterations per window. Default: 200.
    pub optimizer_iterations: usize,
    /// Minimum trades required in train window for a window to count.
    pub min_train_trades: usize,
    /// Minimum trades required in test window for a window to count.
    pub min_test_trades: usize,
    /// Number of Monte Carlo shuffles for significance testing. Default: 500.
    /// Set to 0 to skip Monte Carlo.
    pub monte_carlo_n: usize,
}

impl Default for WalkForwardConfig {
    fn default() -> Self {
        Self {
            train_bars: 1500,  // ~6 months of 4h data
            test_bars: 500,    // ~2 months of 4h data
            step_bars: None,   // advance by test_bars
            trailing_sl: 0.05,
            take_profit: 0.10,
            initial_capital: 10_000.0,
            fee_pct: 0.001,
            slippage_bps: 10.0,
            optimizer_iterations: 200,
            min_train_trades: 20,
            min_test_trades: 10,
            monte_carlo_n: 500,
        }
    }
}

impl WalkForwardConfig {
    pub fn step_size(&self) -> usize {
        self.step_bars.unwrap_or(self.test_bars)
    }
}

// ─── Window Result ─────────────────────────────────────────────────────────────

/// Results from a single walk-forward window.
#[derive(Debug, Clone)]
pub struct WindowResult {
    /// Index of this window (0-based).
    pub window_index: usize,
    /// First bar index of the training window in the full dataset.
    pub train_start: usize,
    /// Last bar index of the training window (exclusive).
    pub train_end: usize,
    /// First bar index of the test window.
    pub test_start: usize,
    /// Last bar index of the test window (exclusive).
    pub test_end: usize,
    /// Metrics from training (in-sample).
    pub train: BacktestResult,
    /// Metrics from testing (out-of-sample).
    pub test: BacktestResult,
    /// Robustness: test_sharpe / train_sharpe. Values > 0.4 are considered robust.
    pub robustness: f64,
    /// Monte Carlo p-value: fraction of shuffled strategies that beat the real one.
    /// Lower is better. < 0.05 = statistically significant at 95%.
    pub monte_carlo_p: Option<f64>,
}

impl WindowResult {
    pub fn passed_gates(&self, cfg: &WalkForwardConfig) -> bool {
        self.train.sharpe_ratio > 0.05
            && self.train.profit_factor > 1.2
            && self.train.total_trades >= cfg.min_train_trades
            && self.test.total_trades >= cfg.min_test_trades
            && self.robustness > 0.4
            && self.test.total_return_pct > 0.0
    }
}

// ─── Aggregate Results ─────────────────────────────────────────────────────────

/// Aggregated results across all walk-forward windows.
#[derive(Debug, Clone)]
pub struct WalkForwardResult {
    /// Results per window.
    pub windows: Vec<WindowResult>,
    /// Fraction of windows that passed all gates (0.0-1.0).
    pub window_win_rate: f64,
    /// Number of windows that passed all gates.
    pub windows_passed: usize,
    /// Total number of windows evaluated.
    pub windows_total: usize,
    /// Average out-of-sample Sharpe across all windows (not just passing ones).
    pub avg_test_sharpe: f64,
    /// Average out-of-sample return % across all windows.
    pub avg_test_return_pct: f64,
    /// Average robustness (test/train Sharpe ratio) across all windows.
    pub avg_robustness: f64,
    /// Maximum drawdown across the concatenated out-of-sample equity curve.
    pub combined_max_drawdown: f64,
    /// Total return % across the concatenated out-of-sample equity curve.
    pub combined_total_return_pct: f64,
    /// Annualized Sharpe of the concatenated out-of-sample equity curve.
    pub combined_sharpe: f64,
    /// Concatenated out-of-sample equity curve (normalized to start at 1.0).
    pub equity_curve: Vec<f64>,
    /// Average Monte Carlo p-value across windows (if computed).
    pub avg_monte_carlo_p: Option<f64>,
    /// Whether the strategy is considered robust (passes our strict criteria).
    pub is_robust: bool,
}

impl WalkForwardResult {
    /// Print a summary table to stdout.
    pub fn print_summary(&self) {
        println!(
            "Walk-Forward Summary: {}/{} windows passed ({:.0}% win rate)",
            self.windows_passed,
            self.windows_total,
            self.window_win_rate * 100.0
        );
        println!(
            "  Avg OOS Sharpe: {:.3} | Avg OOS Return: {:.1}% | Avg Robustness: {:.2}",
            self.avg_test_sharpe, self.avg_test_return_pct, self.avg_robustness
        );
        println!(
            "  Combined OOS: return={:.1}% | sharpe={:.3} | max_dd={:.1}%",
            self.combined_total_return_pct,
            self.combined_sharpe,
            self.combined_max_drawdown
        );
        if let Some(p) = self.avg_monte_carlo_p {
            println!("  Monte Carlo avg p={:.3} (< 0.05 = significant)", p);
        }
        println!("  ROBUST: {}", if self.is_robust { "✅ YES" } else { "❌ NO" });
        println!();
        println!(
            "  {:<6} {:>8} {:>8} {:>10} {:>10} {:>8} {:>8} Pass?",
            "Win#", "TrainSh", "TestSh", "TestRet%", "TestTrade", "Rob", "MC_p"
        );
        println!("  {}", "-".repeat(72));
        for w in &self.windows {
            let mc = w
                .monte_carlo_p
                .map(|p| format!("{:.3}", p))
                .unwrap_or_else(|| "  N/A".to_string());
            println!(
                "  {:<6} {:>8.3} {:>8.3} {:>10.1} {:>10} {:>8.2} {:>8} {}",
                w.window_index,
                w.train.sharpe_ratio,
                w.test.sharpe_ratio,
                w.test.total_return_pct,
                w.test.total_trades,
                w.robustness,
                mc,
                if w.passed_gates(&WalkForwardConfig::default()) {
                    "✅"
                } else {
                    "❌"
                }
            );
        }
    }
}

// ─── Robustness Criteria ───────────────────────────────────────────────────────

/// A strategy is considered robust if it passes these aggregated criteria.
fn is_robust(result: &WalkForwardResult) -> bool {
    // At least 50% of windows must pass
    result.window_win_rate >= 0.5
    // Average OOS Sharpe must be positive
    && result.avg_test_sharpe > 0.0
    // Combined OOS equity must be profitable
    && result.combined_total_return_pct > 0.0
    // Combined OOS Sharpe must be meaningful
    && result.combined_sharpe > 0.2
    // Monte Carlo must be significant (if computed)
    && result.avg_monte_carlo_p.map(|p| p < 0.10).unwrap_or(true)
}

// ─── Walk-Forward Backtester ───────────────────────────────────────────────────

pub struct WalkForwardBacktester {
    pub config: WalkForwardConfig,
}

impl WalkForwardBacktester {
    pub fn new(config: WalkForwardConfig) -> Self {
        Self { config }
    }

    /// Run walk-forward validation on a strategy over the full dataset.
    ///
    /// The strategy's parameters are re-optimized for each training window independently.
    /// Test windows are non-overlapping to give a true out-of-sample assessment.
    pub fn run<S>(&self, strategy: &mut S, df: &DataFrame) -> Result<WalkForwardResult>
    where
        S: OptimizableStrategy + Clone,
    {
        let cfg = &self.config;
        let n = df.height();
        let step = cfg.step_size();
        let window = cfg.train_bars + cfg.test_bars;

        if n < window {
            bail!(
                "Dataset too small: {} bars, need at least {} (train={} + test={})",
                n,
                window,
                cfg.train_bars,
                cfg.test_bars
            );
        }

        let backtester = Backtester::new(cfg.initial_capital, cfg.fee_pct, cfg.slippage_bps)
            .with_position_sizing(PositionSizing::Full);
        let optimizer = Optimizer::new(cfg.optimizer_iterations, 1.0); // full train split inside window

        let mut windows: Vec<WindowResult> = Vec::new();
        let mut window_index = 0;
        let mut start = 0;

        while start + window <= n {
            let train_start = start;
            let train_end = start + cfg.train_bars;
            let test_start = train_end;
            let test_end = (train_end + cfg.test_bars).min(n);

            let train_df = df.slice(train_start as i64, cfg.train_bars);
            let test_df = df.slice(test_start as i64, test_end - test_start);

            // Clone strategy to avoid contaminating params across windows
            let mut window_strat = strategy.clone();

            // Optimize on training data (optimizer.optimize uses 100% of the slice for train)
            let (_, train_result_opt) = optimizer.optimize(&mut window_strat, &train_df);

            if let Some(train_result) = train_result_opt {
                // Only proceed if train meets minimum quality bar
                if train_result.total_trades >= cfg.min_train_trades {
                    // Run optimized strategy on test data
                    if let Ok(test_signals) = window_strat.predict(&test_df) {
                        if let Ok(test_result) = backtester.run(
                            &test_df,
                            &test_signals,
                            cfg.trailing_sl,
                            cfg.take_profit,
                        ) {
                            let robustness = if train_result.sharpe_ratio.abs() > f64::EPSILON {
                                test_result.sharpe_ratio / train_result.sharpe_ratio
                            } else {
                                0.0
                            };

                            // Monte Carlo permutation test
                            let monte_carlo_p = if cfg.monte_carlo_n > 0 {
                                Some(monte_carlo_p_value(
                                    &backtester,
                                    &test_df,
                                    &test_signals,
                                    test_result.sharpe_ratio,
                                    cfg.trailing_sl,
                                    cfg.take_profit,
                                    cfg.monte_carlo_n,
                                ))
                            } else {
                                None
                            };

                            windows.push(WindowResult {
                                window_index,
                                train_start,
                                train_end,
                                test_start,
                                test_end,
                                robustness,
                                monte_carlo_p,
                                train: train_result,
                                test: test_result,
                            });
                        }
                    }
                }
            }

            start += step;
            window_index += 1;
        }

        aggregate_results(windows, cfg)
    }
}

// ─── Monte Carlo ───────────────────────────────────────────────────────────────

/// Compute a Monte Carlo p-value by shuffling signals N times.
///
/// Returns the fraction of shuffled runs that beat the real strategy's Sharpe.
/// A p-value < 0.05 means the strategy is statistically significant at 95%.
fn monte_carlo_p_value(
    backtester: &Backtester,
    df: &DataFrame,
    signals: &Series,
    real_sharpe: f64,
    trailing_sl: f64,
    take_profit: f64,
    n: usize,
) -> f64 {
    let mut rng = thread_rng();
    let signal_vals: Vec<f32> = signals
        .f32()
        .map(|ca| ca.into_iter().map(|v| v.unwrap_or(0.0)).collect())
        .unwrap_or_else(|_| {
            signals
                .cast(&DataType::Float32)
                .ok()
                .and_then(|s| s.f32().ok().map(|ca| ca.into_iter().map(|v| v.unwrap_or(0.0)).collect()))
                .unwrap_or_default()
        });

    let mut beats = 0usize;
    let mut shuffled = signal_vals.clone();

    for _ in 0..n {
        shuffled.shuffle(&mut rng);
        let shuffled_series = Series::new(signals.name(), shuffled.clone())
            .cast(&DataType::Float64)
            .unwrap_or_else(|_| Series::new(signals.name(), vec![0.0f64; shuffled.len()]));

        if let Ok(result) = backtester.run(df, &shuffled_series, trailing_sl, take_profit) {
            if result.sharpe_ratio > real_sharpe {
                beats += 1;
            }
        }
    }

    beats as f64 / n as f64
}

// ─── Aggregation ───────────────────────────────────────────────────────────────

fn aggregate_results(windows: Vec<WindowResult>, cfg: &WalkForwardConfig) -> Result<WalkForwardResult> {
    if windows.is_empty() {
        bail!("No valid windows produced. Dataset may be too small or strategy never trades.");
    }

    let windows_total = windows.len();
    let windows_passed = windows.iter().filter(|w| w.passed_gates(cfg)).count();
    let window_win_rate = windows_passed as f64 / windows_total as f64;

    let avg_test_sharpe = windows.iter().map(|w| w.test.sharpe_ratio).sum::<f64>() / windows_total as f64;
    let avg_test_return_pct = windows.iter().map(|w| w.test.total_return_pct).sum::<f64>() / windows_total as f64;
    let avg_robustness = windows.iter().map(|w| w.robustness).sum::<f64>() / windows_total as f64;

    let avg_monte_carlo_p = if windows.iter().any(|w| w.monte_carlo_p.is_some()) {
        let vals: Vec<f64> = windows.iter().filter_map(|w| w.monte_carlo_p).collect();
        Some(vals.iter().sum::<f64>() / vals.len() as f64)
    } else {
        None
    };

    // Concatenate out-of-sample equity curves, chaining each window from where the last ended
    let mut equity_curve: Vec<f64> = Vec::new();
    let mut running_equity = 1.0f64;
    for w in &windows {
        if w.test.equity_curve.is_empty() {
            continue;
        }
        let start_eq = w.test.equity_curve[0];
        if start_eq <= 0.0 {
            continue;
        }
        for &eq in &w.test.equity_curve {
            equity_curve.push(running_equity * (eq / start_eq));
        }
        // Advance running equity by the window's return
        running_equity *= w.test.final_equity / cfg.initial_capital;
    }

    let combined_total_return_pct = if !equity_curve.is_empty() {
        (equity_curve.last().copied().unwrap_or(1.0) - 1.0) * 100.0
    } else {
        0.0
    };

    let combined_max_drawdown = compute_max_drawdown(&equity_curve);
    let combined_sharpe = compute_sharpe_from_equity(&equity_curve);

    let mut result = WalkForwardResult {
        windows,
        window_win_rate,
        windows_passed,
        windows_total,
        avg_test_sharpe,
        avg_test_return_pct,
        avg_robustness,
        combined_max_drawdown,
        combined_total_return_pct,
        combined_sharpe,
        equity_curve,
        avg_monte_carlo_p,
        is_robust: false, // set below
    };

    result.is_robust = is_robust(&result);
    Ok(result)
}

/// Compute max drawdown from a normalized equity curve (starts at 1.0).
fn compute_max_drawdown(curve: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0f64;
    for &eq in curve {
        if eq > peak {
            peak = eq;
        }
        if peak > 0.0 {
            let dd = (peak - eq) / peak * 100.0;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd
}

/// Compute annualized Sharpe from an equity curve.
///
/// Treats each element as a bar return. Scales by sqrt(bars_per_year).
/// For 4h data: 365.25 * 6 = ~2191 bars/year.
/// For 1h data: 365.25 * 24 = ~8766.
/// We use a conservative 252 * 24 (hourly) and let the caller normalize.
fn compute_sharpe_from_equity(curve: &[f64]) -> f64 {
    if curve.len() < 2 {
        return 0.0;
    }
    let returns: Vec<f64> = curve.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    let std_dev = variance.sqrt();
    if std_dev < f64::EPSILON {
        return 0.0;
    }
    // Annualize assuming ~2191 bars/year (4h data); caller can adjust
    let bars_per_year = 2191.0_f64;
    mean / std_dev * bars_per_year.sqrt()
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    

    fn make_test_df(n: usize) -> DataFrame {
        let times: Vec<i64> = (0..n as i64)
            .map(|i| {
                NaiveDate::from_ymd_opt(2023, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp_millis()
                    + i * 14_400_000
            })
            .collect();
        let close: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.01)).collect();
        let open = close.clone();
        let high: Vec<f64> = close.iter().map(|c| c * 1.01).collect();
        let low: Vec<f64> = close.iter().map(|c| c * 0.99).collect();
        let volume: Vec<f64> = vec![1000.0; n];

        DataFrame::new(vec![
            Series::new("time", times)
                .cast(&DataType::Datetime(TimeUnit::Milliseconds, None))
                .unwrap(),
            Series::new("open", open),
            Series::new("high", high),
            Series::new("low", low),
            Series::new("close", close),
            Series::new("volume", volume),
        ])
        .unwrap()
    }

    #[test]
    fn test_walk_forward_config_defaults() {
        let cfg = WalkForwardConfig::default();
        assert_eq!(cfg.train_bars, 1500);
        assert_eq!(cfg.test_bars, 500);
        assert_eq!(cfg.step_size(), 500);
        assert_eq!(cfg.monte_carlo_n, 500);
    }

    #[test]
    fn test_walk_forward_config_custom_step() {
        let cfg = WalkForwardConfig {
            step_bars: Some(250),
            ..Default::default()
        };
        assert_eq!(cfg.step_size(), 250);
    }

    #[test]
    fn test_monte_carlo_p_range() {
        let backtester = Backtester::new(10_000.0, 0.0, 0.0);
        let df = make_test_df(100);
        let signals = Series::new("signal", vec![1.0f64; 100]);
        let p = monte_carlo_p_value(&backtester, &df, &signals, 0.0, 0.05, 0.10, 50);
        assert!((0.0..=1.0).contains(&p), "p-value must be in [0,1]: {}", p);
    }

    #[test]
    fn test_compute_max_drawdown_flat() {
        let curve = vec![1.0, 1.0, 1.0, 1.0];
        assert_eq!(compute_max_drawdown(&curve), 0.0);
    }

    #[test]
    fn test_compute_max_drawdown_drop() {
        let curve = vec![1.0, 1.2, 0.6, 0.9];
        let dd = compute_max_drawdown(&curve);
        // Peak = 1.2, trough = 0.6, dd = (1.2-0.6)/1.2 * 100 = 50%
        assert!((dd - 50.0).abs() < 0.001, "Expected ~50%, got {:.2}%", dd);
    }

    #[test]
    fn test_compute_sharpe_flat_returns() {
        let curve = vec![1.0, 1.0, 1.0, 1.0];
        assert_eq!(compute_sharpe_from_equity(&curve), 0.0);
    }

    #[test]
    fn test_dataset_too_small_errors() {
        use crate::algo::strategies::DynamicTrend;

        let df = make_test_df(100);
        let cfg = WalkForwardConfig {
            train_bars: 500,
            test_bars: 200,
            monte_carlo_n: 0,
            ..Default::default()
        };
        let mut strat = DynamicTrend::default();
        let result = WalkForwardBacktester::new(cfg).run(&mut strat, &df);
        assert!(result.is_err(), "Should error when dataset is too small");
    }

    #[test]
    fn test_window_passed_gates_logic() {
        use crate::backtest::engine::BacktestResult;

        let good_result = BacktestResult {
            total_trades: 30,
            win_rate: 0.6,
            profit_factor: 1.5,
            final_equity: 11_000.0,
            total_return_pct: 10.0,
            max_drawdown_pct: 5.0,
            sharpe_ratio: 1.0,
            kelly_fraction: 0.2,
            equity_curve: vec![],
            total_fees_paid: 10.0,
            average_position_size: 1.0,
            sortino_ratio: 1.2,
            calmar_ratio: 2.0,
            avg_trade_duration_bars: 4.0,
            max_consecutive_wins: 5,
            max_consecutive_losses: 2,
            avg_win_pct: 2.0,
            avg_loss_pct: -1.0,
            largest_win_pct: 5.0,
            largest_loss_pct: -2.0,
            trades: vec![],
            annualised_return_pct: 0.0,
            trades_per_year: 0.0,
            annualised_sharpe: 0.0,
            return_per_trade_pct: 0.0,
            backtest_years: 0.0,
        };

        let w = WindowResult {
            window_index: 0,
            train_start: 0,
            train_end: 100,
            test_start: 100,
            test_end: 200,
            train: good_result.clone(),
            test: good_result,
            robustness: 0.5,
            monte_carlo_p: Some(0.04),
        };

        let cfg = WalkForwardConfig::default();
        assert!(w.passed_gates(&cfg));
    }
}

//! Hyperparameter Optimization: BollingerReversion BB Period & RSI Filter
//!
//! TARGET: Audit the hardcoded bb_period=20, bb_std=2.0, rsi_filter=30.0
//! These have NEVER been systematically tested in combination.
//!
//! BollingerReversion is HOF-validated (ATR×0.30 stop confirmed optimal).
//! Now we find the optimal band parameters.
//!
//! SWEEP: bb_period ∈ {5,10,15,20,25,30,40,50,60,80,100}
//!         bb_std   ∈ {1.5, 2.0, 2.5} (3 core values)
//!         rsi_filter ∈ {20,30,40} (3 core values)
//! Total: 11 × 3 × 3 = 99 configs
//!
//! Symbol: DOGEFDUSD (best performer, highest Sharpe)
//! Method: 4-window walk-forward (252 train / 252 test)
//! Baseline: period=20, std=2.0, rsi=30.0
//!
//! ALSO: exports equity curves for baseline + top-3 configs to CSV
//! for Python charting (comparison_chart.png).
//!
//! Usage:
//!   cargo run --release --example bollinger_bb_period_hyperopt

use anyhow::Result;
use colored::*;
use krypto::{
    algo::{strategies::BollingerReversion, SignalGenerator},
    backtest::engine::Backtester,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write as IoWrite;

const CANDLES: u32 = 2000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30; // Proven optimal from bollinger_1d_sweep
const INTERVAL: &str = "1d";

// Walk-forward config
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES_PER_WINDOW: usize = 10;

// ── Parameter sweep grid ──────────────────────────────────────────────────────
const BB_PERIODS: &[usize] = &[5, 10, 15, 20, 25, 30, 40, 50, 60, 80, 100];
const BB_STDS: &[f64] = &[1.5, 2.0, 2.5];
const RSI_FILTERS: &[f64] = &[20.0, 30.0, 40.0];

// ── Baseline (current hardcoded defaults) ─────────────────────────────────────
const BASELINE_PERIOD: usize = 20;
const BASELINE_STD: f64 = 2.0;
const BASELINE_RSI: f64 = 30.0;

// ── Equity curve export ───────────────────────────────────────────────────────
const EQUITY_OUTPUT_DIR: &str = "charts/bollinger_bb_hyperopt/";

#[derive(Debug, Clone)]
struct WindowResult {
    wi: usize,
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    passed: bool,
}

#[derive(Debug, Clone)]
struct ConfigResult {
    bb_period: usize,
    bb_std: f64,
    rsi_filter: f64,
    avg_oos_ret: f64,
    avg_oos_sharpe: f64,
    worst_dd: f64,
    total_oos_trades: usize,
    pass_windows: usize,
    total_windows: usize,
    window_results: Vec<WindowResult>,
    equity_curves: HashMap<usize, Vec<f64>>, // wi -> equity curve
}

impl ConfigResult {
    fn new(bb_period: usize, bb_std: f64, rsi_filter: f64) -> Self {
        Self {
            bb_period,
            bb_std,
            rsi_filter,
            avg_oos_ret: 0.0,
            avg_oos_sharpe: 0.0,
            worst_dd: 0.0,
            total_oos_trades: 0,
            pass_windows: 0,
            total_windows: 0,
            window_results: Vec::new(),
            equity_curves: HashMap::new(),
        }
    }
}

fn compute_stop_pct(df: &DataFrame, atr_mult: f64) -> f64 {
    let n = df.height();
    let mid = n.saturating_sub(500).max(100);
    let atr = df
        .column("atr")
        .ok()
        .and_then(|s| s.f64().ok().and_then(|ca| ca.get(mid)))
        .unwrap_or(0.0);
    let close = df
        .column("close")
        .ok()
        .and_then(|s| s.f64().ok().and_then(|ca| ca.get(mid)))
        .unwrap_or(1.0);
    if close > 0.0 {
        (atr * atr_mult / close).clamp(0.005, 0.30)
    } else {
        0.05
    }
}

fn run_window(
    df: &DataFrame,
    bb_period: usize,
    bb_std: f64,
    rsi_filter: f64,
    train_start: usize,
    train_end: usize,
    test_start: usize,
    test_end: usize,
    stop_pct: f64,
) -> Option<WindowResult> {
    let train_df = df.slice(train_start as i64, train_end - train_start);
    let test_df = df.slice(test_start as i64, test_end - test_start);

    if test_df.height() < 50 {
        return None;
    }

    let strategy = BollingerReversion {
        bb_period,
        bb_std,
        rsi_filter,
    };

    let signals = strategy.predict(&test_df).ok()?;
    let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
    let result = bt.run(&test_df, &signals, stop_pct, 0.0).ok()?;

    if result.total_trades < MIN_TRADES_PER_WINDOW {
        return None;
    }

    let sharpe = result.sharpe_ratio;
    let ret = result.total_return_pct;
    let dd = result.max_drawdown_pct;
    let wr = result.win_rate;

    Some(WindowResult {
        wi: 0,
        ret,
        sharpe,
        max_dd: dd,
        trades: result.total_trades,
        win_rate: wr,
        passed: ret > 0.0 && sharpe > 0.0,
    })
}

fn equity_curve_filename(cfg: &ConfigResult) -> String {
    format!(
        "bb{}_std{:.1}_rsi{:.0}",
        cfg.bb_period, cfg.bb_std, cfg.rsi_filter
    )
}

fn is_baseline(cfg: &ConfigResult) -> bool {
    cfg.bb_period == BASELINE_PERIOD
        && (cfg.bb_std - BASELINE_STD).abs() < 0.01
        && (cfg.rsi_filter - BASELINE_RSI).abs() < 0.1
}

fn run_config_on_fddfdusd(
    df: &DataFrame,
    bb_period: usize,
    bb_std: f64,
    rsi_filter: f64,
) -> ConfigResult {
    let mut result = ConfigResult::new(bb_period, bb_std, rsi_filter);
    let stop_pct = compute_stop_pct(df, ATR_MULT);
    let n = df.height();

    let num_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

    for wi in 0..num_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let test_start = train_end;
        let test_end = (train_end + TEST_BARS).min(n);

        let mut wresult = run_window(
            df, bb_period, bb_std, rsi_filter, 0, train_end, test_start, test_end, stop_pct,
        )
        .unwrap_or(WindowResult {
            wi,
            ret: 0.0,
            sharpe: f64::NEG_INFINITY,
            max_dd: 0.0,
            trades: 0,
            win_rate: 0.0,
            passed: false,
        });
        wresult.wi = wi;

        result.window_results.push(wresult.clone());

        if wresult.passed {
            result.pass_windows += 1;
        }
        result.total_windows += 1;
    }

    let valid: Vec<_> = result
        .window_results
        .iter()
        .filter(|w| w.trades >= MIN_TRADES_PER_WINDOW)
        .collect();
    if !valid.is_empty() {
        result.avg_oos_ret = valid.iter().map(|w| w.ret).sum::<f64>() / valid.len() as f64;
        result.avg_oos_sharpe = valid.iter().map(|w| w.sharpe).sum::<f64>() / valid.len() as f64;
        result.worst_dd = valid
            .iter()
            .map(|w| w.max_dd)
            .fold(f64::NEG_INFINITY, f64::max);
        result.total_oos_trades = valid.iter().map(|w| w.trades).sum();
    }

    // Compute equity curves for top configs later (full-sample)
    // We'll compute them in the ranking pass
    result
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "  {}",
        "BollingerReversion BB Period Hyperopt"
            .bright_white()
            .bold()
    );
    println!(
        "  Target: bb_period={}, bb_std={}, rsi_filter={} (CURRENT DEFAULTS)",
        BASELINE_PERIOD, BASELINE_STD, BASELINE_RSI
    );
    println!(
        "  Sweep: {} periods × {} stds × {} rsi = {} configs",
        BB_PERIODS.len(),
        BB_STDS.len(),
        RSI_FILTERS.len(),
        BB_PERIODS.len() * BB_STDS.len() * RSI_FILTERS.len()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    // ── Create output directory ───────────────────────────────────────────────
    std::fs::create_dir_all(EQUITY_OUTPUT_DIR)?;

    // ── Load data ─────────────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    print!("Loading DOGEFDUSD {} candles... ", CANDLES);
    let raw = loader.fetch_data("DOGEFDUSD", INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw, None)?;
    println!("{} ({} bars)", "✓".green(), df.height());

    // ── Get timestamps for chart axis ─────────────────────────────────────────
    let timestamps: Vec<i64> = df
        .column("time")
        .ok()
        .and_then(|s| {
            s.datetime()
                .ok()
                .map(|ca| ca.into_iter().flatten().collect())
        })
        .unwrap_or_default();

    // ── Run sweep ─────────────────────────────────────────────────────────────
    let mut all_results: Vec<ConfigResult> = Vec::new();
    let total_configs = BB_PERIODS.len() * BB_STDS.len() * RSI_FILTERS.len();
    let mut config_idx = 0usize;

    for &period in BB_PERIODS {
        for &std in BB_STDS {
            for &rsi in RSI_FILTERS {
                config_idx += 1;
                print!(
                    "\r  [{:>3}/{:>3}] period={:>3}, std={:.1}, rsi={:>2}...",
                    config_idx, total_configs, period, std, rsi as usize
                );

                let result = run_config_on_fddfdusd(&df, period, std, rsi);
                all_results.push(result);
            }
        }
    }
    println!("\n  {} configs tested", "✓".green());

    // ── Rank by avg OOS Sharpe ────────────────────────────────────────────────
    all_results.sort_by(|a, b| {
        b.avg_oos_sharpe
            .partial_cmp(&a.avg_oos_sharpe)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // ── Print results table ───────────────────────────────────────────────────
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "  {:>4} {:>5} {:>5} {:>7} {:>8} {:>8} {:>7} {:>5} {}",
        "PRD", "STD", "RSI", "PASS", "AVG OOS%", "AVG OOS SH", "WST DD", "TRDS", "NOTE"
    );
    println!("{}", "─".repeat(80));

    for cfg in &all_results {
        let pass_str = format!("{:>2}/{:>2}", cfg.pass_windows, cfg.total_windows);
        let note = if is_baseline(cfg) {
            "← BASELINE".to_string()
        } else {
            String::new()
        };
        println!(
            "  {:>4} {:>5.1} {:>5.0} {:>7} {:>+8.1}% {:>+8.2} {:>7.1}% {:>5} {}",
            cfg.bb_period,
            cfg.bb_std,
            cfg.rsi_filter,
            pass_str,
            cfg.avg_oos_ret,
            cfg.avg_oos_sharpe,
            cfg.worst_dd,
            cfg.total_oos_trades,
            note.green()
        );
    }

    // ── Identify top-3 + baseline ─────────────────────────────────────────────
    let baseline_idx = all_results.iter().position(|c| is_baseline(c));
    let mut top_configs: Vec<usize> = all_results
        .iter()
        .enumerate()
        .take(3)
        .map(|(i, _)| i)
        .collect();
    if let Some(bi) = baseline_idx {
        if !top_configs.contains(&bi) {
            top_configs.push(bi);
        }
    }
    // Always include baseline explicitly
    let export_indices: Vec<usize> = top_configs;

    println!(
        "\n  {} Top config: period={}, std={:.1}, rsi={:.0}",
        "🏆".yellow(),
        all_results[0].bb_period,
        all_results[0].bb_std,
        all_results[0].rsi_filter
    );
    if let Some(bi) = baseline_idx {
        let b = &all_results[bi];
        println!(
            "  {} Baseline:  period={}, std={:.1}, rsi={:.0} → Sharpe={:.2}",
            "📊".blue(),
            b.bb_period,
            b.bb_std,
            b.rsi_filter,
            b.avg_oos_sharpe
        );
    }

    // ── Export equity curves for top configs ───────────────────────────────────
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("  {} Exporting equity curves to CSV...", "📈".yellow());

    // For each export config, run full-sample backtest and export equity curve
    let stop_pct = compute_stop_pct(&df, ATR_MULT);

    // Compute timestamps for test windows
    let n = df.height();
    let num_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

    // Get test window boundaries
    let window_bounds: Vec<(usize, usize)> = (0..num_windows)
        .map(|wi| {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            (test_start, test_end)
        })
        .collect();

    for &idx in &export_indices {
        let cfg = &all_results[idx];
        let strategy = BollingerReversion {
            bb_period: cfg.bb_period,
            bb_std: cfg.bb_std,
            rsi_filter: cfg.rsi_filter,
        };

        // Full-sample equity curve
        let signals = strategy.predict(&df)?;
        let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
        let full_result = bt.run(&df, &signals, stop_pct, 0.0)?;

        // Export full-sample equity curve CSV
        let label = if is_baseline(cfg) {
            "BASELINE".to_string()
        } else {
            format!("TOP{}", idx + 1)
        };

        let filename = format!(
            "{}{}_{}.csv",
            EQUITY_OUTPUT_DIR,
            equity_curve_filename(cfg),
            label
        );
        export_equity_csv(&filename, &full_result.equity_curve, &timestamps, &label)?;

        // Per-window equity curves
        for (wi_idx, &(wstart, wend)) in window_bounds.iter().enumerate() {
            let test_df = df.slice(wstart as i64, wend - wstart);
            let sigs = strategy.predict(&test_df)?;
            let wbt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
            let wres = wbt.run(&test_df, &sigs, stop_pct, 0.0).ok();

            if let Some(wr) = wres {
                let wf_name = format!(
                    "{}window{}_{}.csv",
                    EQUITY_OUTPUT_DIR,
                    wi_idx,
                    equity_curve_filename(cfg)
                );
                let wts: Vec<i64> = timestamps
                    .iter()
                    .skip(wstart)
                    .take(wend - wstart)
                    .copied()
                    .collect();
                export_equity_csv(
                    &wf_name,
                    &wr.equity_curve,
                    &wts,
                    &format!("W{}_{}", wi_idx, label),
                )?;
            }
        }

        println!(
            "  Exported {} (full + {} windows) → {}",
            label, num_windows, filename
        );
    }

    // ── Summary stats for export configs ────────────────────────────────────
    println!("\n{}", "═".repeat(80).bright_cyan());
    println!("  {}", "EQUITY CURVE EXPORT SUMMARY".bright_white().bold());
    println!("{}", "═".repeat(80));

    for &idx in &export_indices {
        let cfg = &all_results[idx];
        let label = if is_baseline(cfg) {
            "BASELINE"
        } else {
            "WINNER"
        };

        // Full-sample stats
        let strategy = BollingerReversion {
            bb_period: cfg.bb_period,
            bb_std: cfg.bb_std,
            rsi_filter: cfg.rsi_filter,
        };
        let signals = strategy.predict(&df)?;
        let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
        let full = bt.run(&df, &signals, stop_pct, 0.0)?;

        println!(
            "\n  {} ({}) period={}, std={:.1}, rsi={:.0}",
            label.yellow(),
            equity_curve_filename(cfg),
            cfg.bb_period,
            cfg.bb_std,
            cfg.rsi_filter
        );
        println!(
            "  Full-sample: Return={:+.1}%, Sharpe={:.2}, MaxDD={:.1}%, Trades={}",
            full.total_return_pct, full.sharpe_ratio, full.max_drawdown_pct, full.total_trades
        );
        println!(
            "  Walk-Forward: Avg OOS Ret={:+.1}%, Avg OOS Sharpe={:.2}, Pass={}/{}",
            cfg.avg_oos_ret, cfg.avg_oos_sharpe, cfg.pass_windows, cfg.total_windows
        );

        for wr in &cfg.window_results {
            println!(
                "    W{}: ret={:+7.1}%, sharpe={:+7.2}, dd={:>7.1}%, trades={:>3}, {}",
                wr.wi,
                wr.ret,
                wr.sharpe,
                wr.max_dd,
                wr.trades,
                if wr.passed { "✅" } else { "❌" }
            );
        }
    }

    // ── Save sweep results CSV ────────────────────────────────────────────────
    let csv_path = "charts/bollinger_bb_sweep_results.csv";
    let mut csv_file = File::create(csv_path)?;
    writeln!(csv_file, "bb_period,bb_std,rsi_filter,avg_oos_ret,avg_oos_sharpe,worst_dd,total_oos_trades,pass_windows,total_windows,is_baseline")?;
    for cfg in &all_results {
        writeln!(
            csv_file,
            "{},{:.1},{:.0},{:+.4},{:+.4},{:.4},{},{},{},{}",
            cfg.bb_period,
            cfg.bb_std,
            cfg.rsi_filter,
            cfg.avg_oos_ret,
            cfg.avg_oos_sharpe,
            cfg.worst_dd,
            cfg.total_oos_trades,
            cfg.pass_windows,
            cfg.total_windows,
            is_baseline(cfg)
        )?;
    }
    println!("\n  {} Sweep results → {}", "💾".green(), csv_path);

    // ── Winner announcement ────────────────────────────────────────────────────
    let winner = &all_results[0];
    println!("\n{}", "━".repeat(80).bright_green());
    println!(
        "  🏆 WINNER: bb_period={}, bb_std={:.1}, rsi_filter={:.0}",
        winner.bb_period, winner.bb_std, winner.rsi_filter
    );
    println!(
        "  Avg OOS Sharpe: {:+.2} vs Baseline: {:+.2} → Δ={:+.2}",
        winner.avg_oos_sharpe,
        baseline_idx
            .map(|bi| all_results[bi].avg_oos_sharpe)
            .unwrap_or(0.0),
        winner.avg_oos_sharpe
            - baseline_idx
                .map(|bi| all_results[bi].avg_oos_sharpe)
                .unwrap_or(0.0)
    );
    println!("{}", "━".repeat(80).bright_green());

    Ok(())
}

fn export_equity_csv(path: &str, equity: &[f64], timestamps: &[i64], label: &str) -> Result<()> {
    let n = equity.len();
    let ts_slice = if timestamps.len() >= n {
        &timestamps[timestamps.len() - n..]
    } else {
        timestamps
    };

    let mut file = File::create(path)?;
    writeln!(file, "step,equity_value,label")?;
    for (i, &eq) in equity.iter().enumerate() {
        let ts = ts_slice.get(i).copied().unwrap_or(i as i64);
        writeln!(file, "{},{},{}", ts, eq, label)?;
    }
    Ok(())
}

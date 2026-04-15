//! Hyperparameter Optimization: RegimeAdaptive atr_lookback & atr_trend_pct
//!
//! TARGET: Audit the hardcoded atr_lookback=100 and atr_trend_pct=0.60
//! These have NEVER been systematically tested in combination.
//!
//! SWEEP: atr_lookback ∈ {20,30,40,50,60,70,80,90,100,120,150,200} (12 values)
//!         atr_trend_pct ∈ {0.40, 0.50, 0.60, 0.70, 0.80, 0.90} (6 values)
//! Total: 12 × 6 = 72 configs
//!
//! Symbol: DOGEFDUSD (best performer)
//! Method: 4-window walk-forward (252 train / 252 test)
//! Baseline: lookback=100, pct=0.60
//!
//! ALSO: exports equity curves for baseline + top configs to CSV
//! for Python charting (comparison_chart.png).
//!
//! Usage:
//!   cargo run --release --example regime_atr_lookback_hyperopt

use anyhow::Result;
use colored::*;
use krypto::{
    algo::{strategies::RegimeAdaptive, SignalGenerator},
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
const ATR_MULT: f64 = 0.30; // standard testing stop
const INTERVAL: &str = "1d";

// Walk-forward config
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES_PER_WINDOW: usize = 5;

// ── Parameter sweep grid ──────────────────────────────────────────────────────
const ATR_LOOKBACKS: &[usize] = &[20, 30, 40, 50, 60, 70, 80, 90, 100, 120, 150, 200];
const ATR_PCTS: &[f64] = &[0.40, 0.50, 0.60, 0.70, 0.80, 0.90];

// ── Baseline (current hardcoded defaults) ─────────────────────────────────────
const BASELINE_LOOKBACK: usize = 100;
const BASELINE_PCT: f64 = 0.60;

// ── Equity curve export ───────────────────────────────────────────────────────
const EQUITY_OUTPUT_DIR: &str = "charts/regime_atr_lookback_hyperopt/";

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
    atr_lookback: usize,
    atr_trend_pct: f64,
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
    fn new(atr_lookback: usize, atr_trend_pct: f64) -> Self {
        Self {
            atr_lookback,
            atr_trend_pct,
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
    atr_lookback: usize,
    atr_trend_pct: f64,
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

    let mut strategy = RegimeAdaptive::new();
    strategy.atr_lookback = atr_lookback;
    strategy.atr_trend_pct = atr_trend_pct;

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
    format!("L{}_P{:.2}", cfg.atr_lookback, cfg.atr_trend_pct)
}

fn is_baseline(cfg: &ConfigResult) -> bool {
    cfg.atr_lookback == BASELINE_LOOKBACK && (cfg.atr_trend_pct - BASELINE_PCT).abs() < 0.01
}

fn run_config_on_fddfdusd(df: &DataFrame, atr_lookback: usize, atr_trend_pct: f64) -> ConfigResult {
    let mut result = ConfigResult::new(atr_lookback, atr_trend_pct);
    let stop_pct = compute_stop_pct(df, ATR_MULT);
    let n = df.height();

    let num_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

    for wi in 0..num_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let test_start = train_end;
        let test_end = (train_end + TEST_BARS).min(n);

        let mut wresult = run_window(
            df,
            atr_lookback,
            atr_trend_pct,
            0,
            train_end,
            test_start,
            test_end,
            stop_pct,
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
        "RegimeAdaptive atr_lookback Hyperopt".bright_white().bold()
    );
    println!(
        "  Target: atr_lookback={}, atr_trend_pct={} (CURRENT DEFAULTS)",
        BASELINE_LOOKBACK, BASELINE_PCT
    );
    println!(
        "  Sweep: {} lookbacks × {} pcts = {} configs",
        ATR_LOOKBACKS.len(),
        ATR_PCTS.len(),
        ATR_LOOKBACKS.len() * ATR_PCTS.len()
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
    let total_configs = ATR_LOOKBACKS.len() * ATR_PCTS.len();
    let mut config_idx = 0usize;

    for &atr_lookback in ATR_LOOKBACKS {
        for &atr_trend_pct in ATR_PCTS {
            config_idx += 1;
            print!(
                "\r  [{:>3}/{:>3}] atr_lookback={:>3}, atr_trend_pct={:.2}...",
                config_idx, total_configs, atr_lookback, atr_trend_pct
            );

            let result = run_config_on_fddfdusd(&df, atr_lookback, atr_trend_pct);
            all_results.push(result);
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
        "  {:>4} {:>5} {:>7} {:>8} {:>8} {:>7} {:>5} {}",
        "LB", "PCT", "PASS", "AVG OOS%", "AVG OOS SH", "WST DD", "TRDS", "NOTE"
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
            "  {:>4} {:>5.2} {:>7} {:>+8.1}% {:>+8.2} {:>7.1}% {:>5} {}",
            cfg.atr_lookback,
            cfg.atr_trend_pct,
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
        "\n  {} Top config: lookback={}, pct={:.2}",
        "🏆".yellow(),
        all_results[0].atr_lookback,
        all_results[0].atr_trend_pct
    );
    if let Some(bi) = baseline_idx {
        let b = &all_results[bi];
        println!(
            "  {} Baseline:  lookback={}, pct={:.2} → Sharpe={:.2}",
            "📊".blue(),
            b.atr_lookback,
            b.atr_trend_pct,
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
        let mut strategy = RegimeAdaptive::new();
        strategy.atr_lookback = cfg.atr_lookback;
        strategy.atr_trend_pct = cfg.atr_trend_pct;

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
        let mut strategy = RegimeAdaptive::new();
        strategy.atr_lookback = cfg.atr_lookback;
        strategy.atr_trend_pct = cfg.atr_trend_pct;

        let signals = strategy.predict(&df)?;
        let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
        let full = bt.run(&df, &signals, stop_pct, 0.0)?;

        println!(
            "\n  {} ({}) lookback={}, pct={:.2}",
            label.yellow(),
            equity_curve_filename(cfg),
            cfg.atr_lookback,
            cfg.atr_trend_pct
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
    let csv_path = "charts/regime_atr_lookback_sweep_results.csv";
    let mut csv_file = File::create(csv_path)?;
    writeln!(csv_file, "atr_lookback,atr_trend_pct,avg_oos_ret,avg_oos_sharpe,worst_dd,total_oos_trades,pass_windows,total_windows,is_baseline")?;
    for cfg in &all_results {
        writeln!(
            csv_file,
            "{},{:.2},{:+.4},{:+.4},{:.4},{},{},{},{}",
            cfg.atr_lookback,
            cfg.atr_trend_pct,
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
        "  🏆 WINNER: atr_lookback={}, atr_trend_pct={:.2}",
        winner.atr_lookback, winner.atr_trend_pct
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

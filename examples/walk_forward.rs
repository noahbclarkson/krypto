//! Walk-Forward Validation Runner
//!
//! Runs walk-forward backtesting across all strategies and symbol/interval combinations.
//! Unlike the basic backtest which uses a single train/test split, walk-forward uses
//! rolling windows over the full history — a much more rigorous test of robustness.
//!
//! Results are logged to `walk_forward_results.json`.

use colored::*;
use krypto::algo::optimization::OptimizableStrategy;
use krypto::algo::strategies::{
    AdaptiveMaCrossover, AtrBreakout, BollingerReversion, DynamicTrend, MacdTrend, ObvTrend,
    PriceMomentum, RegimeAdaptive, RsiMeanReversion, VolAdjustedMomentum, VolatilitySqueeze,
};
use krypto::backtest::walk_forward::{WalkForwardBacktester, WalkForwardConfig, WalkForwardResult};
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

const CACHE_DIR: &str = "examples/cache";
const RESULTS_FILE: &str = "walk_forward_results.json";

// ─── Serializable result for JSON output ──────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct RunRecord {
    strategy: String,
    symbol: String,
    interval: String,
    windows_total: usize,
    windows_passed: usize,
    window_win_rate: f64,
    avg_test_sharpe: f64,
    avg_test_return_pct: f64,
    avg_robustness: f64,
    combined_return_pct: f64,
    combined_sharpe: f64,
    combined_max_drawdown: f64,
    avg_monte_carlo_p: Option<f64>,
    is_robust: bool,
    elapsed_secs: f64,
}

impl RunRecord {
    fn from_result(
        strategy: &str,
        symbol: &str,
        interval: &str,
        r: &WalkForwardResult,
        elapsed: f64,
    ) -> Self {
        Self {
            strategy: strategy.to_string(),
            symbol: symbol.to_string(),
            interval: interval.to_string(),
            windows_total: r.windows_total,
            windows_passed: r.windows_passed,
            window_win_rate: r.window_win_rate,
            avg_test_sharpe: r.avg_test_sharpe,
            avg_test_return_pct: r.avg_test_return_pct,
            avg_robustness: r.avg_robustness,
            combined_return_pct: r.combined_total_return_pct,
            combined_sharpe: r.combined_sharpe,
            combined_max_drawdown: r.combined_max_drawdown,
            avg_monte_carlo_p: r.avg_monte_carlo_p,
            is_robust: r.is_robust,
            elapsed_secs: elapsed,
        }
    }
}

// ─── Data loading with cache ───────────────────────────────────────────────────

fn cache_path(symbol: &str, interval: &str, limit: u32) -> PathBuf {
    PathBuf::from(CACHE_DIR).join(format!("{symbol}_{interval}_{limit}.bin"))
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CandleCache {
    time_ms: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

fn load_cache(path: &PathBuf) -> Option<Vec<CandleCache>> {
    let data = std::fs::read(path).ok()?;
    bincode::deserialize(&data).ok()
}

fn save_cache(path: &PathBuf, records: &[CandleCache]) {
    if let Ok(data) = bincode::serialize(records) {
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(path));
        let _ = std::fs::write(path, data);
    }
}

fn df_to_cache(df: &polars::frame::DataFrame) -> anyhow::Result<Vec<CandleCache>> {
    use polars::prelude::*;
    let times = df.column("time")?.cast(&DataType::Int64)?;
    let opens = df.column("open")?.f64()?;
    let highs = df.column("high")?.f64()?;
    let lows = df.column("low")?.f64()?;
    let closes = df.column("close")?.f64()?;
    let volumes = df.column("volume")?.f64()?;

    let mut out = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        out.push(CandleCache {
            time_ms: times.i64()?.get(i).unwrap_or(0),
            open: opens.get(i).unwrap_or(0.0),
            high: highs.get(i).unwrap_or(0.0),
            low: lows.get(i).unwrap_or(0.0),
            close: closes.get(i).unwrap_or(0.0),
            volume: volumes.get(i).unwrap_or(0.0),
        });
    }
    Ok(out)
}

fn cache_to_df(records: Vec<CandleCache>) -> anyhow::Result<polars::frame::DataFrame> {
    use polars::prelude::*;
    let mut times = Vec::with_capacity(records.len());
    let mut opens = Vec::with_capacity(records.len());
    let mut highs = Vec::with_capacity(records.len());
    let mut lows = Vec::with_capacity(records.len());
    let mut closes = Vec::with_capacity(records.len());
    let mut volumes = Vec::with_capacity(records.len());
    for r in records {
        times.push(r.time_ms);
        opens.push(r.open);
        highs.push(r.high);
        lows.push(r.low);
        closes.push(r.close);
        volumes.push(r.volume);
    }
    Ok(DataFrame::new(vec![
        Series::new("time".into(), times)
            .cast(&DataType::Datetime(TimeUnit::Milliseconds, None))?,
        Series::new("open".into(), opens),
        Series::new("high".into(), highs),
        Series::new("low".into(), lows),
        Series::new("close".into(), closes),
        Series::new("volume".into(), volumes),
    ])?)
}

async fn load_or_fetch(
    loader: &DataLoader,
    symbol: &str,
    interval: &str,
    limit: u32,
) -> anyhow::Result<(polars::frame::DataFrame, bool)> {
    let path = cache_path(symbol, interval, limit);
    if path.exists() {
        if let Some(records) = load_cache(&path) {
            return cache_to_df(records).map(|df| (df, true));
        }
    }
    let df = loader.fetch_data(symbol, interval, limit).await?;
    if let Ok(records) = df_to_cache(&df) {
        save_cache(&path, &records);
    }
    Ok((df, false))
}

// ─── Strategy runner macro ─────────────────────────────────────────────────────

macro_rules! run_strategy {
    ($name:expr, $strat:expr, $df:expr, $cfg:expr, $symbol:expr, $interval:expr, $records:expr) => {{
        let t0 = Instant::now();
        let mut strat = $strat;
        let wf = WalkForwardBacktester::new($cfg.clone());
        match wf.run(&mut strat, $df) {
            Ok(result) => {
                let elapsed = t0.elapsed().as_secs_f64();
                let status = if result.is_robust {
                    "✅ ROBUST".green().bold()
                } else {
                    "❌ not robust".red()
                };
                println!(
                    "  {:<28} wins={}/{} OOS_sh={:.2} OOS_ret={:.1}% {} ({:.1}s)",
                    $name,
                    result.windows_passed,
                    result.windows_total,
                    result.avg_test_sharpe,
                    result.avg_test_return_pct,
                    status,
                    elapsed
                );
                $records.push(RunRecord::from_result(
                    $name, $symbol, $interval, &result, elapsed,
                ));
            }
            Err(e) => {
                println!("  {:<28} ERROR: {}", $name, e);
            }
        }
    }};
}

// ─── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("{}", "═══ KRYPTO WALK-FORWARD VALIDATOR ═══".cyan().bold());
    println!("Strict gates: train_sharpe>0.05, pf>1.2, train_trades>20, OOS_trades>10, robustness>0.4");
    println!("Robustness: >=50% windows pass, OOS_sharpe>0, combined_sharpe>0.2, MC_p<0.10");
    println!();

    // Config: use fewer bars per window for intervals with less data
    // 1d: 500 train / 150 test | 4h: 1500/500 | 1h: 4000/1500
    let configs: HashMap<&str, WalkForwardConfig> = [
        (
            "1d",
            WalkForwardConfig {
                train_bars: 500,
                test_bars: 150,
                optimizer_iterations: 200,
                monte_carlo_n: 200, // fewer MC iterations to keep runtime sane
                ..Default::default()
            },
        ),
        (
            "4h",
            WalkForwardConfig {
                train_bars: 1500,
                test_bars: 500,
                optimizer_iterations: 200,
                monte_carlo_n: 200,
                ..Default::default()
            },
        ),
        (
            "1h",
            WalkForwardConfig {
                train_bars: 4000,
                test_bars: 1500,
                optimizer_iterations: 200,
                monte_carlo_n: 100,
                ..Default::default()
            },
        ),
    ]
    .into();

    let symbols = vec!["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "DOGEFDUSD", "XRPFDUSD"];
    let intervals = vec!["4h", "1d", "1h"];
    let limit: u32 = 10_000;

    let loader = DataLoader::new(None, None);
    let mut all_records: Vec<RunRecord> = Vec::new();
    let mut robust_count = 0;

    for interval in &intervals {
        println!("{}", format!("── Interval: {} ──", interval).yellow().bold());

        for symbol in &symbols {
            println!("  {} {}", symbol.white().bold(), interval);
            let (df, cached) = load_or_fetch(&loader, symbol, interval, limit).await?;
            let df_tech = FeatureEngine::add_technicals(&df, None)?;
            let hint = if cached { "(cached)" } else { "(fetched)" };
            println!("    {} candles {}", df_tech.height(), hint);

            let cfg = configs[interval].clone();

            run_strategy!("DynamicTrend", DynamicTrend::default(), &df_tech, cfg, symbol, interval, all_records);
            run_strategy!("RsiMeanReversion", RsiMeanReversion::default(), &df_tech, cfg, symbol, interval, all_records);
            run_strategy!("MacdTrend", MacdTrend::default(), &df_tech, cfg, symbol, interval, all_records);
            run_strategy!("BollingerReversion", BollingerReversion::default(), &df_tech, cfg, symbol, interval, all_records);
            run_strategy!("AtrBreakout", AtrBreakout::default(), &df_tech, cfg, symbol, interval, all_records);
            run_strategy!("ObvTrend", ObvTrend::default(), &df_tech, cfg, symbol, interval, all_records);
            run_strategy!("PriceMomentum", PriceMomentum::default(), &df_tech, cfg, symbol, interval, all_records);
            run_strategy!("VolatilitySqueeze", VolatilitySqueeze::default(), &df_tech, cfg, symbol, interval, all_records);
            run_strategy!("AdaptiveMaCrossover", AdaptiveMaCrossover::default(), &df_tech, cfg, symbol, interval, all_records);
            run_strategy!("VolAdjMomentum", VolAdjustedMomentum::default(), &df_tech, cfg, symbol, interval, all_records);
            run_strategy!("RegimeAdaptive", RegimeAdaptive::default(), &df_tech, cfg, symbol, interval, all_records);

            println!();
        }
    }

    // Summary
    robust_count = all_records.iter().filter(|r| r.is_robust).count();
    let total = all_records.len();

    println!("{}", "═══ FINAL SUMMARY ═══".cyan().bold());
    println!("Robust strategies: {}/{}", robust_count, total);
    println!();

    if robust_count > 0 {
        println!("{}", "✅ Robust strategies:".green().bold());
        for r in all_records.iter().filter(|r| r.is_robust) {
            println!(
                "  {} on {}/{}: OOS_ret={:.1}% OOS_sh={:.3} wins={}/{} MC_p={:.3}",
                r.strategy,
                r.symbol,
                r.interval,
                r.combined_return_pct,
                r.combined_sharpe,
                r.windows_passed,
                r.windows_total,
                r.avg_monte_carlo_p.unwrap_or(f64::NAN)
            );
        }
    } else {
        println!(
            "{}",
            "No strategies passed walk-forward validation. This is expected for basic strategies — it means the gates are working.".yellow()
        );
    }

    // Save JSON
    let json = serde_json::to_string_pretty(&all_records)?;
    std::fs::write(RESULTS_FILE, &json)?;
    println!("\nResults saved to {}", RESULTS_FILE);

    Ok(())
}
// Note: ConfirmedDynamicTrend is added separately in a dedicated run

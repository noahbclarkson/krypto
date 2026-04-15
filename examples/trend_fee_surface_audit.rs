//! Audit how much current daily benchmark candidates depend on execution fee path.
//!
//! Purpose:
//! - bring passive / maker-style execution back into scope for credible current candidates
//! - keep the same fair daily harness used elsewhere: signal at close, next-open entry,
//!   fixed 21-bar hold, chronology slices, and no stop-based path assumptions
//! - ask whether lower-fee execution changes trust materially, or mostly flatters the same rows
//!
//! Fee scenarios:
//! - Taker/Taker: 10 bps entry + 10 bps exit (current fair baseline)
//! - MakerEntry/TakerExit: 2 bps entry + 10 bps exit (plausible partial passive path)
//! - Maker/Maker: 2 bps entry + 2 bps exit (optimistic liquid post-only benchmark)

use anyhow::Result;
use chrono::Utc;
use krypto::{
    algo::{strategies::CrossSectionalMomentum, SignalGenerator},
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::{collections::HashMap, fs};

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const RESAMPLE_BLOCKS: usize = 6;
const TURTLE_PERIOD: usize = 20;
const CS_LOOKBACK: usize = 63;
const MIN_TRADES_PER_WINDOW: usize = 30;
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/trend_fee_surface_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/trend_fee_surface_latest.csv";

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
    ),
    ("NoDOGE", &["ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    (
        "OldGuardNoBNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
    (
        "LargeCaps5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"],
    ),
];

const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
    "EOSUSDT", "BCHUSDT",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    CrossSectionalMomentum,
    MacdRegime,
    TurtleRegimeMacd,
    EnsembleMajority3,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::CrossSectionalMomentum,
            Self::MacdRegime,
            Self::TurtleRegimeMacd,
            Self::EnsembleMajority3,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
            Self::EnsembleMajority3 => "Ensemble(Majority 2/3)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FeeScenario {
    TakerTaker,
    MakerEntryTakerExit,
    MakerMaker,
}

impl FeeScenario {
    fn all() -> &'static [FeeScenario] {
        &[
            Self::TakerTaker,
            Self::MakerEntryTakerExit,
            Self::MakerMaker,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::TakerTaker => "Taker/Taker (10+10 bps)",
            Self::MakerEntryTakerExit => "MakerEntry/TakerExit (2+10 bps)",
            Self::MakerMaker => "Maker/Maker (2+2 bps)",
        }
    }

    fn total_fee(&self) -> f64 {
        match self {
            Self::TakerTaker => 0.001 + 0.001,
            Self::MakerEntryTakerExit => 0.0002 + 0.001,
            Self::MakerMaker => 0.0002 + 0.0002,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct StrategyResult {
    total_return_pct: f64,
    trades: usize,
    wins: usize,
}

impl StrategyResult {
    fn win_rate(&self) -> f64 {
        if self.trades == 0 {
            0.0
        } else {
            self.wins as f64 / self.trades as f64
        }
    }
}

#[derive(Clone, Debug)]
struct ResultRow {
    strategy: StrategyKind,
    scenario: FeeScenario,
    full_return_pct: f64,
    full_trades: usize,
    win_rate_pct: f64,
    wf_passed: usize,
    wf_total: usize,
    resample_passed: usize,
    resample_total: usize,
    wf_avg_return_pct: f64,
    resample_avg_return_pct: f64,
}

#[derive(Clone, Debug)]
struct UniverseSummary {
    label: String,
    rows: Vec<ResultRow>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== TREND FEE SURFACE AUDIT ===\n");
    println!("Goal: test whether lower-fee execution materially changes the trust read for current daily candidates");
    println!(
        "Execution stays fixed: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Chronology checks: 4 quarter windows + 15 CPCV-style resamples");
    println!("Scenarios: taker baseline, maker-entry hybrid, optimistic maker/maker\n");

    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache("BTCUSDT", "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;

    let mut data_cache = HashMap::<String, DataFrame>::new();
    data_cache.insert("BTCUSDT".to_string(), bench_df.clone());
    for &symbol in LOAD_SYMBOLS.iter().filter(|&&s| s != "BTCUSDT") {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, Some(&bench_df))?;
        println!("{} bars", enriched.height());
        data_cache.insert(symbol.to_string(), enriched);
    }

    let mut cs_map = HashMap::<String, DataFrame>::new();
    for &symbol in LOAD_SYMBOLS.iter().filter(|&&s| s != "BTCUSDT") {
        cs_map.insert(symbol.to_string(), data_cache.get(symbol).unwrap().clone());
    }
    compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;
    for (symbol, df) in cs_map {
        data_cache.insert(symbol, df);
    }

    let mut universe_summaries = Vec::new();
    for &(label, symbols) in UNIVERSES {
        println!("\n--- Universe: {} ({}) ---", label, symbols.join(", "));
        let quarter_windows = quarter_windows(&data_cache, symbols)?;
        let resample_sets = cpcv_style_windows(&data_cache, symbols)?;
        let mut rows = Vec::new();

        for &strategy in StrategyKind::all() {
            let full_base = evaluate_strategy_on_windows(
                &data_cache,
                strategy,
                symbols,
                &[(0, usable_len(&data_cache, symbols)?)],
                FeeScenario::TakerTaker,
            )?;
            for &scenario in FeeScenario::all() {
                let full = if scenario == FeeScenario::TakerTaker {
                    full_base.clone()
                } else {
                    evaluate_strategy_on_windows(
                        &data_cache,
                        strategy,
                        symbols,
                        &[(0, usable_len(&data_cache, symbols)?)],
                        scenario,
                    )?
                };

                let mut wf_passed = 0usize;
                let mut wf_return_sum = 0.0;
                for &(start, end) in &quarter_windows {
                    let eval = evaluate_strategy_on_windows(
                        &data_cache,
                        strategy,
                        symbols,
                        &[(start, end)],
                        scenario,
                    )?;
                    wf_return_sum += eval.total_return_pct;
                    if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                        wf_passed += 1;
                    }
                }

                let mut resample_passed = 0usize;
                let mut resample_return_sum = 0.0;
                for window_set in &resample_sets {
                    let eval = evaluate_strategy_on_windows(
                        &data_cache,
                        strategy,
                        symbols,
                        window_set,
                        scenario,
                    )?;
                    resample_return_sum += eval.total_return_pct;
                    if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                        resample_passed += 1;
                    }
                }

                rows.push(ResultRow {
                    strategy,
                    scenario,
                    full_return_pct: full.total_return_pct,
                    full_trades: full.trades,
                    win_rate_pct: full.win_rate() * 100.0,
                    wf_passed,
                    wf_total: quarter_windows.len(),
                    resample_passed,
                    resample_total: resample_sets.len(),
                    wf_avg_return_pct: wf_return_sum / quarter_windows.len() as f64,
                    resample_avg_return_pct: resample_return_sum / resample_sets.len() as f64,
                });
            }
        }

        rows.sort_by(|a, b| {
            b.resample_passed
                .cmp(&a.resample_passed)
                .then_with(|| b.wf_passed.cmp(&a.wf_passed))
                .then_with(|| b.full_return_pct.partial_cmp(&a.full_return_pct).unwrap())
        });

        println!(
            "{:<28} {:<30} {:>10} {:>8} {:>8} {:>8} {:>10}",
            "Strategy", "Scenario", "FullRet%", "Trades", "WF", "RS", "WinRate"
        );
        println!("{}", "-".repeat(112));
        for row in &rows {
            println!(
                "{:<28} {:<30} {:>9.1} {:>8} {:>4}/{} {:>4}/{} {:>9.1}%",
                row.strategy.name(),
                row.scenario.name(),
                row.full_return_pct,
                row.full_trades,
                row.wf_passed,
                row.wf_total,
                row.resample_passed,
                row.resample_total,
                row.win_rate_pct,
            );
        }

        universe_summaries.push(UniverseSummary {
            label: label.to_string(),
            rows,
        });
    }

    let (archive_md, archive_csv) = write_snapshot(&universe_summaries)?;
    println!("\nSnapshots written:");
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);
    println!("- {}", archive_md);
    println!("- {}", archive_csv);
    println!("\nInterpretation:");
    println!("- If lower-fee paths only lift levels but not chronology counts, that is execution assistance, not new trust.");
    println!("- If a row improves materially under MakerEntry/TakerExit, passive entry work may deserve a deeper fill-model follow-up.");
    println!("- This is still not a deployment decision; it is a fee-path realism audit.");

    Ok(())
}

fn usable_len(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    Ok(min_symbol_len(data_cache, symbols)?.saturating_sub(HOLD_BARS + 1))
}

fn min_symbol_len(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    symbols
        .iter()
        .filter_map(|symbol| data_cache.get(*symbol).map(|df| df.height()))
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty universe"))
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
    symbols: &[&str],
    windows: &[(usize, usize)],
    scenario: FeeScenario,
) -> Result<StrategyResult> {
    let mut out = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache
            .get(symbol)
            .ok_or_else(|| anyhow::anyhow!("missing {}", symbol))?;
        let signals = signal_for_strategy(df, strategy)?;
        for &(start, end) in windows {
            let result = backtest_fixed_hold_next_open_window(df, &signals, start, end, scenario)?;
            out.total_return_pct += result.total_return_pct;
            out.trades += result.trades;
            out.wins += result.wins;
        }
    }
    Ok(out)
}

fn signal_for_strategy(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    let macd_regime = generate_macd_regime_signals(df)?;
    let turtle_regime_macd = generate_turtle_regime_macd_signals(df, TURTLE_PERIOD)?;
    let cross_sectional = CrossSectionalMomentum::new()
        .predict(df)?
        .f64()?
        .into_iter()
        .map(|v| match v.unwrap_or(0.0).partial_cmp(&0.0) {
            Some(std::cmp::Ordering::Greater) => 1,
            Some(std::cmp::Ordering::Less) => -1,
            _ => 0,
        })
        .collect::<Vec<_>>();

    Ok(match strategy {
        StrategyKind::CrossSectionalMomentum => cross_sectional,
        StrategyKind::MacdRegime => macd_regime,
        StrategyKind::TurtleRegimeMacd => turtle_regime_macd,
        StrategyKind::EnsembleMajority3 => {
            consensus_signal(&[&macd_regime, &turtle_regime_macd, &cross_sectional], 2)
        }
    })
}

fn consensus_signal(signals: &[&Vec<i32>], min_agree: usize) -> Vec<i32> {
    let len = signals.first().map(|s| s.len()).unwrap_or(0);
    let mut out = vec![0i32; len];
    for i in 0..len {
        let long_votes = signals.iter().filter(|sig| sig[i] > 0).count();
        let short_votes = signals.iter().filter(|sig| sig[i] < 0).count();
        if long_votes >= min_agree {
            out[i] = 1;
        } else if short_votes >= min_agree {
            out[i] = -1;
        }
    }
    out
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let macd = df.column("macd")?.f64()?;
    let macd_signal = df.column("macd_signal")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; df.height()];

    for i in 0..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let macd_now = macd.get(i).unwrap_or(0.0);
        let macd_sig_now = macd_signal.get(i).unwrap_or(0.0);
        let sma_now = sma_200.get(i).copied().unwrap_or(0.0);
        if sma_now <= 0.0 {
            continue;
        }
        if macd_now > macd_sig_now && price > sma_now {
            out[i] = 1;
        } else if macd_now < macd_sig_now && price < sma_now {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_regime_signals(df, period)?;
    let macd = df.column("macd")?.f64()?;
    let macd_signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0i32; df.height()];
    for i in 0..df.height() {
        let t = turtle[i];
        let m = if macd.get(i).unwrap_or(0.0) > macd_signal.get(i).unwrap_or(0.0) {
            1
        } else if macd.get(i).unwrap_or(0.0) < macd_signal.get(i).unwrap_or(0.0) {
            -1
        } else {
            0
        };
        if t == m {
            out[i] = t;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; df.height()];
    for i in period..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let sma_now = sma_200.get(i).copied().unwrap_or(0.0);
        if sma_now <= 0.0 {
            continue;
        }
        let highest = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        if price > highest && price > sma_now {
            out[i] = 1;
        } else if price < lowest && price < sma_now {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn calculate_sma(values: &Float64Chunked, period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values.get(i).unwrap_or(0.0);
        if i >= period {
            sum -= values.get(i - period).unwrap_or(0.0);
        }
        if i + 1 >= period {
            out[i] = sum / period as f64;
        }
    }
    out
}

fn backtest_fixed_hold_next_open_window(
    df: &DataFrame,
    signals: &[i32],
    start: usize,
    end: usize,
    scenario: FeeScenario,
) -> Result<StrategyResult> {
    let open = df.column("open")?.f64()?;
    let mut out = StrategyResult::default();
    let fee = scenario.total_fee();

    let mut i = start.max(1);
    while i + HOLD_BARS < end && i + HOLD_BARS < df.height() {
        let signal = signals[i - 1];
        if signal == 0 {
            i += 1;
            continue;
        }
        let entry = open.get(i).unwrap_or(0.0);
        let exit = open.get(i + HOLD_BARS).unwrap_or(0.0);
        if entry <= 0.0 || exit <= 0.0 {
            i += 1;
            continue;
        }
        let gross = if signal > 0 {
            (exit / entry) - 1.0
        } else {
            (entry / exit) - 1.0
        };
        let net = gross - fee;
        out.total_return_pct += net * 100.0;
        out.trades += 1;
        if net > 0.0 {
            out.wins += 1;
        }
        i += HOLD_BARS;
    }
    Ok(out)
}

fn quarter_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<Vec<(usize, usize)>> {
    let usable = usable_len(data_cache, symbols)?;
    let quarter = usable / 4;
    let mut out = Vec::new();
    for k in 0..4 {
        let start = k * quarter;
        let end = if k == 3 { usable } else { (k + 1) * quarter };
        out.push((start, end));
    }
    Ok(out)
}

fn cpcv_style_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<Vec<Vec<(usize, usize)>>> {
    let usable = usable_len(data_cache, symbols)?;
    let block = usable / RESAMPLE_BLOCKS;
    let ranges: Vec<(usize, usize)> = (0..RESAMPLE_BLOCKS)
        .map(|i| {
            let start = i * block;
            let end = if i == RESAMPLE_BLOCKS - 1 {
                usable
            } else {
                (i + 1) * block
            };
            (start, end)
        })
        .collect();
    let mut out = Vec::new();
    for i in 0..RESAMPLE_BLOCKS {
        for j in i + 1..RESAMPLE_BLOCKS {
            out.push(vec![ranges[i], ranges[j]]);
        }
    }
    Ok(out)
}

fn write_snapshot(universes: &[UniverseSummary]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!("{}/trend_fee_surface_{}.md", SNAPSHOT_DIR, timestamp);
    let archive_csv = format!("{}/trend_fee_surface_{}.csv", SNAPSHOT_DIR, timestamp);

    let mut markdown = String::new();
    markdown.push_str("# Trend Fee Surface Audit Snapshot\n\n");
    markdown.push_str(&format!("- Timestamp (UTC): {}\n", Utc::now().to_rfc3339()));
    markdown.push_str(&format!("- Hold bars: {}\n", HOLD_BARS));
    markdown.push_str(
        "- Baseline execution: signal at close, next-open entry, next-open exit after fixed hold\n",
    );
    markdown.push_str("- Scenarios: Taker/Taker, MakerEntry/TakerExit, Maker/Maker\n\n");

    let mut csv = String::from("universe,strategy,scenario,full_return_pct,full_trades,win_rate_pct,wf_passed,wf_total,resample_passed,resample_total,wf_avg_return_pct,resample_avg_return_pct\n");

    for universe in universes {
        markdown.push_str(&format!("## {}\n\n", universe.label));
        markdown.push_str("| Strategy | Scenario | Full Return % | Trades | Win Rate % | WF | RS | WF AvgRet % | RS AvgRet % |\n");
        markdown.push_str("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
        for row in &universe.rows {
            markdown.push_str(&format!(
                "| {} | {} | {:.1} | {} | {:.1} | {}/{} | {}/{} | {:.1} | {:.1} |\n",
                row.strategy.name(),
                row.scenario.name(),
                row.full_return_pct,
                row.full_trades,
                row.win_rate_pct,
                row.wf_passed,
                row.wf_total,
                row.resample_passed,
                row.resample_total,
                row.wf_avg_return_pct,
                row.resample_avg_return_pct
            ));
            csv.push_str(&format!(
                "{},{},{},{:.4},{},{:.4},{},{},{},{},{:.4},{:.4}\n",
                universe.label,
                row.strategy.name(),
                row.scenario.name(),
                row.full_return_pct,
                row.full_trades,
                row.win_rate_pct,
                row.wf_passed,
                row.wf_total,
                row.resample_passed,
                row.resample_total,
                row.wf_avg_return_pct,
                row.resample_avg_return_pct
            ));
        }
        markdown.push('\n');
    }

    fs::write(SNAPSHOT_LATEST_MD, &markdown)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, markdown)?;
    fs::write(&archive_csv, csv)?;
    Ok((archive_md, archive_csv))
}

//! Head-to-head comparison harness for daily trend strategies.
//!
//! Goal: compare all serious trend candidates on the SAME universe,
//! SAME execution assumptions, and SAME walk-forward windows.
//!
//! Execution assumptions:
//! - Signal generated from bar i close and prior history only
//! - Entry at next bar open
//! - Exit at open after fixed 21-bar hold
//! - 0.1% taker fee charged on entry and exit
//!
//! This version adds harsher validation beyond simple quarter splits:
//! - Chronological block resampling (CPCV-style approximation)
//! - Symbol bootstrap / leave-one-out ranking stability
//! - Equity curve + drawdown PNG outputs for the top contenders

use anyhow::Result;
use chrono::Utc;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use plotters::prelude::*;
use polars::prelude::*;
use std::{collections::HashMap, fs};

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const STRESS_UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base6",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "Legacy5",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT"],
    ),
    ("LegacyCore4", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT"]),
    ("Majors4", &["BTCUSDT", "ETHUSDT", "BNBUSDT", "LTCUSDT"]),
    (
        "AltMix6",
        &[
            "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT",
        ],
    ),
    (
        "NoSOL",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "BNBUSDT",
        ],
    ),
    (
        "NoDOGE",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT",
        ],
    ),
    (
        "LargeCaps6",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT", "LTCUSDT",
        ],
    ),
    // Harsher, more legacy-skewed basket: no recent superstar alts (SOL/DOGE/ADA),
    // adds older 2017-cycle survivors that act as a simple delisting/survivorship proxy.
    (
        "OldGuard6",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
        ],
    ),
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const PERIOD: usize = 20;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 30;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const EQUITY_OUTPUT: &str = "charts/head_to_head_top2_equity.png";
const DRAWDOWN_OUTPUT: &str = "charts/head_to_head_top2_drawdown.png";
const FAILURE_ATTRIBUTION_OUTPUT: &str = "charts/macd_failure_pair_symbol_returns.png";
const MACD_REGIME_FAILURE_OUTPUT: &str = "charts/macd_regime_failure_pair_symbol_returns.png";
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/head_to_head_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/head_to_head_latest.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    MomentumBreakout,
    MaCross,
    Turtle,
    Macd,
    MacdRegime,
    TurtleRegime,
    TurtleMacd,
    TurtleRegimeMacd,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::MomentumBreakout => "MomentumBreakout",
            Self::MaCross => "MaCross50",
            Self::Turtle => "Turtle20",
            Self::Macd => "MACD",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegime => "Turtle+Regime",
            Self::TurtleMacd => "Turtle+MACD",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::MomentumBreakout,
            Self::MaCross,
            Self::Turtle,
            Self::Macd,
            Self::MacdRegime,
            Self::TurtleRegime,
            Self::TurtleMacd,
            Self::TurtleRegimeMacd,
        ]
    }
}

#[derive(Default, Clone, Debug)]
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

    fn add_assign(&mut self, other: &StrategyResult) {
        self.total_return_pct += other.total_return_pct;
        self.trades += other.trades;
        self.wins += other.wins;
    }
}

#[derive(Clone, Debug)]
struct SummaryRow {
    kind: StrategyKind,
    strategy: &'static str,
    full_return_pct: f64,
    full_trades: usize,
    full_win_rate: f64,
    wf_passed: usize,
    wf_total: usize,
    wf_avg_return_pct: f64,
    wf_avg_trades: f64,
    resample_passed: usize,
    resample_total: usize,
    resample_avg_return_pct: f64,
    resample_avg_trades: f64,
    loo_first_place: usize,
    bootstrap_first_place: usize,
    bootstrap_total: usize,
    universe_first_place: usize,
    universe_total: usize,
    composite_score: f64,
}

#[derive(Clone, Debug)]
struct ResampleDiagnostic {
    strategy: StrategyKind,
    test_blocks: Vec<usize>,
    total_return_pct: f64,
    trades: usize,
    passed: bool,
}

#[derive(Clone, Debug)]
struct UniverseStressRow {
    label: &'static str,
    symbols: &'static [&'static str],
    best: StrategyKind,
    best_return_pct: f64,
    second: StrategyKind,
    second_return_pct: f64,
}

#[derive(Clone, Debug)]
struct SymbolAttributionRow {
    symbol: &'static str,
    primary_return_pct: f64,
    primary_trades: usize,
    comparison_return_pct: f64,
    comparison_trades: usize,
    block_returns: Vec<f64>,
    block_trades: Vec<usize>,
}

#[derive(Clone, Debug)]
struct StrategyEval {
    result: StrategyResult,
    equity_curve: Vec<f64>,
    trade_returns: Vec<f64>,
}

impl StrategyEval {
    fn drawdown_curve(&self) -> Vec<f64> {
        let mut peak = 1.0f64;
        let mut out = Vec::with_capacity(self.equity_curve.len());
        for &equity in &self.equity_curve {
            peak = peak.max(equity);
            out.push((equity / peak - 1.0) * 100.0);
        }
        out
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== HEAD-TO-HEAD DAILY TREND COMPARISON ===\n");
    println!("Universe: {}", SYMBOLS.join(", "));
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Walk-forward: 4 equal chronological windows, pass = positive portfolio return + >= {} trades",
        MIN_TRADES_PER_WINDOW
    );
    println!(
        "Resampling: {} chronological blocks choose {} train / {} test (CPCV-style approximation)\n",
        RESAMPLE_BLOCKS,
        RESAMPLE_TRAIN_BLOCKS,
        RESAMPLE_BLOCKS - RESAMPLE_TRAIN_BLOCKS
    );

    let loader = DataLoader::new(None, None);
    let mut data_cache = HashMap::<String, DataFrame>::new();
    let mut load_symbols: Vec<&str> = SYMBOLS.to_vec();
    for &(_, universe_symbols) in STRESS_UNIVERSES {
        for &symbol in universe_symbols {
            if !load_symbols.contains(&symbol) {
                load_symbols.push(symbol);
            }
        }
    }

    for symbol in load_symbols {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", df.height());
        data_cache.insert(symbol.to_string(), df);
    }

    let mut rows = Vec::new();
    let mut full_evals = HashMap::<StrategyKind, StrategyEval>::new();
    let universe_stress = universe_stress_test(&data_cache)?;
    let resample_sets = cpcv_style_windows(&data_cache)?;
    let mut diagnostics = Vec::<ResampleDiagnostic>::new();

    for strategy in StrategyKind::all() {
        let full_eval = evaluate_strategy(&data_cache, *strategy, SYMBOLS)?;
        full_evals.insert(*strategy, full_eval.clone());

        let quarter_windows = quarter_windows(&data_cache)?;
        let mut wf_passed = 0usize;
        let mut wf_total = 0usize;
        let mut wf_return_sum = 0.0;
        let mut wf_trade_sum = 0usize;

        for (start, end) in &quarter_windows {
            let eval = evaluate_strategy_on_slice(&data_cache, *strategy, SYMBOLS, *start, *end)?;
            wf_total += 1;
            wf_return_sum += eval.result.total_return_pct;
            wf_trade_sum += eval.result.trades;
            if eval.result.total_return_pct > 0.0 && eval.result.trades >= MIN_TRADES_PER_WINDOW {
                wf_passed += 1;
            }
        }

        let mut resample_passed = 0usize;
        let mut resample_return_sum = 0.0;
        let mut resample_trade_sum = 0usize;

        for window_set in &resample_sets {
            let eval = evaluate_strategy_on_windows(&data_cache, *strategy, SYMBOLS, window_set)?;
            let test_blocks = window_set
                .iter()
                .map(|(start, _)| {
                    start / (min_rows(&data_cache).unwrap_or(1) / RESAMPLE_BLOCKS).max(1)
                })
                .collect();
            let passed =
                eval.result.total_return_pct > 0.0 && eval.result.trades >= MIN_TRADES_PER_WINDOW;
            diagnostics.push(ResampleDiagnostic {
                strategy: *strategy,
                test_blocks,
                total_return_pct: eval.result.total_return_pct,
                trades: eval.result.trades,
                passed,
            });
            resample_return_sum += eval.result.total_return_pct;
            resample_trade_sum += eval.result.trades;
            if passed {
                resample_passed += 1;
            }
        }

        let loo_first_place = leave_one_out_rank_wins(&data_cache, *strategy)?;
        let bootstrap_first_place = bootstrap_rank_wins(&data_cache, *strategy, 64)?;

        let universe_first_place = universe_stress
            .iter()
            .filter(|row| row.best == *strategy)
            .count();
        let composite_score = composite_score(
            full_eval.result.total_return_pct,
            wf_passed,
            wf_total,
            resample_passed,
            resample_sets.len(),
            universe_first_place,
            universe_stress.len(),
            loo_first_place,
            SYMBOLS.len(),
            bootstrap_first_place,
            64,
            full_eval.result.trades,
        );

        rows.push(SummaryRow {
            kind: *strategy,
            strategy: strategy.name(),
            full_return_pct: full_eval.result.total_return_pct,
            full_trades: full_eval.result.trades,
            full_win_rate: full_eval.result.win_rate(),
            wf_passed,
            wf_total,
            wf_avg_return_pct: wf_return_sum / wf_total as f64,
            wf_avg_trades: wf_trade_sum as f64 / wf_total as f64,
            resample_passed,
            resample_total: resample_sets.len(),
            resample_avg_return_pct: resample_return_sum / resample_sets.len() as f64,
            resample_avg_trades: resample_trade_sum as f64 / resample_sets.len() as f64,
            loo_first_place,
            bootstrap_first_place,
            bootstrap_total: 64,
            universe_first_place,
            universe_total: universe_stress.len(),
            composite_score,
        });
    }

    rows.sort_by(|a, b| {
        b.composite_score
            .partial_cmp(&a.composite_score)
            .unwrap()
            .then_with(|| b.resample_passed.cmp(&a.resample_passed))
            .then_with(|| b.universe_first_place.cmp(&a.universe_first_place))
            .then_with(|| b.bootstrap_first_place.cmp(&a.bootstrap_first_place))
            .then_with(|| b.loo_first_place.cmp(&a.loo_first_place))
            .then_with(|| b.full_return_pct.partial_cmp(&a.full_return_pct).unwrap())
    });

    println!("\n{:-<176}", "");
    println!(
        "{:<22} {:>8} {:>10} {:>8} {:>8} {:>11} {:>11} {:>9} {:>11} {:>11} {:>9} {:>11} {:>8}",
        "Strategy",
        "Score",
        "FullRet%",
        "Trades",
        "WF",
        "RS",
        "RS AvgRet%",
        "Uni #1",
        "LOO #1",
        "Boot #1",
        "WinRate",
        "TradeMean",
        "RS AvgT"
    );
    println!("{:-<176}", "");
    for row in &rows {
        println!(
            "{:<22} {:>8.3} {:>10.1} {:>8} {:>4}/{} {:>4}/{} {:>11.1} {:>7}/{} {:>7}/{} {:>7}/{} {:>8.1}% {:>11.1} {:>8.1}",
            row.strategy,
            row.composite_score,
            row.full_return_pct,
            row.full_trades,
            row.wf_passed,
            row.wf_total,
            row.resample_passed,
            row.resample_total,
            row.resample_avg_return_pct,
            row.universe_first_place,
            row.universe_total,
            row.loo_first_place,
            SYMBOLS.len(),
            row.bootstrap_first_place,
            row.bootstrap_total,
            row.full_win_rate * 100.0,
            row.wf_avg_trades,
            row.resample_avg_trades,
        );
    }
    println!("{:-<176}", "");

    if let Some(best) = rows.first() {
        println!("\nBest by this harsher harness: {}", best.strategy);
    }

    println!("\nPer-quarter portfolio returns:");
    for strategy in StrategyKind::all() {
        print!("  {:<22}", strategy.name());
        for (window_idx, (start, end)) in quarter_windows(&data_cache)?.iter().enumerate() {
            let eval = evaluate_strategy_on_slice(&data_cache, *strategy, SYMBOLS, *start, *end)?;
            print!(
                " W{}={:>7.1}%/{}tr",
                window_idx + 1,
                eval.result.total_return_pct,
                eval.result.trades
            );
        }
        println!();
    }

    println!("\nUniverse stress test winners:");
    for stress in &universe_stress {
        println!(
            "- {:<8} [{}] -> {} ({:.1}%) over {} ({:.1}%)",
            stress.label,
            stress.symbols.join(", "),
            stress.best.name(),
            stress.best_return_pct,
            stress.second.name(),
            stress.second_return_pct,
        );
    }

    println!("\nTop 3 resample summaries:");
    for row in rows.iter().take(3) {
        println!(
            "- {}: score {:.3}, resample {}/{} positive, universe #1 in {}/{}, bootstrap #1 in {}/{}, leave-one-out #1 in {}/{} universes",
            row.strategy,
            row.composite_score,
            row.resample_passed,
            row.resample_total,
            row.universe_first_place,
            row.universe_total,
            row.bootstrap_first_place,
            row.bootstrap_total,
            row.loo_first_place,
            SYMBOLS.len()
        );
    }

    println!("\nFailing chronological resamples:");
    for strategy in StrategyKind::all() {
        let failures: Vec<&ResampleDiagnostic> = diagnostics
            .iter()
            .filter(|d| d.strategy == *strategy && !d.passed)
            .collect();
        if failures.is_empty() {
            println!("- {:<22} none", strategy.name());
        } else {
            for failure in failures {
                println!(
                    "- {:<22} test blocks {:?}: return {:+.1}% across {} trades",
                    strategy.name(),
                    failure.test_blocks,
                    failure.total_return_pct,
                    failure.trades
                );
            }
        }
    }

    let macd_failure_blocks = [0usize, 2usize];
    let attribution = strategy_pair_symbol_attribution(
        &data_cache,
        StrategyKind::Macd,
        StrategyKind::TurtleRegimeMacd,
        &macd_failure_blocks,
    )?;
    print_symbol_attribution_table(
        "MACD failing pair attribution",
        StrategyKind::Macd,
        StrategyKind::TurtleRegimeMacd,
        &macd_failure_blocks,
        &attribution,
    );
    render_failure_attribution_chart(
        &attribution,
        FAILURE_ATTRIBUTION_OUTPUT,
        "MACD chronology miss attribution — test blocks [0,2]",
        StrategyKind::Macd.name(),
        StrategyKind::TurtleRegimeMacd.name(),
    )?;

    let macd_regime_failure_blocks = [2usize, 3usize];
    let macd_regime_attribution = strategy_pair_symbol_attribution(
        &data_cache,
        StrategyKind::MacdRegime,
        StrategyKind::Macd,
        &macd_regime_failure_blocks,
    )?;
    print_symbol_attribution_table(
        "MACD+Regime failing pair attribution",
        StrategyKind::MacdRegime,
        StrategyKind::Macd,
        &macd_regime_failure_blocks,
        &macd_regime_attribution,
    );
    render_failure_attribution_chart(
        &macd_regime_attribution,
        MACD_REGIME_FAILURE_OUTPUT,
        "MACD+Regime chronology miss attribution — test blocks [2,3]",
        StrategyKind::MacdRegime.name(),
        StrategyKind::Macd.name(),
    )?;

    if rows.len() >= 2 {
        let top_two = [&rows[0], &rows[1]];
        render_top_two_charts(&full_evals, &top_two)?;
        println!("\nCharts written:");
        println!("- {}", EQUITY_OUTPUT);
        println!("- {}", DRAWDOWN_OUTPUT);
        println!("- {}", FAILURE_ATTRIBUTION_OUTPUT);
        println!("- {}", MACD_REGIME_FAILURE_OUTPUT);
    }

    let snapshot_paths = write_benchmark_snapshot(&rows, &universe_stress)?;
    println!("\nBenchmark snapshots written:");
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);
    println!("- {}", snapshot_paths.0);
    println!("- {}", snapshot_paths.1);

    println!("\nInterpretation:");
    println!("- Score = composite ranking score (chronology 35%, universe stress 20%, bootstrap 15%, leave-one-out 10%, walk-forward 10%, full return 5%, trade depth 5%)");
    println!("- WF = positive quarter-split windows with enough trades");
    println!(
        "- RS = positive chronological resamples with enough trades (CPCV-style approximation)"
    );
    println!("- Uni #1 = how often strategy ranks first across explicit stress universes");
    println!("- LOO #1 = how often strategy ranks first when each symbol is removed once");
    println!("- Boot #1 = rank-first count across deterministic symbol bootstrap baskets");
    println!("- Same hold / fees / execution assumptions for every strategy");

    Ok(())
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
    symbols: &[&str],
) -> Result<StrategyEval> {
    let mut aggregate = StrategyEval {
        result: StrategyResult::default(),
        equity_curve: vec![1.0],
        trade_returns: Vec::new(),
    };

    for symbol in symbols {
        let df = data_cache.get(&symbol.to_string()).unwrap();
        let eval = run_strategy(df, strategy)?;
        aggregate.result.add_assign(&eval.result);
        if aggregate.equity_curve.len() < eval.equity_curve.len() {
            aggregate.equity_curve.resize(
                eval.equity_curve.len(),
                *aggregate.equity_curve.last().unwrap_or(&1.0),
            );
        }
        for (idx, value) in eval.equity_curve.iter().enumerate() {
            aggregate.equity_curve[idx] += value - 1.0;
        }
        aggregate.trade_returns.extend(eval.trade_returns);
    }

    if !symbols.is_empty() {
        for equity in &mut aggregate.equity_curve {
            *equity = 1.0 + (*equity - 1.0) / symbols.len() as f64;
        }
    }

    Ok(aggregate)
}

fn evaluate_strategy_on_slice(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
    symbols: &[&str],
    start: usize,
    end: usize,
) -> Result<StrategyEval> {
    let mut aggregate = StrategyEval {
        result: StrategyResult::default(),
        equity_curve: vec![1.0],
        trade_returns: Vec::new(),
    };

    for symbol in symbols {
        let df = data_cache.get(&symbol.to_string()).unwrap();
        let eval = run_strategy_in_window(df, strategy, start, end)?;
        aggregate.result.add_assign(&eval.result);
        aggregate.trade_returns.extend(eval.trade_returns);
    }

    Ok(aggregate)
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
    symbols: &[&str],
    windows: &[(usize, usize)],
) -> Result<StrategyEval> {
    let mut aggregate = StrategyEval {
        result: StrategyResult::default(),
        equity_curve: vec![1.0],
        trade_returns: Vec::new(),
    };

    for symbol in symbols {
        let df = data_cache.get(&symbol.to_string()).unwrap();
        for &(start, end) in windows {
            let eval = run_strategy_in_window(df, strategy, start, end)?;
            aggregate.result.add_assign(&eval.result);
            aggregate.trade_returns.extend(eval.trade_returns);
        }
    }

    Ok(aggregate)
}

fn quarter_windows(data_cache: &HashMap<String, DataFrame>) -> Result<Vec<(usize, usize)>> {
    let n = min_rows(data_cache)?;
    let quarter = n / 4;
    let mut windows = Vec::new();
    for window_idx in 0..4usize {
        let start = window_idx * quarter;
        let end = if window_idx == 3 {
            n
        } else {
            (window_idx + 1) * quarter
        };
        windows.push((start, end));
    }
    Ok(windows)
}

fn cpcv_base_blocks(data_cache: &HashMap<String, DataFrame>) -> Result<Vec<(usize, usize)>> {
    let n = min_rows(data_cache)?;
    let block = n / RESAMPLE_BLOCKS;
    let mut base = Vec::new();
    for block_idx in 0..RESAMPLE_BLOCKS {
        let start = block_idx * block;
        let end = if block_idx == RESAMPLE_BLOCKS - 1 {
            n
        } else {
            (block_idx + 1) * block
        };
        base.push((start, end));
    }
    Ok(base)
}

fn cpcv_style_windows(data_cache: &HashMap<String, DataFrame>) -> Result<Vec<Vec<(usize, usize)>>> {
    let base = cpcv_base_blocks(data_cache)?;
    let combos = choose_indices(RESAMPLE_BLOCKS, RESAMPLE_TRAIN_BLOCKS);
    let mut windows = Vec::new();
    for combo in combos {
        let mut test_windows = Vec::new();
        for (idx, window) in base.iter().enumerate() {
            if !combo.contains(&idx) {
                test_windows.push(*window);
            }
        }
        windows.push(test_windows);
    }
    Ok(windows)
}

fn universe_stress_test(data_cache: &HashMap<String, DataFrame>) -> Result<Vec<UniverseStressRow>> {
    let mut out = Vec::new();

    for &(label, symbols) in STRESS_UNIVERSES {
        let mut scored = Vec::new();
        for strategy in StrategyKind::all() {
            let eval = evaluate_strategy(data_cache, *strategy, symbols)?;
            scored.push((*strategy, eval.result.total_return_pct));
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        out.push(UniverseStressRow {
            label,
            symbols,
            best: scored[0].0,
            best_return_pct: scored[0].1,
            second: scored[1].0,
            second_return_pct: scored[1].1,
        });
    }

    Ok(out)
}

fn leave_one_out_rank_wins(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
) -> Result<usize> {
    let mut wins = 0usize;
    for excluded_idx in 0..SYMBOLS.len() {
        let subset: Vec<&str> = SYMBOLS
            .iter()
            .enumerate()
            .filter_map(|(idx, symbol)| {
                if idx != excluded_idx {
                    Some(*symbol)
                } else {
                    None
                }
            })
            .collect();
        let best = best_strategy_for_symbols(data_cache, &subset)?;
        if best == strategy {
            wins += 1;
        }
    }
    Ok(wins)
}

fn bootstrap_rank_wins(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
    iterations: usize,
) -> Result<usize> {
    let mut wins = 0usize;
    let mut state = 0x9E37_79B9_7F4A_7C15u64;

    for _ in 0..iterations {
        let subset: Vec<&str> = (0..SYMBOLS.len())
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let idx = ((state >> 32) as usize) % SYMBOLS.len();
                SYMBOLS[idx]
            })
            .collect();
        let best = best_strategy_for_symbols(data_cache, &subset)?;
        if best == strategy {
            wins += 1;
        }
    }
    Ok(wins)
}

fn best_strategy_for_symbols(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<StrategyKind> {
    let mut scored = Vec::new();
    for strategy in StrategyKind::all() {
        let eval = evaluate_strategy(data_cache, *strategy, symbols)?;
        let resamples = cpcv_style_windows(data_cache)?;
        let mut pass = 0usize;
        let mut avg_ret = 0.0;
        for windows in &resamples {
            let eval_resample =
                evaluate_strategy_on_windows(data_cache, *strategy, symbols, windows)?;
            if eval_resample.result.total_return_pct > 0.0
                && eval_resample.result.trades >= MIN_TRADES_PER_WINDOW
            {
                pass += 1;
            }
            avg_ret += eval_resample.result.total_return_pct;
        }
        scored.push((
            *strategy,
            pass,
            avg_ret / resamples.len() as f64,
            eval.result.total_return_pct,
        ));
    }

    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.partial_cmp(&a.2).unwrap())
            .then_with(|| b.3.partial_cmp(&a.3).unwrap())
    });
    Ok(scored[0].0)
}

fn min_rows(data_cache: &HashMap<String, DataFrame>) -> Result<usize> {
    data_cache
        .values()
        .map(|df| df.height())
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty data cache"))
}

fn composite_score(
    full_return_pct: f64,
    wf_passed: usize,
    wf_total: usize,
    resample_passed: usize,
    resample_total: usize,
    universe_first_place: usize,
    universe_total: usize,
    loo_first_place: usize,
    loo_total: usize,
    bootstrap_first_place: usize,
    bootstrap_total: usize,
    trades: usize,
) -> f64 {
    let return_score = (full_return_pct.max(0.0) / 4000.0).min(1.0);
    let wf_score = wf_passed as f64 / wf_total.max(1) as f64;
    let resample_score = resample_passed as f64 / resample_total.max(1) as f64;
    let universe_score = universe_first_place as f64 / universe_total.max(1) as f64;
    let loo_score = loo_first_place as f64 / loo_total.max(1) as f64;
    let bootstrap_score = bootstrap_first_place as f64 / bootstrap_total.max(1) as f64;
    let trade_depth_score = (trades as f64 / 700.0).min(1.0);

    0.35 * resample_score
        + 0.20 * universe_score
        + 0.15 * bootstrap_score
        + 0.10 * loo_score
        + 0.10 * wf_score
        + 0.05 * return_score
        + 0.05 * trade_depth_score
}

fn choose_indices(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn rec(start: usize, n: usize, k: usize, current: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if current.len() == k {
            out.push(current.clone());
            return;
        }
        for idx in start..n {
            current.push(idx);
            rec(idx + 1, n, k, current, out);
            current.pop();
        }
    }

    let mut out = Vec::new();
    rec(0, n, k, &mut Vec::new(), &mut out);
    out
}

fn run_strategy(df: &DataFrame, strategy: StrategyKind) -> Result<StrategyEval> {
    let signals = generate_signals(df, strategy)?;
    backtest_fixed_hold_next_open(df, &signals)
}

fn run_strategy_in_window(
    df: &DataFrame,
    strategy: StrategyKind,
    start: usize,
    end: usize,
) -> Result<StrategyEval> {
    let signals = generate_signals(df, strategy)?;
    backtest_fixed_hold_next_open_window(df, &signals, start, end)
}

fn generate_signals(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::MomentumBreakout => generate_momentum_breakout(df),
        StrategyKind::MaCross => generate_ma_cross(df),
        StrategyKind::Turtle => generate_turtle_signals(df, PERIOD),
        StrategyKind::Macd => generate_macd_signals(df),
        StrategyKind::MacdRegime => generate_macd_regime_signals(df),
        StrategyKind::TurtleRegime => generate_turtle_regime_signals(df, PERIOD),
        StrategyKind::TurtleMacd => generate_turtle_macd_signals(df, PERIOD),
        StrategyKind::TurtleRegimeMacd => generate_turtle_regime_macd_signals(df, PERIOD),
    }
}

fn backtest_fixed_hold_next_open(df: &DataFrame, signals: &[i32]) -> Result<StrategyEval> {
    backtest_fixed_hold_next_open_window(df, signals, 0, df.height())
}

fn backtest_fixed_hold_next_open_window(
    df: &DataFrame,
    signals: &[i32],
    start: usize,
    end: usize,
) -> Result<StrategyEval> {
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let mut trade_returns = Vec::new();
    let mut equity_curve = vec![1.0f64];
    let mut equity = 1.0f64;
    let mut i = 200usize.max(start);
    let end = end.min(n);

    while i + HOLD_BARS + 1 < end {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }

        let entry_idx = i + 1;
        let exit_idx = i + 1 + HOLD_BARS;
        if exit_idx >= end {
            break;
        }
        let entry = match open.get(entry_idx) {
            Some(v) if v > 0.0 => v,
            _ => {
                i += 1;
                continue;
            }
        };
        let exit = match open.get(exit_idx) {
            Some(v) if v > 0.0 => v,
            _ => {
                i += 1;
                continue;
            }
        };

        let gross = if signal > 0 {
            (exit / entry - 1.0) * 100.0
        } else {
            (entry / exit - 1.0) * 100.0
        };
        let net = gross - 2.0 * TAKER_FEE * 100.0;
        trade_returns.push(net);
        equity *= 1.0 + net / 100.0;
        equity_curve.push(equity);
        i = exit_idx;
    }

    let trades = trade_returns.len();
    let wins = trade_returns.iter().filter(|&&r| r > 0.0).count();
    Ok(StrategyEval {
        result: StrategyResult {
            total_return_pct: trade_returns.iter().sum(),
            trades,
            wins,
        },
        equity_curve,
        trade_returns,
    })
}

fn generate_momentum_breakout(df: &DataFrame) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let n = close.len();
    let period = 20usize;
    let mut signals = vec![0i32; n];

    for i in period..n {
        let highest = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let price = close.get(i).unwrap_or(0.0);
        if price > highest && price > 0.0 {
            signals[i] = 1;
        }
    }
    Ok(signals)
}

fn generate_ma_cross(df: &DataFrame) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let sma_50 = calculate_sma(&close, 50)?;
    let n = close.len();
    let mut signals = vec![0i32; n];

    for i in 51..n {
        let prev_price = close.get(i - 1).unwrap_or(0.0);
        let curr_price = close.get(i).unwrap_or(0.0);
        let prev_ma = sma_50[i - 1].unwrap_or(0.0);
        let curr_ma = sma_50[i].unwrap_or(0.0);

        if prev_price <= prev_ma && curr_price > curr_ma && curr_price > 0.0 {
            signals[i] = 1;
        } else if prev_price >= prev_ma && curr_price < curr_ma && curr_price > 0.0 {
            signals[i] = -1;
        }
    }
    Ok(signals)
}

fn generate_turtle_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();
    let mut signals = vec![0i32; n];

    for i in period..n {
        let period_high = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let period_low = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let current_close = close.get(i).unwrap_or(0.0);

        if current_close > period_high {
            signals[i] = 1;
        } else if current_close < period_low {
            signals[i] = -1;
        }
    }

    Ok(signals)
}

fn generate_macd_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = df.column("macd").ok().and_then(|s| s.f64().ok());
    let signal = df.column("macd_signal").ok().and_then(|s| s.f64().ok());
    let close = df.column("close")?.f64()?;
    let n = close.len();
    let mut signals = vec![0i32; n];

    if let (Some(macd_series), Some(signal_series)) = (macd, signal) {
        for i in 1..n {
            let macd_curr = macd_series.get(i).unwrap_or(0.0);
            let sig_curr = signal_series.get(i).unwrap_or(0.0);
            if macd_curr > sig_curr {
                signals[i] = 1;
            } else if macd_curr < sig_curr {
                signals[i] = -1;
            }
        }
    } else {
        let ema_12 = calculate_ema(&close, 12)?;
        let ema_26 = calculate_ema(&close, 26)?;
        for i in 26..n {
            let fast = ema_12[i].unwrap_or(0.0);
            let slow = ema_26[i].unwrap_or(0.0);
            if fast > slow {
                signals[i] = 1;
            } else if fast < slow {
                signals[i] = -1;
            }
        }
    }
    Ok(signals)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = generate_macd_signals(df)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200)?;
    let mut out = vec![0i32; macd.len()];

    for i in 0..macd.len() {
        let sig = macd[i];
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200[i].unwrap_or(0.0);
        if sig > 0 && price > sma {
            out[i] = 1;
        } else if sig < 0 && price < sma {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_signals(df, period)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200)?;
    let mut out = vec![0i32; turtle.len()];

    for i in 0..turtle.len() {
        let sig = turtle[i];
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200[i].unwrap_or(0.0);
        if sig > 0 && price > sma {
            out[i] = 1;
        } else if sig < 0 {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_signals(df, period)?;
    let macd = generate_macd_signals(df)?;
    let mut out = vec![0i32; turtle.len()];

    for i in 0..turtle.len() {
        let t = turtle[i];
        let m = macd[i];
        if t > 0 && m > 0 {
            out[i] = 1;
        } else if t < 0 && m < 0 {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle_regime = generate_turtle_regime_signals(df, period)?;
    let macd = generate_macd_signals(df)?;
    let mut out = vec![0i32; turtle_regime.len()];

    for i in 0..turtle_regime.len() {
        let t = turtle_regime[i];
        let m = macd[i];
        if t > 0 && m > 0 {
            out[i] = 1;
        } else if t < 0 && m < 0 {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn calculate_sma(series: &ChunkedArray<Float64Type>, period: usize) -> Result<Vec<Option<f64>>> {
    let n = series.len();
    let mut sma = vec![None; n];
    for i in period..n {
        let sum: f64 = (0..period).filter_map(|j| series.get(i - j)).sum();
        sma[i] = Some(sum / period as f64);
    }
    Ok(sma)
}

fn calculate_ema(series: &ChunkedArray<Float64Type>, period: usize) -> Result<Vec<Option<f64>>> {
    let n = series.len();
    let mut ema = vec![None; n];
    let multiplier = 2.0 / (period as f64 + 1.0);

    if n >= period {
        let sum: f64 = (0..period).filter_map(|j| series.get(j)).sum();
        ema[period - 1] = Some(sum / period as f64);
        for i in period..n {
            if let (Some(prev_ema), Some(curr_price)) = (ema[i - 1], series.get(i)) {
                ema[i] = Some((curr_price - prev_ema) * multiplier + prev_ema);
            }
        }
    }

    Ok(ema)
}

fn strategy_pair_symbol_attribution(
    data_cache: &HashMap<String, DataFrame>,
    primary: StrategyKind,
    comparison: StrategyKind,
    block_indices: &[usize],
) -> Result<Vec<SymbolAttributionRow>> {
    let base_blocks = cpcv_base_blocks(data_cache)?;
    let mut rows = Vec::new();

    for &symbol in SYMBOLS {
        let mut block_returns = Vec::new();
        let mut block_trades = Vec::new();
        let mut primary_total_return = 0.0;
        let mut primary_total_trades = 0usize;

        for &block_idx in block_indices {
            let (start, end) = base_blocks[block_idx];
            let primary_eval =
                evaluate_strategy_on_slice(data_cache, primary, &[symbol], start, end)?;
            primary_total_return += primary_eval.result.total_return_pct;
            primary_total_trades += primary_eval.result.trades;
            block_returns.push(primary_eval.result.total_return_pct);
            block_trades.push(primary_eval.result.trades);
        }

        let windows: Vec<(usize, usize)> =
            block_indices.iter().map(|&idx| base_blocks[idx]).collect();
        let comparison_eval =
            evaluate_strategy_on_windows(data_cache, comparison, &[symbol], &windows)?;

        rows.push(SymbolAttributionRow {
            symbol,
            primary_return_pct: primary_total_return,
            primary_trades: primary_total_trades,
            comparison_return_pct: comparison_eval.result.total_return_pct,
            comparison_trades: comparison_eval.result.trades,
            block_returns,
            block_trades,
        });
    }

    rows.sort_by(|a, b| {
        a.primary_return_pct
            .partial_cmp(&b.primary_return_pct)
            .unwrap()
    });
    Ok(rows)
}

fn print_symbol_attribution_table(
    title: &str,
    primary: StrategyKind,
    comparison: StrategyKind,
    block_indices: &[usize],
    rows: &[SymbolAttributionRow],
) {
    println!("\n{} (test blocks {:?}):", title, block_indices);
    println!(
        "{:<10} {:>11} {:>8} {:>11} {:>8} {:>12} {:>12}",
        "Symbol",
        format!("{} Ret%", primary.name()),
        "Trades",
        format!("{} Ret%", comparison.name()),
        "Trades",
        format!("Block{} Ret%", block_indices[0]),
        format!("Block{} Ret%", block_indices[1]),
    );
    for row in rows {
        println!(
            "{:<10} {:+11.1} {:>8} {:+11.1} {:>8} {:+12.1} {:+12.1}",
            row.symbol,
            row.primary_return_pct,
            row.primary_trades,
            row.comparison_return_pct,
            row.comparison_trades,
            row.block_returns[0],
            row.block_returns[1],
        );
    }

    let worst = rows
        .iter()
        .min_by(|a, b| {
            a.primary_return_pct
                .partial_cmp(&b.primary_return_pct)
                .unwrap()
        })
        .unwrap();
    let best = rows
        .iter()
        .max_by(|a, b| {
            a.primary_return_pct
                .partial_cmp(&b.primary_return_pct)
                .unwrap()
        })
        .unwrap();
    println!(
        "Worst {} contributor: {} ({:+.1}% across {} trades). Best offset: {} ({:+.1}% across {} trades).",
        primary.name(),
        worst.symbol,
        worst.primary_return_pct,
        worst.primary_trades,
        best.symbol,
        best.primary_return_pct,
        best.primary_trades,
    );
}

fn render_failure_attribution_chart(
    rows: &[SymbolAttributionRow],
    path: &str,
    title: &str,
    primary_label: &str,
    comparison_label: &str,
) -> Result<()> {
    std::fs::create_dir_all("charts")?;

    let root = BitMapBackend::new(path, (1600, 900)).into_drawing_area();
    root.fill(&WHITE)?;

    let y_min = rows
        .iter()
        .flat_map(|row| [row.primary_return_pct, row.comparison_return_pct])
        .fold(f64::INFINITY, f64::min)
        .min(-10.0);
    let y_max = rows
        .iter()
        .flat_map(|row| [row.primary_return_pct, row.comparison_return_pct])
        .fold(f64::NEG_INFINITY, f64::max)
        .max(10.0);

    let mut chart = ChartBuilder::on(&root)
        .caption(title, ("sans-serif", 32))
        .margin(20)
        .x_label_area_size(60)
        .y_label_area_size(70)
        .build_cartesian_2d(0f64..rows.len() as f64, (y_min - 5.0)..(y_max + 5.0))?;

    chart
        .configure_mesh()
        .x_desc("Symbol")
        .y_desc("Return % on failing resample")
        .x_labels(rows.len())
        .x_label_formatter(&|x| {
            let idx = (*x).floor() as usize;
            rows.get(idx)
                .map(|row| row.symbol.to_string())
                .unwrap_or_default()
        })
        .draw()?;

    for (idx, row) in rows.iter().enumerate() {
        let x = idx as f64;
        chart.draw_series(std::iter::once(Rectangle::new(
            [(x + 0.05, 0.0), (x + 0.40, row.primary_return_pct)],
            RED.mix(0.7).filled(),
        )))?;
        chart.draw_series(std::iter::once(Rectangle::new(
            [(x + 0.45, 0.0), (x + 0.80, row.comparison_return_pct)],
            BLUE.mix(0.7).filled(),
        )))?;
    }

    chart
        .draw_series(std::iter::once(PathElement::new(
            vec![(0.0, 0.0), (rows.len() as f64, 0.0)],
            BLACK,
        )))?
        .label("0%")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], BLACK));

    chart
        .draw_series(std::iter::once(EmptyElement::at((0.0, y_max + 4.0))))?
        .label(primary_label)
        .legend(|(x, y)| Rectangle::new([(x, y - 5), (x + 12, y + 5)], RED.mix(0.7).filled()));
    chart
        .draw_series(std::iter::once(EmptyElement::at((0.0, y_max + 4.0))))?
        .label(comparison_label)
        .legend(|(x, y)| Rectangle::new([(x, y - 5), (x + 12, y + 5)], BLUE.mix(0.7).filled()));

    chart.configure_series_labels().border_style(BLACK).draw()?;
    root.present()?;
    Ok(())
}

fn render_top_two_charts(
    evals: &HashMap<StrategyKind, StrategyEval>,
    rows: &[&SummaryRow; 2],
) -> Result<()> {
    std::fs::create_dir_all("charts")?;

    let a = evals.get(&rows[0].kind).unwrap();
    let b = evals.get(&rows[1].kind).unwrap();

    render_line_chart(
        EQUITY_OUTPUT,
        "Head-to-Head Top 2 Equity Curves",
        &[
            (&a.equity_curve, rows[0].strategy, RED),
            (&b.equity_curve, rows[1].strategy, BLUE),
        ],
        false,
    )?;

    let drawdown_a = a.drawdown_curve();
    let drawdown_b = b.drawdown_curve();
    render_line_chart(
        DRAWDOWN_OUTPUT,
        "Head-to-Head Top 2 Drawdowns",
        &[
            (&drawdown_a, rows[0].strategy, RED),
            (&drawdown_b, rows[1].strategy, BLUE),
        ],
        true,
    )?;

    Ok(())
}

fn render_line_chart(
    path: &str,
    title: &str,
    series: &[(&Vec<f64>, &str, RGBColor)],
    allow_negative: bool,
) -> Result<()> {
    let root = BitMapBackend::new(path, (1400, 900)).into_drawing_area();
    root.fill(&WHITE)?;

    let x_max = series
        .iter()
        .map(|(values, _, _)| values.len())
        .max()
        .unwrap_or(1)
        .max(2);
    let y_min = series
        .iter()
        .flat_map(|(values, _, _)| values.iter().copied())
        .fold(f64::INFINITY, f64::min);
    let y_max = series
        .iter()
        .flat_map(|(values, _, _)| values.iter().copied())
        .fold(f64::NEG_INFINITY, f64::max);

    let lower = if allow_negative {
        y_min.min(-5.0)
    } else {
        y_min.min(0.95)
    };
    let upper = y_max.max(if allow_negative { 1.0 } else { 1.05 });

    let mut chart = ChartBuilder::on(&root)
        .caption(title, ("sans-serif", 32))
        .margin(20)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(0..x_max, lower..upper)?;

    chart
        .configure_mesh()
        .x_desc("Trade Index")
        .y_desc(if allow_negative {
            "Drawdown %"
        } else {
            "Equity"
        })
        .draw()?;

    for (values, label, color) in series {
        chart
            .draw_series(LineSeries::new(
                values.iter().enumerate().map(|(idx, value)| (idx, *value)),
                *color,
            ))?
            .label(*label)
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 30, y)], *color));
    }

    chart.configure_series_labels().border_style(BLACK).draw()?;
    root.present()?;
    Ok(())
}

fn write_benchmark_snapshot(
    rows: &[SummaryRow],
    universe_stress: &[UniverseStressRow],
) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;

    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!("{}/head_to_head_{}.md", SNAPSHOT_DIR, timestamp);
    let archive_csv = format!("{}/head_to_head_{}.csv", SNAPSHOT_DIR, timestamp);

    let mut markdown = String::new();
    markdown.push_str("# Head-to-Head Trend Benchmark Snapshot\n\n");
    markdown.push_str(&format!("- Timestamp (UTC): {}\n", Utc::now().to_rfc3339()));
    markdown.push_str(&format!("- Base universe: {}\n", SYMBOLS.join(", ")));
    markdown.push_str(&format!("- Stress universes: {}\n", STRESS_UNIVERSES.len()));
    markdown.push_str(&format!("- Candles: {}\n", CANDLES));
    markdown.push_str(&format!("- Hold bars: {}\n", HOLD_BARS));
    markdown.push_str(&format!("- Fee each side: {:.3}%\n", TAKER_FEE * 100.0));
    markdown.push_str(&format!(
        "- Resampling: {} blocks, {} train / {} test\n\n",
        RESAMPLE_BLOCKS,
        RESAMPLE_TRAIN_BLOCKS,
        RESAMPLE_BLOCKS - RESAMPLE_TRAIN_BLOCKS
    ));
    markdown.push_str("## Ranking table\n\n");
    markdown.push_str("| Rank | Strategy | Score | Full Return % | Trades | WF | Resamples | Universe #1 | LOO #1 | Bootstrap #1 | Win Rate % |\n");
    markdown.push_str("|------|----------|------:|--------------:|-------:|----|-----------|-------------|--------|--------------|-----------:|\n");
    for (idx, row) in rows.iter().enumerate() {
        markdown.push_str(&format!(
            "| {} | {} | {:.3} | {:.1} | {} | {}/{} | {}/{} | {}/{} | {}/{} | {}/{} | {:.1} |\n",
            idx + 1,
            row.strategy,
            row.composite_score,
            row.full_return_pct,
            row.full_trades,
            row.wf_passed,
            row.wf_total,
            row.resample_passed,
            row.resample_total,
            row.universe_first_place,
            row.universe_total,
            row.loo_first_place,
            SYMBOLS.len(),
            row.bootstrap_first_place,
            row.bootstrap_total,
            row.full_win_rate * 100.0,
        ));
    }

    markdown.push_str("\n## Stress-universe winners\n\n");
    markdown.push_str("| Universe | Symbols | Winner | Return % | Runner-up | Return % |\n");
    markdown.push_str("|----------|---------|--------|---------:|-----------|---------:|\n");
    for row in universe_stress {
        markdown.push_str(&format!(
            "| {} | {} | {} | {:.1} | {} | {:.1} |\n",
            row.label,
            row.symbols.join(", "),
            row.best.name(),
            row.best_return_pct,
            row.second.name(),
            row.second_return_pct,
        ));
    }

    let mut csv = String::from(
        "rank,strategy,score,full_return_pct,full_trades,full_win_rate_pct,wf_passed,wf_total,wf_avg_return_pct,wf_avg_trades,resample_passed,resample_total,resample_avg_return_pct,resample_avg_trades,universe_first_place,universe_total,loo_first_place,loo_total,bootstrap_first_place,bootstrap_total\n",
    );
    for (idx, row) in rows.iter().enumerate() {
        csv.push_str(&format!(
            "{},{},{:.6},{:.1},{},{:.2},{},{},{:.1},{:.1},{},{},{:.1},{:.1},{},{},{},{},{},{}\n",
            idx + 1,
            row.strategy,
            row.composite_score,
            row.full_return_pct,
            row.full_trades,
            row.full_win_rate * 100.0,
            row.wf_passed,
            row.wf_total,
            row.wf_avg_return_pct,
            row.wf_avg_trades,
            row.resample_passed,
            row.resample_total,
            row.resample_avg_return_pct,
            row.resample_avg_trades,
            row.universe_first_place,
            row.universe_total,
            row.loo_first_place,
            SYMBOLS.len(),
            row.bootstrap_first_place,
            row.bootstrap_total,
        ));
    }

    fs::write(SNAPSHOT_LATEST_MD, &markdown)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, markdown)?;
    fs::write(&archive_csv, csv)?;

    Ok((archive_md, archive_csv))
}

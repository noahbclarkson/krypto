//! Stress-test latency / timing realism for the capped-book portfolio audit.
//!
//! This keeps the same daily trend leaders and stress universes as
//! `trend_portfolio_stress_audit`, but compares execution-timing scenarios:
//! 1. baseline next-open entry / fixed-hold next-open exit
//! 2. one-extra-bar delay on both entry and exit
//!
//! The goal is to see whether the portfolio-level / capped-book conclusions
//! survive the timing fragility already identified in the fair trend harness.

use anyhow::Result;
use chrono::Utc;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::{collections::HashMap, fs};

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const WARMUP_BARS: usize = 200;
const PERIOD: usize = 20;
const POSITION_CAP: usize = 3;
const MIN_TRADES_PER_WINDOW: usize = 30;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/trend_portfolio_latency_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/trend_portfolio_latency_latest.csv";

const STRESS_UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base6",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "NoDOGE",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT",
        ],
    ),
    (
        "Legacy5",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT"],
    ),
    ("LegacyCore4", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT"]),
    (
        "LargeCaps6",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT", "LTCUSDT",
        ],
    ),
    (
        "OldGuard6",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
        ],
    ),
    (
        "OldGuardNoBNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    Macd,
    MacdRegime,
    TurtleRegime,
    TurtleRegimeMacd,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::Macd => "MACD",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegime => "Turtle+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::Macd,
            Self::MacdRegime,
            Self::TurtleRegime,
            Self::TurtleRegimeMacd,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
struct ExecutionScenario {
    name: &'static str,
    entry_delay_bars: usize,
    exit_delay_bars: usize,
}

const EXECUTION_SCENARIOS: &[ExecutionScenario] = &[
    ExecutionScenario {
        name: "Baseline",
        entry_delay_bars: 1,
        exit_delay_bars: 1,
    },
    ExecutionScenario {
        name: "Delay1",
        entry_delay_bars: 2,
        exit_delay_bars: 2,
    },
];

#[derive(Clone, Debug)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    gross_return: f64,
    net_return: f64,
}

#[derive(Clone, Debug)]
struct SymbolPlan {
    trades: Vec<TradeWindow>,
}

#[derive(Clone, Debug)]
struct PortfolioStats {
    aligned_return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    avg_active_positions: f64,
    idle_days_pct: f64,
    trade_sum_return_pct: f64,
    trades: usize,
    win_rate_pct: f64,
}

#[derive(Clone, Debug)]
struct ScenarioSummaryRow {
    strategy: StrategyKind,
    uncapped: PortfolioStats,
    capped: PortfolioStats,
    capped_wf_passed: usize,
    capped_wf_total: usize,
    capped_wf_avg_return_pct: f64,
    capped_resample_passed: usize,
    capped_resample_total: usize,
    capped_resample_avg_return_pct: f64,
}

#[derive(Clone, Debug)]
struct ScenarioSummary {
    scenario: ExecutionScenario,
    rows: Vec<ScenarioSummaryRow>,
}

#[derive(Clone, Debug)]
struct UniverseSummary {
    label: String,
    symbols: Vec<String>,
    scenarios: Vec<ScenarioSummary>,
}

#[derive(Clone, Debug)]
struct IntegratedSummaryRow {
    strategy: StrategyKind,
    score: f64,
    baseline_universe_wins: usize,
    delay_universe_wins: usize,
    total_universe_cases: usize,
    baseline_wf_passed: usize,
    baseline_wf_total: usize,
    delay_wf_passed: usize,
    delay_wf_total: usize,
    baseline_resample_passed: usize,
    baseline_resample_total: usize,
    delay_resample_passed: usize,
    delay_resample_total: usize,
    avg_baseline_capped_return_pct: f64,
    avg_delay_capped_return_pct: f64,
    avg_baseline_sharpe: f64,
    avg_delay_sharpe: f64,
    avg_delay_return_delta_pct: f64,
    avg_delay_sharpe_delta: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== TREND PORTFOLIO LATENCY AUDIT ===\n");
    println!("Hold: {} bars", HOLD_BARS);
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Portfolio lenses: uncapped equal-weight and top-{} strength-capped book",
        POSITION_CAP
    );
    println!("Execution scenarios:");
    for scenario in EXECUTION_SCENARIOS {
        println!(
            "- {}: entry delay {} bar(s), exit delay {} bar(s)",
            scenario.name, scenario.entry_delay_bars, scenario.exit_delay_bars
        );
    }

    let loader = DataLoader::new(None, None);
    let mut all_symbols = Vec::<&str>::new();
    for &(_, symbols) in STRESS_UNIVERSES {
        for &symbol in symbols {
            if !all_symbols.contains(&symbol) {
                all_symbols.push(symbol);
            }
        }
    }

    let mut raw_cache = HashMap::<String, DataFrame>::new();
    for &symbol in &all_symbols {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", df.height());
        raw_cache.insert(symbol.to_string(), df);
    }

    let mut universe_summaries = Vec::new();
    for &(label, symbols) in STRESS_UNIVERSES {
        println!("\n=== {} ===", label);
        println!("Symbols: {}", symbols.join(", "));

        let universe = aligned_universe(&raw_cache, symbols)?;
        let quarter_windows = quarter_windows(universe.steps);
        let resample_windows = cpcv_style_windows(universe.steps);
        let mut scenarios = Vec::new();
        for &scenario in EXECUTION_SCENARIOS {
            let mut rows = Vec::new();
            for &strategy in StrategyKind::all() {
                let plans = build_symbol_plans(&universe, strategy, scenario)?;
                let uncapped = simulate_portfolio(&plans, universe.steps, None);
                let capped = simulate_portfolio(&plans, universe.steps, Some(POSITION_CAP));

                let mut capped_wf_passed = 0usize;
                let mut capped_wf_return_sum = 0.0;
                for &(start, end) in &quarter_windows {
                    let window_stats = simulate_portfolio_windows(
                        &plans,
                        universe.steps,
                        &[(start, end)],
                        Some(POSITION_CAP),
                    );
                    capped_wf_return_sum += window_stats.aligned_return_pct;
                    if window_stats.aligned_return_pct > 0.0
                        && window_stats.trades >= MIN_TRADES_PER_WINDOW
                    {
                        capped_wf_passed += 1;
                    }
                }

                let mut capped_resample_passed = 0usize;
                let mut capped_resample_return_sum = 0.0;
                for windows in &resample_windows {
                    let resample_stats = simulate_portfolio_windows(
                        &plans,
                        universe.steps,
                        windows,
                        Some(POSITION_CAP),
                    );
                    capped_resample_return_sum += resample_stats.aligned_return_pct;
                    if resample_stats.aligned_return_pct > 0.0
                        && resample_stats.trades >= MIN_TRADES_PER_WINDOW
                    {
                        capped_resample_passed += 1;
                    }
                }

                rows.push(ScenarioSummaryRow {
                    strategy,
                    uncapped,
                    capped,
                    capped_wf_passed,
                    capped_wf_total: quarter_windows.len(),
                    capped_wf_avg_return_pct: capped_wf_return_sum / quarter_windows.len() as f64,
                    capped_resample_passed,
                    capped_resample_total: resample_windows.len(),
                    capped_resample_avg_return_pct: capped_resample_return_sum
                        / resample_windows.len() as f64,
                });
            }
            rows.sort_by(|a, b| {
                b.capped
                    .sharpe
                    .partial_cmp(&a.capped.sharpe)
                    .unwrap()
                    .then_with(|| {
                        b.capped
                            .aligned_return_pct
                            .partial_cmp(&a.capped.aligned_return_pct)
                            .unwrap()
                    })
            });

            println!("\nScenario: {}", scenario.name);
            println!(
                "{:<20} {:>10} {:>8} {:>8} {:>10} || {:>10} {:>8} {:>8} {:>10} {:>9} {:>11}",
                "Strategy",
                "UncapRet",
                "UShp",
                "UMaxDD",
                "UAvgAct",
                "CapRet",
                "CShp",
                "CMaxDD",
                "CAvgAct",
                "WF",
                "RS"
            );
            println!("{}", "-".repeat(142));
            for row in &rows {
                println!(
                    "{:<20} {:>9.1}% {:>8.2} {:>7.1}% {:>10.2} || {:>9.1}% {:>8.2} {:>7.1}% {:>10.2} {:>4}/{} {:>5}/{}",
                    row.strategy.name(),
                    row.uncapped.aligned_return_pct,
                    row.uncapped.sharpe,
                    row.uncapped.max_dd_pct,
                    row.uncapped.avg_active_positions,
                    row.capped.aligned_return_pct,
                    row.capped.sharpe,
                    row.capped.max_dd_pct,
                    row.capped.avg_active_positions,
                    row.capped_wf_passed,
                    row.capped_wf_total,
                    row.capped_resample_passed,
                    row.capped_resample_total,
                );
            }

            scenarios.push(ScenarioSummary { scenario, rows });
        }

        println!("\nCapped-book baseline vs delay deltas:");
        let baseline = scenarios
            .iter()
            .find(|s| s.scenario.name == "Baseline")
            .unwrap();
        let delay = scenarios
            .iter()
            .find(|s| s.scenario.name == "Delay1")
            .unwrap();
        for strategy in StrategyKind::all() {
            let base = baseline
                .rows
                .iter()
                .find(|r| r.strategy == *strategy)
                .unwrap();
            let delayed = delay.rows.iter().find(|r| r.strategy == *strategy).unwrap();
            println!(
                "- {:<18} CapRet {:+9.1}% | Sharpe {:+6.2} | MaxDD {:+6.1}% | Trades {:+4}",
                strategy.name(),
                delayed.capped.aligned_return_pct - base.capped.aligned_return_pct,
                delayed.capped.sharpe - base.capped.sharpe,
                delayed.capped.max_dd_pct - base.capped.max_dd_pct,
                delayed.capped.trades as i64 - base.capped.trades as i64,
            );
        }

        universe_summaries.push(UniverseSummary {
            label: label.to_string(),
            symbols: symbols.iter().map(|s| s.to_string()).collect(),
            scenarios,
        });
    }

    let integrated_rows = integrated_summary_rows(&universe_summaries);
    println!("\n=== INTEGRATED CAPPED-BOOK LATENCY SUMMARY ===");
    println!(
        "{:<20} {:>7} {:>7} {:>7} {:>10} {:>10} {:>9} {:>9} {:>11} {:>11}",
        "Strategy",
        "Score",
        "Base#1",
        "Dly#1",
        "BaseSharpe",
        "DlySharpe",
        "BaseRS",
        "DlyRS",
        "DlyRetΔ",
        "DlyShpΔ"
    );
    println!("{}", "-".repeat(118));
    for row in &integrated_rows {
        println!(
            "{:<20} {:>7.3} {:>4}/{} {:>4}/{} {:>10.2} {:>10.2} {:>4}/{} {:>4}/{} {:>+10.1}% {:>+11.2}",
            row.strategy.name(),
            row.score,
            row.baseline_universe_wins,
            row.total_universe_cases,
            row.delay_universe_wins,
            row.total_universe_cases,
            row.avg_baseline_sharpe,
            row.avg_delay_sharpe,
            row.baseline_resample_passed,
            row.baseline_resample_total,
            row.delay_resample_passed,
            row.delay_resample_total,
            row.avg_delay_return_delta_pct,
            row.avg_delay_sharpe_delta,
        );
    }

    let snapshot_paths = write_snapshot(&universe_summaries, &integrated_rows)?;
    println!("\nPortfolio-latency snapshots written:");
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);
    println!("- {}", snapshot_paths.0);
    println!("- {}", snapshot_paths.1);

    Ok(())
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

fn aligned_universe(
    raw_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<UniverseData> {
    let min_len = symbols
        .iter()
        .filter_map(|symbol| raw_cache.get(*symbol).map(|df| df.height()))
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty universe"))?;
    let steps = min_len.saturating_sub(1);
    if steps == 0 {
        anyhow::bail!("not enough data");
    }

    let mut data = Vec::new();
    for &symbol in symbols {
        let df = raw_cache
            .get(symbol)
            .ok_or_else(|| anyhow::anyhow!("missing symbol {symbol}"))?
            .slice(0, min_len);
        data.push((symbol.to_string(), df));
    }

    Ok(UniverseData { data, steps })
}

fn build_symbol_plans(
    universe: &UniverseData,
    strategy: StrategyKind,
    scenario: ExecutionScenario,
) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(_, df)| build_symbol_plan(df, strategy, scenario))
        .collect()
}

fn build_symbol_plan(
    df: &DataFrame,
    strategy: StrategyKind,
    scenario: ExecutionScenario,
) -> Result<SymbolPlan> {
    let signals = generate_signals(df, strategy)?;
    let strengths = generate_strengths(df, strategy)?;
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let mut trades = Vec::new();
    let mut i = WARMUP_BARS;

    while i + scenario.entry_delay_bars + HOLD_BARS + scenario.exit_delay_bars <= n {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }

        let entry_idx = i + scenario.entry_delay_bars;
        let exit_idx = entry_idx + HOLD_BARS + scenario.exit_delay_bars - 1;
        if exit_idx >= n {
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

        let gross_return = if signal > 0 {
            exit / entry - 1.0
        } else {
            entry / exit - 1.0
        };
        let net_return = gross_return - 2.0 * TAKER_FEE;
        trades.push(TradeWindow {
            entry_idx,
            exit_idx,
            strength: strengths.get(i).copied().unwrap_or(0.0).abs(),
            gross_return,
            net_return,
        });
        i = exit_idx;
    }

    Ok(SymbolPlan { trades })
}

fn simulate_portfolio(plans: &[SymbolPlan], steps: usize, cap: Option<usize>) -> PortfolioStats {
    let full_window = vec![(0usize, steps)];
    simulate_portfolio_windows(plans, steps, &full_window, cap)
}

fn simulate_portfolio_windows(
    plans: &[SymbolPlan],
    steps: usize,
    windows: &[(usize, usize)],
    cap: Option<usize>,
) -> PortfolioStats {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut trade_sum_return_pct = 0.0;
    let mut trades = 0usize;
    let mut wins = 0usize;

    for plan in plans {
        for trade in &plan.trades {
            if trade_in_windows(trade, windows) {
                trade_sum_return_pct += trade.net_return * 100.0;
                trades += 1;
                if trade.net_return > 0.0 {
                    wins += 1;
                }
            }
        }
    }

    for &(start, end) in windows {
        let bounded_end = end.min(steps);
        for day in start.min(steps)..bounded_end {
            let mut active = Vec::<(f64, f64)>::new();
            for plan in plans {
                for trade in &plan.trades {
                    if !trade_in_window(trade, start, bounded_end) {
                        continue;
                    }
                    if day == trade.entry_idx {
                        active.push((trade.strength, -TAKER_FEE));
                    }
                    if day >= trade.entry_idx && day < trade.exit_idx {
                        let span = (trade.exit_idx - trade.entry_idx) as f64;
                        if span > 0.0 {
                            active.push((trade.strength, trade.gross_return / span));
                        }
                    }
                    if day == trade.exit_idx {
                        active.push((trade.strength, -TAKER_FEE));
                    }
                }
            }

            active_counts[day] = active.len();
            if active.is_empty() {
                daily_returns[day] = 0.0;
                equity_curve[day + 1] = equity_curve[day];
                continue;
            }

            active.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            let selected_len = cap.unwrap_or(active.len()).min(active.len());
            let selected = &active[..selected_len];
            let avg_ret = selected.iter().map(|(_, r)| *r).sum::<f64>() / selected.len() as f64;
            daily_returns[day] = avg_ret;
            equity_curve[day + 1] = equity_curve[day] * (1.0 + avg_ret);
        }
    }
    active_counts[steps] = active_counts[steps.saturating_sub(1)];

    let aligned_return_pct = (equity_curve.last().copied().unwrap_or(1.0) - 1.0) * 100.0;
    let sharpe = calc_sharpe_from_returns(&daily_returns);
    let max_dd_pct = calc_max_drawdown_pct(&equity_curve);
    let avg_active_positions =
        active_counts.iter().sum::<usize>() as f64 / active_counts.len() as f64;
    let idle_days_pct = active_counts.iter().filter(|&&c| c == 0).count() as f64
        / active_counts.len() as f64
        * 100.0;
    let win_rate_pct = if trades == 0 {
        0.0
    } else {
        wins as f64 / trades as f64 * 100.0
    };

    PortfolioStats {
        aligned_return_pct,
        sharpe,
        max_dd_pct,
        avg_active_positions,
        idle_days_pct,
        trade_sum_return_pct,
        trades,
        win_rate_pct,
    }
}

fn trade_in_window(trade: &TradeWindow, start: usize, end: usize) -> bool {
    trade.entry_idx >= start && trade.exit_idx < end
}

fn trade_in_windows(trade: &TradeWindow, windows: &[(usize, usize)]) -> bool {
    windows
        .iter()
        .any(|&(start, end)| trade_in_window(trade, start, end))
}

fn generate_signals(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::Macd => generate_macd_signals(df),
        StrategyKind::MacdRegime => generate_macd_regime_signals(df),
        StrategyKind::TurtleRegime => generate_turtle_regime_signals(df, PERIOD),
        StrategyKind::TurtleRegimeMacd => generate_turtle_regime_macd_signals(df, PERIOD),
    }
}

fn generate_strengths(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<f64>> {
    match strategy {
        StrategyKind::Macd | StrategyKind::MacdRegime => generate_macd_strengths(df),
        StrategyKind::TurtleRegime | StrategyKind::TurtleRegimeMacd => {
            generate_turtle_strengths(df, PERIOD)
        }
    }
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
    }
    Ok(signals)
}

fn generate_macd_strengths(df: &DataFrame) -> Result<Vec<f64>> {
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0.0; macd.len()];
    for i in 0..macd.len() {
        out[i] = (macd.get(i).unwrap_or(0.0) - signal.get(i).unwrap_or(0.0)).abs();
    }
    Ok(out)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = generate_macd_signals(df)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; macd.len()];

    for i in 0..macd.len() {
        let sig = macd[i];
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200.get(i).copied().unwrap_or(0.0);
        if sig > 0 && price > sma {
            out[i] = 1;
        } else if sig < 0 && price < sma {
            out[i] = -1;
        }
    }
    Ok(out)
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

fn generate_turtle_strengths(df: &DataFrame, period: usize) -> Result<Vec<f64>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();
    let mut out = vec![0.0; n];

    for i in period..n {
        let period_high = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let period_low = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let current_close = close.get(i).unwrap_or(0.0);
        let range = (period_high - period_low).abs().max(1e-9);
        if current_close > period_high {
            out[i] = (current_close - period_high) / range;
        } else if current_close < period_low {
            out[i] = (period_low - current_close) / range;
        }
    }

    Ok(out)
}

fn generate_turtle_regime_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_signals(df, period)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; turtle.len()];

    for i in 0..turtle.len() {
        let sig = turtle[i];
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200.get(i).copied().unwrap_or(0.0);
        if sig > 0 && price > sma {
            out[i] = 1;
        } else if sig < 0 && price < sma {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_regime_signals(df, period)?;
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

fn quarter_windows(steps: usize) -> Vec<(usize, usize)> {
    let quarter = steps / 4;
    let mut windows = Vec::new();
    for window_idx in 0..4usize {
        let start = window_idx * quarter;
        let end = if window_idx == 3 {
            steps
        } else {
            (window_idx + 1) * quarter
        };
        windows.push((start, end));
    }
    windows
}

fn cpcv_base_blocks(steps: usize) -> Vec<(usize, usize)> {
    let block = steps / RESAMPLE_BLOCKS;
    let mut base = Vec::new();
    for block_idx in 0..RESAMPLE_BLOCKS {
        let start = block_idx * block;
        let end = if block_idx == RESAMPLE_BLOCKS - 1 {
            steps
        } else {
            (block_idx + 1) * block
        };
        base.push((start, end));
    }
    base
}

fn cpcv_style_windows(steps: usize) -> Vec<Vec<(usize, usize)>> {
    let base = cpcv_base_blocks(steps);
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
    windows
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

fn calc_sharpe_from_returns(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns
        .iter()
        .map(|r| {
            let d = r - mean;
            d * d
        })
        .sum::<f64>()
        / returns.len() as f64;
    let std = var.sqrt();
    if std <= 1e-12 {
        0.0
    } else {
        mean / std * 365.0f64.sqrt()
    }
}

fn calc_max_drawdown_pct(equity_curve: &[f64]) -> f64 {
    let mut peak = equity_curve.first().copied().unwrap_or(1.0);
    let mut max_dd = 0.0;
    for &value in equity_curve {
        if value > peak {
            peak = value;
        }
        let dd = (value / peak - 1.0) * 100.0;
        if dd < max_dd {
            max_dd = dd;
        }
    }
    max_dd.abs()
}

fn integrated_summary_rows(universes: &[UniverseSummary]) -> Vec<IntegratedSummaryRow> {
    let total_universe_cases = universes.len();
    let mut rows = Vec::new();

    for &strategy in StrategyKind::all() {
        let mut baseline_universe_wins = 0usize;
        let mut delay_universe_wins = 0usize;
        let mut baseline_wf_passed = 0usize;
        let mut baseline_wf_total = 0usize;
        let mut delay_wf_passed = 0usize;
        let mut delay_wf_total = 0usize;
        let mut baseline_resample_passed = 0usize;
        let mut baseline_resample_total = 0usize;
        let mut delay_resample_passed = 0usize;
        let mut delay_resample_total = 0usize;
        let mut avg_baseline_capped_return_pct = 0.0;
        let mut avg_delay_capped_return_pct = 0.0;
        let mut avg_baseline_sharpe = 0.0;
        let mut avg_delay_sharpe = 0.0;
        let mut avg_delay_return_delta_pct = 0.0;
        let mut avg_delay_sharpe_delta = 0.0;

        for universe in universes {
            let baseline = universe
                .scenarios
                .iter()
                .find(|s| s.scenario.name == "Baseline")
                .unwrap();
            let delay = universe
                .scenarios
                .iter()
                .find(|s| s.scenario.name == "Delay1")
                .unwrap();
            let base = baseline
                .rows
                .iter()
                .find(|r| r.strategy == strategy)
                .unwrap();
            let delayed = delay.rows.iter().find(|r| r.strategy == strategy).unwrap();

            if baseline.rows.first().map(|r| r.strategy) == Some(strategy) {
                baseline_universe_wins += 1;
            }
            if delay.rows.first().map(|r| r.strategy) == Some(strategy) {
                delay_universe_wins += 1;
            }

            baseline_wf_passed += base.capped_wf_passed;
            baseline_wf_total += base.capped_wf_total;
            delay_wf_passed += delayed.capped_wf_passed;
            delay_wf_total += delayed.capped_wf_total;
            baseline_resample_passed += base.capped_resample_passed;
            baseline_resample_total += base.capped_resample_total;
            delay_resample_passed += delayed.capped_resample_passed;
            delay_resample_total += delayed.capped_resample_total;
            avg_baseline_capped_return_pct += base.capped.aligned_return_pct;
            avg_delay_capped_return_pct += delayed.capped.aligned_return_pct;
            avg_baseline_sharpe += base.capped.sharpe;
            avg_delay_sharpe += delayed.capped.sharpe;
            avg_delay_return_delta_pct +=
                delayed.capped.aligned_return_pct - base.capped.aligned_return_pct;
            avg_delay_sharpe_delta += delayed.capped.sharpe - base.capped.sharpe;
        }

        let denom = total_universe_cases.max(1) as f64;
        avg_baseline_capped_return_pct /= denom;
        avg_delay_capped_return_pct /= denom;
        avg_baseline_sharpe /= denom;
        avg_delay_sharpe /= denom;
        avg_delay_return_delta_pct /= denom;
        avg_delay_sharpe_delta /= denom;

        let baseline_win_score = baseline_universe_wins as f64 / total_universe_cases.max(1) as f64;
        let delay_win_score = delay_universe_wins as f64 / total_universe_cases.max(1) as f64;
        let baseline_wf_score = baseline_wf_passed as f64 / baseline_wf_total.max(1) as f64;
        let delay_wf_score = delay_wf_passed as f64 / delay_wf_total.max(1) as f64;
        let baseline_resample_score =
            baseline_resample_passed as f64 / baseline_resample_total.max(1) as f64;
        let delay_resample_score =
            delay_resample_passed as f64 / delay_resample_total.max(1) as f64;
        let baseline_sharpe_score = (avg_baseline_sharpe / 8.0).clamp(0.0, 1.0);
        let delay_sharpe_score = (avg_delay_sharpe / 8.0).clamp(0.0, 1.0);
        let delay_delta_score = ((avg_delay_sharpe_delta + 2.0) / 4.0).clamp(0.0, 1.0);

        let score = 0.20 * baseline_win_score
            + 0.15 * delay_win_score
            + 0.15 * baseline_wf_score
            + 0.20 * baseline_resample_score
            + 0.10 * delay_wf_score
            + 0.10 * delay_resample_score
            + 0.05 * baseline_sharpe_score
            + 0.03 * delay_sharpe_score
            + 0.02 * delay_delta_score;

        rows.push(IntegratedSummaryRow {
            strategy,
            score,
            baseline_universe_wins,
            delay_universe_wins,
            total_universe_cases,
            baseline_wf_passed,
            baseline_wf_total,
            delay_wf_passed,
            delay_wf_total,
            baseline_resample_passed,
            baseline_resample_total,
            delay_resample_passed,
            delay_resample_total,
            avg_baseline_capped_return_pct,
            avg_delay_capped_return_pct,
            avg_baseline_sharpe,
            avg_delay_sharpe,
            avg_delay_return_delta_pct,
            avg_delay_sharpe_delta,
        });
    }

    rows.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap()
            .then_with(|| b.delay_resample_passed.cmp(&a.delay_resample_passed))
            .then_with(|| b.baseline_resample_passed.cmp(&a.baseline_resample_passed))
            .then_with(|| b.delay_universe_wins.cmp(&a.delay_universe_wins))
            .then_with(|| b.avg_delay_sharpe.partial_cmp(&a.avg_delay_sharpe).unwrap())
    });

    rows
}

fn write_snapshot(
    universes: &[UniverseSummary],
    integrated_rows: &[IntegratedSummaryRow],
) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!("{}/trend_portfolio_latency_{}.md", SNAPSHOT_DIR, timestamp);
    let archive_csv = format!("{}/trend_portfolio_latency_{}.csv", SNAPSHOT_DIR, timestamp);

    let mut markdown = String::new();
    markdown.push_str("# Trend Portfolio Latency Audit Snapshot\n\n");
    markdown.push_str(&format!("- Timestamp (UTC): {}\n", Utc::now().to_rfc3339()));
    markdown.push_str(&format!("- Candles: {}\n", CANDLES));
    markdown.push_str(&format!("- Hold bars: {}\n", HOLD_BARS));
    markdown.push_str(&format!("- Fee each side: {:.3}%\n", TAKER_FEE * 100.0));
    markdown.push_str(&format!("- Warmup bars: {}\n", WARMUP_BARS));
    markdown.push_str(&format!(
        "- Position cap: top {} active trades by strength\n",
        POSITION_CAP
    ));
    markdown.push_str("- Scenarios: Baseline next-open vs one-extra-bar delay on entry/exit\n\n");

    markdown.push_str("## Integrated ranking summary\n\n");
    markdown.push_str("| Rank | Strategy | Score | Base #1 | Delay #1 | Base WF | Delay WF | Base RS | Delay RS | Avg Base Sharpe | Avg Delay Sharpe | Avg Delay Ret Δ % | Avg Delay Sharpe Δ |\n");
    markdown.push_str("|------|----------|------:|--------:|---------:|--------:|---------:|--------:|---------:|----------------:|-----------------:|------------------:|-------------------:|\n");
    for (idx, row) in integrated_rows.iter().enumerate() {
        markdown.push_str(&format!(
            "| {} | {} | {:.3} | {}/{} | {}/{} | {}/{} | {}/{} | {}/{} | {}/{} | {:.2} | {:.2} | {:+.1} | {:+.2} |\n",
            idx + 1,
            row.strategy.name(),
            row.score,
            row.baseline_universe_wins,
            row.total_universe_cases,
            row.delay_universe_wins,
            row.total_universe_cases,
            row.baseline_wf_passed,
            row.baseline_wf_total,
            row.delay_wf_passed,
            row.delay_wf_total,
            row.baseline_resample_passed,
            row.baseline_resample_total,
            row.delay_resample_passed,
            row.delay_resample_total,
            row.avg_baseline_sharpe,
            row.avg_delay_sharpe,
            row.avg_delay_return_delta_pct,
            row.avg_delay_sharpe_delta,
        ));
    }

    markdown.push_str("\n## Capped-book winners by universe and scenario\n\n");
    markdown.push_str("| Universe | Scenario | Winner | Cap Return % | Sharpe | MaxDD % | WF | RS | Runner-up | Cap Return % | Sharpe |\n");
    markdown.push_str("|----------|----------|--------|-------------:|-------:|--------:|----|----|-----------|-------------:|-------:|\n");
    for universe in universes {
        for scenario in &universe.scenarios {
            let winner = &scenario.rows[0];
            let runner_up = scenario.rows.get(1).unwrap_or(winner);
            markdown.push_str(&format!(
                "| {} | {} | {} | {:.1} | {:.2} | {:.1} | {}/{} | {}/{} | {} | {:.1} | {:.2} |\n",
                universe.label,
                scenario.scenario.name,
                winner.strategy.name(),
                winner.capped.aligned_return_pct,
                winner.capped.sharpe,
                winner.capped.max_dd_pct,
                winner.capped_wf_passed,
                winner.capped_wf_total,
                winner.capped_resample_passed,
                winner.capped_resample_total,
                runner_up.strategy.name(),
                runner_up.capped.aligned_return_pct,
                runner_up.capped.sharpe,
            ));
        }
    }

    markdown.push_str("\n## Delay deltas vs baseline (capped book)\n\n");
    markdown.push_str("| Universe | Strategy | Delay Ret Delta % | Delay Sharpe Delta | Delay MaxDD Delta % | Delay Trade Delta |\n");
    markdown.push_str("|----------|----------|------------------:|-------------------:|--------------------:|------------------:|\n");
    for universe in universes {
        let baseline = universe
            .scenarios
            .iter()
            .find(|s| s.scenario.name == "Baseline")
            .unwrap();
        let delay = universe
            .scenarios
            .iter()
            .find(|s| s.scenario.name == "Delay1")
            .unwrap();
        for strategy in StrategyKind::all() {
            let base = baseline
                .rows
                .iter()
                .find(|r| r.strategy == *strategy)
                .unwrap();
            let delayed = delay.rows.iter().find(|r| r.strategy == *strategy).unwrap();
            markdown.push_str(&format!(
                "| {} | {} | {:+.1} | {:+.2} | {:+.1} | {} |\n",
                universe.label,
                strategy.name(),
                delayed.capped.aligned_return_pct - base.capped.aligned_return_pct,
                delayed.capped.sharpe - base.capped.sharpe,
                delayed.capped.max_dd_pct - base.capped.max_dd_pct,
                delayed.capped.trades as i64 - base.capped.trades as i64,
            ));
        }
    }

    markdown.push_str("\n## Full per-universe scenario tables\n");
    for universe in universes {
        markdown.push_str(&format!("\n### {}\n\n", universe.label));
        markdown.push_str(&format!("Symbols: {}\n\n", universe.symbols.join(", ")));
        for scenario in &universe.scenarios {
            markdown.push_str(&format!("#### {}\n\n", scenario.scenario.name));
            markdown.push_str("| Rank | Strategy | Uncap Ret % | Uncap Sharpe | Uncap MaxDD % | Cap Ret % | Cap Sharpe | Cap MaxDD % | WF | RS | Trades | Win % | Idle % | Cap Gap vs Trade Sum % |\n");
            markdown.push_str("|------|----------|------------:|-------------:|---------------:|----------:|-----------:|------------:|----|----|-------:|------:|-------:|-----------------------:|\n");
            for (idx, row) in scenario.rows.iter().enumerate() {
                markdown.push_str(&format!(
                    "| {} | {} | {:.1} | {:.2} | {:.1} | {:.1} | {:.2} | {:.1} | {}/{} | {}/{} | {} | {:.1} | {:.1} | {:+.1} |\n",
                    idx + 1,
                    row.strategy.name(),
                    row.uncapped.aligned_return_pct,
                    row.uncapped.sharpe,
                    row.uncapped.max_dd_pct,
                    row.capped.aligned_return_pct,
                    row.capped.sharpe,
                    row.capped.max_dd_pct,
                    row.capped_wf_passed,
                    row.capped_wf_total,
                    row.capped_resample_passed,
                    row.capped_resample_total,
                    row.capped.trades,
                    row.capped.win_rate_pct,
                    row.capped.idle_days_pct,
                    row.capped.aligned_return_pct - row.capped.trade_sum_return_pct,
                ));
            }
            markdown.push('\n');
        }
    }

    let mut csv = String::from(
        "section,universe,symbols,scenario,rank,strategy,score,baseline_universe_wins,total_universe_cases,delay_universe_wins,baseline_wf_passed,baseline_wf_total,delay_wf_passed,delay_wf_total,baseline_resample_passed,baseline_resample_total,delay_resample_passed,delay_resample_total,avg_baseline_capped_return_pct,avg_delay_capped_return_pct,avg_baseline_sharpe,avg_delay_sharpe,avg_delay_return_delta_pct,avg_delay_sharpe_delta,uncapped_return_pct,uncapped_sharpe,uncapped_max_dd_pct,uncapped_avg_active_positions,uncapped_idle_days_pct,uncapped_trade_sum_return_pct,uncapped_trades,uncapped_win_rate_pct,capped_return_pct,capped_sharpe,capped_max_dd_pct,capped_avg_active_positions,capped_idle_days_pct,capped_trade_sum_return_pct,capped_trades,capped_win_rate_pct,capped_wf_passed,capped_wf_total,capped_wf_avg_return_pct,capped_resample_passed,capped_resample_total,capped_resample_avg_return_pct,capped_gap_vs_trade_sum_pct\n",
    );
    for (idx, row) in integrated_rows.iter().enumerate() {
        csv.push_str(&format!(
            "integrated,,,{},,{},{:.4},{},{},{},{},{},{},{},{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},,,,,,,,,,,,,,,,,,,,,,,\n",
            idx + 1,
            row.strategy.name(),
            row.score,
            row.baseline_universe_wins,
            row.total_universe_cases,
            row.delay_universe_wins,
            row.baseline_wf_passed,
            row.baseline_wf_total,
            row.delay_wf_passed,
            row.delay_wf_total,
            row.baseline_resample_passed,
            row.baseline_resample_total,
            row.delay_resample_passed,
            row.delay_resample_total,
            row.avg_baseline_capped_return_pct,
            row.avg_delay_capped_return_pct,
            row.avg_baseline_sharpe,
            row.avg_delay_sharpe,
            row.avg_delay_return_delta_pct,
            row.avg_delay_sharpe_delta,
        ));
    }
    for universe in universes {
        for scenario in &universe.scenarios {
            for (idx, row) in scenario.rows.iter().enumerate() {
                csv.push_str(&format!(
                    "detail,{},{},{},{},{},,,,,,,,,,,,,,,,,,,{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{},{:.4},{},{},{:.4},{},{},{:.4},{:.4}\n",
                    universe.label,
                    universe.symbols.join("|"),
                    scenario.scenario.name,
                    idx + 1,
                    row.strategy.name(),
                    row.uncapped.aligned_return_pct,
                    row.uncapped.sharpe,
                    row.uncapped.max_dd_pct,
                    row.uncapped.avg_active_positions,
                    row.uncapped.idle_days_pct,
                    row.uncapped.trade_sum_return_pct,
                    row.uncapped.trades,
                    row.uncapped.win_rate_pct,
                    row.capped.aligned_return_pct,
                    row.capped.sharpe,
                    row.capped.max_dd_pct,
                    row.capped.avg_active_positions,
                    row.capped.idle_days_pct,
                    row.capped.trade_sum_return_pct,
                    row.capped.trades,
                    row.capped.win_rate_pct,
                    row.capped_wf_passed,
                    row.capped_wf_total,
                    row.capped_wf_avg_return_pct,
                    row.capped_resample_passed,
                    row.capped_resample_total,
                    row.capped_resample_avg_return_pct,
                    row.capped.aligned_return_pct - row.capped.trade_sum_return_pct,
                ));
            }
        }
    }

    fs::write(SNAPSHOT_LATEST_MD, &markdown)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, markdown)?;
    fs::write(&archive_csv, csv)?;

    Ok((archive_md, archive_csv))
}

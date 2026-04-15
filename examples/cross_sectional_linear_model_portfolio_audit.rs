//! Portfolio-realism audit for the rolling pooled linear alpha model.
//!
//! Goal:
//! - pressure-test the new richer-model candidate under the same realistic top-3 capped-book lens
//!   used for the incumbent yardsticks
//! - compare the real model, its shuffled-label control, and the current benchmark rows in one file
//! - treat this as trust work, not promotion

use anyhow::{Context, Result};
use chrono::Utc;
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::{collections::HashMap, fs};

const BENCHMARK: &str = "BTCUSDT";
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const WARMUP_BARS: usize = 250;
const CS_LOOKBACK: usize = 63;
const TURTLE_PERIOD: usize = 20;
const TRAIN_BARS: usize = 252;
const RIDGE_LAMBDA: f64 = 1e-3;
const TOP_K: usize = 2;
const BOTTOM_K: usize = 2;
const SHUFFLE_ROTATE_BY: usize = 17;

const STRESS_UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
    ),
    (
        "NoDOGE",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "LTCUSDT"],
    ),
    ("Legacy4", &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "Legacy5BNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"],
    ),
    (
        "OldGuardNoBNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
    (
        "LargeCaps5",
        &["ETHUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT", "LTCUSDT"],
    ),
];

const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/cross_sectional_linear_model_portfolio_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/cross_sectional_linear_model_portfolio_latest.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    LinearAlpha,
    LinearAlphaShuffled,
    MacdRegime,
    TurtleRegimeMacd,
    EnsembleMajority3,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::LinearAlpha,
            Self::LinearAlphaShuffled,
            Self::MacdRegime,
            Self::TurtleRegimeMacd,
            Self::EnsembleMajority3,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::LinearAlpha => "LinearAlpha(panel)",
            Self::LinearAlphaShuffled => "LinearAlpha(shuffled_y)",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
            Self::EnsembleMajority3 => "Ensemble(Majority 2/3)",
        }
    }
}

#[derive(Clone, Debug)]
struct FeatureSet {
    per_symbol: HashMap<String, Vec<Vec<f64>>>,
}

#[derive(Clone, Debug)]
struct StrategySignals {
    signals: HashMap<String, Vec<i32>>,
    strengths: HashMap<String, Vec<f64>>,
}

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

#[derive(Clone, Debug, Default)]
struct PortfolioStats {
    aligned_return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate_pct: f64,
    avg_active_positions: f64,
    idle_days_pct: f64,
}

#[derive(Clone, Debug)]
struct ResultRow {
    strategy: StrategyKind,
    stats: PortfolioStats,
}

#[derive(Clone, Debug)]
struct UniverseSummary {
    label: String,
    rows: Vec<ResultRow>,
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== CROSS-SECTIONAL LINEAR MODEL PORTFOLIO AUDIT ===\n");
    println!(
        "Goal: force the rolling linear model through the realistic top-{} capped-book lens",
        POSITION_CAP
    );
    println!("Benchmark leader for shared features: {}", BENCHMARK);
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Model train window: {} bars | shuffle rotate: {}\n",
        TRAIN_BARS, SHUFFLE_ROTATE_BY
    );

    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;

    let mut load_symbols = vec![BENCHMARK];
    for &(_, symbols) in STRESS_UNIVERSES {
        for &symbol in symbols {
            if !load_symbols.contains(&symbol) {
                load_symbols.push(symbol);
            }
        }
    }

    let mut raw_cache = HashMap::<String, DataFrame>::new();
    raw_cache.insert(BENCHMARK.to_string(), bench_df.clone());
    for &symbol in load_symbols.iter().filter(|&&s| s != BENCHMARK) {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, Some(&bench_df))?;
        println!("{} bars", enriched.height());
        raw_cache.insert(symbol.to_string(), enriched);
    }

    let mut summaries = Vec::new();
    for &(label, symbols) in STRESS_UNIVERSES {
        println!("\n--- Universe: {} ({}) ---", label, symbols.join(", "));
        let data_cache = prepare_universe_cache(&raw_cache, symbols)?;
        let feature_set = build_feature_set(&data_cache, symbols)?;
        let signal_map = generate_all_signals(&data_cache, &feature_set, symbols)?;
        let universe = aligned_universe(&data_cache, symbols)?;

        let mut rows = Vec::new();
        for &strategy in StrategyKind::all() {
            let plans = build_symbol_plans(
                &universe,
                signal_map
                    .get(&strategy)
                    .context("missing strategy outputs")?,
            )?;
            let stats = simulate_portfolio(&plans, universe.steps);
            rows.push(ResultRow { strategy, stats });
        }

        rows.sort_by(|a, b| {
            b.stats
                .sharpe
                .partial_cmp(&a.stats.sharpe)
                .unwrap()
                .then_with(|| {
                    b.stats
                        .aligned_return_pct
                        .partial_cmp(&a.stats.aligned_return_pct)
                        .unwrap()
                })
        });

        println!(
            "{:<26} {:>11} {:>8} {:>8} {:>7} {:>7}",
            "Strategy", "Ret%", "Sharpe", "MaxDD", "Trades", "Win%"
        );
        println!("{}", "-".repeat(88));
        for row in &rows {
            println!(
                "{:<26} {:>10.1} {:>8.2} {:>7.1} {:>7} {:>6.1}",
                row.strategy.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
            );
        }

        summaries.push(UniverseSummary {
            label: label.to_string(),
            rows,
        });
    }

    let (archive_md, archive_csv) = write_snapshot(&summaries)?;
    println!("\nSnapshots written:");
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);
    println!("- {}", archive_md);
    println!("- {}", archive_csv);

    println!("\nInterpretation:");
    println!("- If the real model stays strong here while the shuffled-label control compresses, trust improves.");
    println!("- If the control stays too competitive under the capped-book lens, treat the richer-model story as still structurally suspicious.");
    println!("- This is a trust audit, not a promotion result.");

    Ok(())
}

fn prepare_universe_cache(
    raw_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<HashMap<String, DataFrame>> {
    let mut cache = HashMap::<String, DataFrame>::new();
    cache.insert(
        BENCHMARK.to_string(),
        raw_cache.get(BENCHMARK).unwrap().clone(),
    );

    let mut cs_map = HashMap::<String, DataFrame>::new();
    for &symbol in symbols {
        let df = raw_cache
            .get(symbol)
            .ok_or_else(|| anyhow::anyhow!("missing symbol {symbol}"))?
            .clone();
        cs_map.insert(symbol.to_string(), df);
    }
    compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;
    for &symbol in symbols {
        cache.insert(symbol.to_string(), cs_map.get(symbol).unwrap().clone());
    }
    Ok(cache)
}

fn build_feature_set(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<FeatureSet> {
    let mut per_symbol = HashMap::new();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        per_symbol.insert(symbol.to_string(), build_symbol_features(df)?);
    }
    Ok(FeatureSet { per_symbol })
}

fn build_symbol_features(df: &DataFrame) -> Result<Vec<Vec<f64>>> {
    let n = df.height();
    let close = collect_f64(df, "close")?;
    let macd = collect_f64(df, "macd")?;
    let macd_signal = collect_f64(df, "macd_signal")?;
    let cs_mom = collect_f64(df, "cs_momentum_rank")?;
    let cs_trend = collect_f64(df, "cs_trend_score")?;
    let sma_200 = calculate_sma_vec(&close, 200);
    let vol21 = rolling_log_vol(&close, 21);

    let mut out = vec![vec![0.0; 6]; n];
    for i in 0..n {
        let c = close[i];
        let mom = cs_mom.get(i).copied().unwrap_or(0.5) - 0.5;
        let trend = cs_trend.get(i).copied().unwrap_or(0.5) - 0.5;
        let macd_gap = if c > 0.0 {
            (macd[i] - macd_signal[i]) / c
        } else {
            0.0
        };
        let sma_gap = if c > 0.0 && sma_200[i] > 0.0 {
            (c - sma_200[i]) / c
        } else {
            0.0
        };
        let reversal21 = if i >= 21 && close[i - 21] > 0.0 {
            -((c / close[i - 21]) - 1.0)
        } else {
            0.0
        };
        out[i] = vec![mom, trend, macd_gap, sma_gap, reversal21, vol21[i]];
    }
    Ok(out)
}

fn generate_all_signals(
    data_cache: &HashMap<String, DataFrame>,
    feature_set: &FeatureSet,
    symbols: &[&str],
) -> Result<HashMap<StrategyKind, StrategySignals>> {
    let mut out = HashMap::new();
    out.insert(
        StrategyKind::LinearAlpha,
        generate_linear_alpha_outputs(data_cache, feature_set, symbols, false)?,
    );
    out.insert(
        StrategyKind::LinearAlphaShuffled,
        generate_linear_alpha_outputs(data_cache, feature_set, symbols, true)?,
    );

    let mut macd_signals = HashMap::new();
    let mut macd_strengths = HashMap::new();
    let mut turtle_signals = HashMap::new();
    let mut turtle_strengths = HashMap::new();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        let macd = generate_macd_regime_signals(df)?;
        let turtle = generate_turtle_regime_macd_signals(df, TURTLE_PERIOD)?;
        let macd_strength = generate_macd_strengths(df)?;
        let turtle_strength = generate_turtle_strengths(df, TURTLE_PERIOD)?;
        macd_signals.insert(symbol.to_string(), macd.clone());
        macd_strengths.insert(symbol.to_string(), macd_strength.clone());
        turtle_signals.insert(symbol.to_string(), turtle.clone());
        turtle_strengths.insert(symbol.to_string(), turtle_strength.clone());
    }
    out.insert(
        StrategyKind::MacdRegime,
        StrategySignals {
            signals: macd_signals.clone(),
            strengths: macd_strengths.clone(),
        },
    );
    out.insert(
        StrategyKind::TurtleRegimeMacd,
        StrategySignals {
            signals: turtle_signals.clone(),
            strengths: turtle_strengths.clone(),
        },
    );

    let mut ensemble_signals = HashMap::new();
    let mut ensemble_strengths = HashMap::new();
    for &symbol in symbols {
        let linear = &out.get(&StrategyKind::LinearAlpha).unwrap().signals[symbol];
        let macd = &macd_signals[symbol];
        let turtle = &turtle_signals[symbol];
        ensemble_signals.insert(
            symbol.to_string(),
            consensus_signal(&[linear, macd, turtle], 2),
        );
        ensemble_strengths.insert(symbol.to_string(), macd_strengths[symbol].clone());
    }
    out.insert(
        StrategyKind::EnsembleMajority3,
        StrategySignals {
            signals: ensemble_signals,
            strengths: ensemble_strengths,
        },
    );

    Ok(out)
}

fn generate_linear_alpha_outputs(
    data_cache: &HashMap<String, DataFrame>,
    feature_set: &FeatureSet,
    symbols: &[&str],
    shuffle_targets: bool,
) -> Result<StrategySignals> {
    let n = min_rows(data_cache)?;
    let mut signal_map = HashMap::<String, Vec<i32>>::new();
    let mut strength_map = HashMap::<String, Vec<f64>>::new();
    for &symbol in symbols {
        signal_map.insert(symbol.to_string(), vec![0; n]);
        strength_map.insert(symbol.to_string(), vec![0.0; n]);
    }

    let mut open_map = HashMap::<String, Vec<f64>>::new();
    for &symbol in symbols {
        open_map.insert(
            symbol.to_string(),
            collect_f64(data_cache.get(symbol).unwrap(), "open")?,
        );
    }

    let feature_count = 6;
    for i in WARMUP_BARS.max(TRAIN_BARS)..(n.saturating_sub(HOLD_BARS + 1)) {
        let train_start = i.saturating_sub(TRAIN_BARS);
        let train_end = i;

        let mut train_x = Vec::<Vec<f64>>::new();
        let mut train_y = Vec::<f64>::new();
        for t in train_start..train_end {
            for &symbol in symbols {
                let feats = &feature_set.per_symbol[symbol][t];
                let open = open_map.get(symbol).unwrap();
                if t + 1 + HOLD_BARS >= open.len() {
                    continue;
                }
                let entry = open[t + 1];
                let exit = open[t + 1 + HOLD_BARS];
                if entry <= 0.0 || exit <= 0.0 {
                    continue;
                }
                train_x.push(feats.clone());
                train_y.push(exit / entry - 1.0);
            }
        }
        if train_x.len() < feature_count * 8 {
            continue;
        }
        if shuffle_targets {
            rotate_targets(&mut train_y, SHUFFLE_ROTATE_BY);
        }

        let scaler = fit_scaler(&train_x, feature_count);
        let beta = fit_ridge_model(&train_x, &train_y, &scaler, RIDGE_LAMBDA)?;

        let mut scores = Vec::<(String, f64)>::new();
        for &symbol in symbols {
            let feats = &feature_set.per_symbol[symbol][i];
            let score = predict_with_beta(feats, &scaler, &beta);
            scores.push((symbol.to_string(), score));
        }

        for (symbol, score) in &scores {
            if let Some(strengths) = strength_map.get_mut(symbol) {
                strengths[i] = score.abs();
            }
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (rank, (symbol, score)) in scores.iter().enumerate() {
            let signal = if rank < TOP_K && *score > 0.0 {
                1
            } else if rank >= scores.len().saturating_sub(BOTTOM_K) && *score < 0.0 {
                -1
            } else {
                0
            };
            if let Some(vec) = signal_map.get_mut(symbol) {
                vec[i] = signal;
            }
        }
    }

    Ok(StrategySignals {
        signals: signal_map,
        strengths: strength_map,
    })
}

fn aligned_universe(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<UniverseData> {
    let min_len = symbols
        .iter()
        .filter_map(|symbol| data_cache.get(*symbol).map(|df| df.height()))
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty universe"))?;
    let steps = min_len.saturating_sub(1);
    if steps == 0 {
        anyhow::bail!("not enough data")
    }
    let mut data = Vec::new();
    for &symbol in symbols {
        let df = data_cache
            .get(symbol)
            .with_context(|| format!("missing symbol {symbol}"))?
            .slice(0, min_len);
        data.push((symbol.to_string(), df));
    }
    Ok(UniverseData { data, steps })
}

fn build_symbol_plans(
    universe: &UniverseData,
    outputs: &StrategySignals,
) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(symbol, df)| {
            let signals = outputs
                .signals
                .get(symbol)
                .with_context(|| format!("missing signals for {symbol}"))?;
            let strengths = outputs
                .strengths
                .get(symbol)
                .with_context(|| format!("missing strengths for {symbol}"))?;
            build_symbol_plan(df, signals, strengths)
        })
        .collect()
}

fn build_symbol_plan(df: &DataFrame, signals: &[i32], strengths: &[f64]) -> Result<SymbolPlan> {
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let mut trades = Vec::new();
    let mut i = WARMUP_BARS;

    while i + HOLD_BARS + 1 < n {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }
        let entry_idx = i + 1;
        let exit_idx = i + 1 + HOLD_BARS;
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

fn simulate_portfolio(plans: &[SymbolPlan], steps: usize) -> PortfolioStats {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut trade_count = 0usize;
    let mut wins = 0usize;

    for plan in plans {
        for trade in &plan.trades {
            if trade.net_return > 0.0 {
                wins += 1;
            }
            trade_count += 1;
        }
    }

    for day in 0..steps {
        let mut active = Vec::<(f64, f64)>::new();
        for plan in plans {
            for trade in &plan.trades {
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
            equity_curve[day + 1] = equity_curve[day];
            continue;
        }
        active.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let selected = &active[..POSITION_CAP.min(active.len())];
        let avg_ret = selected.iter().map(|(_, r)| *r).sum::<f64>() / selected.len() as f64;
        daily_returns[day] = avg_ret;
        equity_curve[day + 1] = equity_curve[day] * (1.0 + avg_ret);
    }
    active_counts[steps] = active_counts[steps.saturating_sub(1)];

    PortfolioStats {
        aligned_return_pct: (equity_curve.last().copied().unwrap_or(1.0) - 1.0) * 100.0,
        sharpe: calc_sharpe_from_returns(&daily_returns),
        max_dd_pct: calc_max_drawdown_pct(&equity_curve),
        trades: trade_count,
        win_rate_pct: if trade_count == 0 {
            0.0
        } else {
            wins as f64 / trade_count as f64 * 100.0
        },
        avg_active_positions: active_counts.iter().sum::<usize>() as f64
            / active_counts.len() as f64,
        idle_days_pct: active_counts.iter().filter(|&&c| c == 0).count() as f64
            / active_counts.len() as f64
            * 100.0,
    }
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

fn rotate_targets(values: &mut [f64], by: usize) {
    if values.len() > 1 {
        values.rotate_left(by % values.len().max(1));
    }
}

#[derive(Clone, Debug)]
struct Scaler {
    means: Vec<f64>,
    stds: Vec<f64>,
}

fn fit_scaler(rows: &[Vec<f64>], feature_count: usize) -> Scaler {
    let mut means = vec![0.0; feature_count];
    let mut stds = vec![1.0; feature_count];
    if rows.is_empty() {
        return Scaler { means, stds };
    }
    for j in 0..feature_count {
        means[j] = rows.iter().map(|r| r[j]).sum::<f64>() / rows.len() as f64;
        let var = rows
            .iter()
            .map(|r| {
                let d = r[j] - means[j];
                d * d
            })
            .sum::<f64>()
            / rows.len() as f64;
        stds[j] = var.sqrt().max(1e-6);
    }
    Scaler { means, stds }
}

fn fit_ridge_model(
    rows: &[Vec<f64>],
    targets: &[f64],
    scaler: &Scaler,
    lambda: f64,
) -> Result<Vec<f64>> {
    let p = scaler.means.len() + 1;
    let mut xtx = vec![vec![0.0; p]; p];
    let mut xty = vec![0.0; p];
    for (row, &y) in rows.iter().zip(targets.iter()) {
        let x = standardized_with_bias(row, scaler);
        for a in 0..p {
            xty[a] += x[a] * y;
            for b in 0..p {
                xtx[a][b] += x[a] * x[b];
            }
        }
    }
    for d in 1..p {
        xtx[d][d] += lambda;
    }
    solve_linear_system(xtx, xty)
}

fn standardized_with_bias(row: &[f64], scaler: &Scaler) -> Vec<f64> {
    let mut out = Vec::with_capacity(row.len() + 1);
    out.push(1.0);
    for (j, &v) in row.iter().enumerate() {
        out.push((v - scaler.means[j]) / scaler.stds[j]);
    }
    out
}

fn predict_with_beta(row: &[f64], scaler: &Scaler, beta: &[f64]) -> f64 {
    let x = standardized_with_bias(row, scaler);
    x.iter().zip(beta.iter()).map(|(a, b)| a * b).sum()
}

fn solve_linear_system(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Result<Vec<f64>> {
    let n = b.len();
    for i in 0..n {
        let mut pivot = i;
        for r in (i + 1)..n {
            if a[r][i].abs() > a[pivot][i].abs() {
                pivot = r;
            }
        }
        if a[pivot][i].abs() < 1e-12 {
            return Err(anyhow::anyhow!("singular linear system"));
        }
        if pivot != i {
            a.swap(i, pivot);
            b.swap(i, pivot);
        }
        let diag = a[i][i];
        for j in i..n {
            a[i][j] /= diag;
        }
        b[i] /= diag;
        for r in 0..n {
            if r == i {
                continue;
            }
            let factor = a[r][i];
            if factor.abs() < 1e-15 {
                continue;
            }
            for c in i..n {
                a[r][c] -= factor * a[i][c];
            }
            b[r] -= factor * b[i];
        }
    }
    Ok(b)
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
        let sma = sma_200[i].unwrap_or(0.0);
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
    let mut strengths = vec![0.0; n];
    for i in period..n {
        let period_high = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let period_low = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let current_close = close.get(i).unwrap_or(0.0);
        let breakout_up = if period_high.is_finite() && period_high > 0.0 {
            (current_close / period_high - 1.0).max(0.0)
        } else {
            0.0
        };
        let breakout_down = if period_low.is_finite() && period_low > 0.0 {
            (period_low / current_close - 1.0).max(0.0)
        } else {
            0.0
        };
        strengths[i] = breakout_up.max(breakout_down);
    }
    Ok(strengths)
}

fn generate_turtle_regime_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_signals(df, period)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; df.height()];
    for i in 0..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200[i].unwrap_or(0.0);
        if sma <= 0.0 {
            continue;
        }
        if turtle[i] > 0 && price > sma {
            out[i] = 1;
        } else if turtle[i] < 0 && price < sma {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_regime_signals(df, period)?;
    let macd = generate_macd_signals(df)?;
    let mut out = vec![0i32; df.height()];
    for i in 0..df.height() {
        if turtle[i] != 0 && turtle[i] == macd[i] {
            out[i] = turtle[i];
        }
    }
    Ok(out)
}

fn calculate_sma(series: &Float64Chunked, period: usize) -> Vec<Option<f64>> {
    let n = series.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    let mut sum = 0.0;
    let mut window = std::collections::VecDeque::<f64>::new();
    for i in 0..n {
        let v = series.get(i).unwrap_or(0.0);
        window.push_back(v);
        sum += v;
        if window.len() > period {
            if let Some(old) = window.pop_front() {
                sum -= old;
            }
        }
        if window.len() == period {
            out[i] = Some(sum / period as f64);
        }
    }
    out
}

fn calculate_sma_vec(values: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    if period == 0 {
        return out;
    }
    let mut sum = 0.0;
    let mut window = std::collections::VecDeque::<f64>::new();
    for (i, &v) in values.iter().enumerate() {
        window.push_back(v);
        sum += v;
        if window.len() > period {
            if let Some(old) = window.pop_front() {
                sum -= old;
            }
        }
        if window.len() == period {
            out[i] = sum / period as f64;
        }
    }
    out
}

fn rolling_log_vol(close: &[f64], window: usize) -> Vec<f64> {
    let mut out = vec![0.0; close.len()];
    let mut rets = vec![0.0; close.len()];
    for i in 1..close.len() {
        if close[i - 1] > 0.0 && close[i] > 0.0 {
            rets[i] = (close[i] / close[i - 1]).ln();
        }
    }
    for i in 0..close.len() {
        if i + 1 < window {
            continue;
        }
        let slice = &rets[i + 1 - window..=i];
        let mean = slice.iter().sum::<f64>() / slice.len() as f64;
        let var = slice.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / slice.len() as f64;
        out[i] = var.sqrt();
    }
    out
}

fn collect_f64(df: &DataFrame, col: &str) -> Result<Vec<f64>> {
    Ok(df
        .column(col)?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect())
}

fn min_rows(data_cache: &HashMap<String, DataFrame>) -> Result<usize> {
    data_cache
        .values()
        .map(|df| df.height())
        .min()
        .context("empty cache")
}

fn calc_sharpe_from_returns(returns: &[f64]) -> f64 {
    if returns.len() < 2 {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    let std = variance.sqrt();
    if std <= 1e-12 {
        return 0.0;
    }
    mean / std * 252.0f64.sqrt()
}

fn calc_max_drawdown_pct(equity_curve: &[f64]) -> f64 {
    let mut peak = equity_curve.first().copied().unwrap_or(1.0);
    let mut max_dd: f64 = 0.0;
    for &eq in equity_curve {
        peak = peak.max(eq);
        if peak > 0.0 {
            max_dd = max_dd.max(1.0 - eq / peak);
        }
    }
    max_dd * 100.0
}

fn write_snapshot(summaries: &[UniverseSummary]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!(
        "{}/cross_sectional_linear_model_portfolio_{}.md",
        SNAPSHOT_DIR, timestamp
    );
    let archive_csv = format!(
        "{}/cross_sectional_linear_model_portfolio_{}.csv",
        SNAPSHOT_DIR, timestamp
    );
    let md = render_markdown(summaries);
    let csv = render_csv(summaries);
    fs::write(SNAPSHOT_LATEST_MD, &md)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, md)?;
    fs::write(&archive_csv, csv)?;
    Ok((archive_md, archive_csv))
}

fn render_markdown(summaries: &[UniverseSummary]) -> String {
    let mut out = String::new();
    out.push_str("# Cross-Sectional Linear Model Portfolio Audit\n\n");
    out.push_str("Fair daily harness: signal at close, next-open entry, fixed 21-bar hold, 0.1% taker each side, top-3 strength-capped book.\n\n");
    for summary in summaries {
        out.push_str(&format!("## {}\n\n", summary.label));
        out.push_str(
            "| Strategy | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Active | Idle % |\n",
        );
        out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|\n");
        for row in &summary.rows {
            out.push_str(&format!(
                "| {} | {:.1} | {:.2} | {:.1} | {} | {:.1} | {:.2} | {:.1} |\n",
                row.strategy.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
            ));
        }
        out.push('\n');
    }
    out
}

fn render_csv(summaries: &[UniverseSummary]) -> String {
    let mut out = String::from("universe,strategy,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_active_positions,idle_days_pct\n");
    for summary in summaries {
        for row in &summary.rows {
            out.push_str(&format!(
                "{},{},{:.4},{:.4},{:.4},{},{:.4},{:.4},{:.4}\n",
                summary.label,
                row.strategy.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
            ));
        }
    }
    out
}

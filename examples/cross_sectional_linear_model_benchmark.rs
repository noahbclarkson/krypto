//! Test a simple richer-model Track C bucket: a rolling pooled linear alpha model.
//!
//! Goal:
//! - move beyond nearby trend / exogenous / interaction tweaks
//! - use only lagged daily features available at the close
//! - train a simple pooled linear model on prior bars across the basket
//! - rank symbols each day and trade the strongest / weakest predictions
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry at next open
//! - exit at open after fixed 21-bar hold
//! - 0.1% taker fee on entry and exit

use anyhow::Result;
use krypto::{
    algo::{strategies::CrossSectionalMomentum, SignalGenerator},
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;

const BENCHMARK: &str = "BTCUSDT";
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 30;
const CS_LOOKBACK: usize = 63;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const WARMUP_BARS: usize = 250;
const TRAIN_BARS: usize = 252;
const RIDGE_LAMBDA: f64 = 1e-3;
const TOP_K: usize = 2;
const BOTTOM_K: usize = 2;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    LinearAlpha,
    CrossSectionalMomentum,
    MacdRegime,
    EnsembleMajority,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::LinearAlpha => "LinearAlpha(panel)",
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::MacdRegime => "MACD+Regime",
            Self::EnsembleMajority => "Ensemble(Majority 2/3)",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::LinearAlpha,
            Self::CrossSectionalMomentum,
            Self::MacdRegime,
            Self::EnsembleMajority,
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
struct UniverseSummary {
    label: &'static str,
    rows: Vec<SummaryRow>,
}

#[derive(Clone, Debug)]
struct SummaryRow {
    kind: StrategyKind,
    full_return_pct: f64,
    full_trades: usize,
    full_win_rate: f64,
    wf_passed: usize,
    wf_total: usize,
    resample_passed: usize,
    resample_total: usize,
}

#[derive(Clone, Debug)]
struct FeatureSet {
    names: Vec<String>,
    per_symbol: HashMap<String, Vec<Vec<f64>>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== CROSS-SECTIONAL LINEAR MODEL BENCHMARK ===\n");
    println!("Benchmark leader for shared features: {}", BENCHMARK);
    println!("Train window: {} bars", TRAIN_BARS);
    println!("Cross-sectional lookback: {} bars", CS_LOOKBACK);
    println!("Model: pooled ridge regression over lagged panel features");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side\n", TAKER_FEE * 100.0);

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
    let mut strategy_universe_wins = HashMap::<StrategyKind, usize>::new();
    for &strategy in StrategyKind::all() {
        strategy_universe_wins.insert(strategy, 0);
    }

    for &(label, symbols) in STRESS_UNIVERSES {
        println!("\nRunning universe: {}", label);
        let data_cache = prepare_universe_cache(&raw_cache, symbols)?;
        let feature_set = build_feature_set(&data_cache, symbols)?;
        let signal_map = generate_all_signals(&data_cache, &feature_set, symbols)?;
        let quarter_windows = quarter_windows(&data_cache)?;
        let resample_sets = cpcv_style_windows(&data_cache)?;
        let mut rows = Vec::new();

        for &strategy in StrategyKind::all() {
            let full = evaluate_strategy(
                &data_cache,
                &signal_map,
                strategy,
                symbols,
                &[(0, min_rows(&data_cache)?)],
            )?;

            let mut wf_passed = 0usize;
            for &(start, end) in &quarter_windows {
                let eval = evaluate_strategy(
                    &data_cache,
                    &signal_map,
                    strategy,
                    symbols,
                    &[(start, end)],
                )?;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    wf_passed += 1;
                }
            }

            let mut rs_passed = 0usize;
            for windows in &resample_sets {
                let eval = evaluate_strategy(&data_cache, &signal_map, strategy, symbols, windows)?;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    rs_passed += 1;
                }
            }

            rows.push(SummaryRow {
                kind: strategy,
                full_return_pct: full.total_return_pct,
                full_trades: full.trades,
                full_win_rate: full.win_rate(),
                wf_passed,
                wf_total: quarter_windows.len(),
                resample_passed: rs_passed,
                resample_total: resample_sets.len(),
            });
        }

        rows.sort_by(|a, b| {
            b.resample_passed
                .cmp(&a.resample_passed)
                .then_with(|| b.wf_passed.cmp(&a.wf_passed))
                .then_with(|| b.full_return_pct.partial_cmp(&a.full_return_pct).unwrap())
        });

        if let Some(best) = rows.first() {
            *strategy_universe_wins.entry(best.kind).or_default() += 1;
        }

        summaries.push(UniverseSummary { label, rows });
    }

    for summary in &summaries {
        println!("\n=== {} ===", summary.label);
        println!(
            "{:<26} {:>10} {:>8} {:>8} {:>10} {:>12}",
            "Strategy", "Return%", "Trades", "Win%", "WF", "Resamples"
        );
        for row in &summary.rows {
            println!(
                "{:<26} {:>10.1} {:>8} {:>7.1}% {:>4}/{} {:>6}/{}",
                row.kind.name(),
                row.full_return_pct,
                row.full_trades,
                row.full_win_rate * 100.0,
                row.wf_passed,
                row.wf_total,
                row.resample_passed,
                row.resample_total,
            );
        }
    }

    println!("\n=== UNIVERSE WIN COUNT ===");
    let mut win_rows: Vec<_> = StrategyKind::all()
        .iter()
        .map(|kind| (*kind, *strategy_universe_wins.get(kind).unwrap_or(&0)))
        .collect();
    win_rows.sort_by(|a, b| b.1.cmp(&a.1));
    for (kind, wins) in win_rows {
        println!(
            "- {:<26} {}/{} universes",
            kind.name(),
            wins,
            STRESS_UNIVERSES.len()
        );
    }

    println!("\nInterpretation:");
    println!("- This is a breadth probe for a richer model class, not a deployment candidate.");
    println!("- The right comparison is chronology-first against the current yardsticks, not just raw return.");
    println!("- If the linear panel model fails, that is still useful: it means the validation stack is strong enough to kill a more flexible family.");

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
        let df = cs_map
            .get(symbol)
            .ok_or_else(|| anyhow::anyhow!("missing computed symbol {symbol}"))?
            .clone();
        cache.insert(symbol.to_string(), df);
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
    Ok(FeatureSet {
        names: vec![
            "cs_momentum_rank".into(),
            "cs_trend_score".into(),
            "macd_gap".into(),
            "sma200_gap".into(),
            "reversal21".into(),
            "vol21".into(),
        ],
        per_symbol,
    })
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
) -> Result<HashMap<StrategyKind, HashMap<String, Vec<i32>>>> {
    let mut out = HashMap::new();

    let linear = generate_linear_alpha_signals(data_cache, feature_set, symbols)?;
    out.insert(StrategyKind::LinearAlpha, linear);

    let mut csm = HashMap::new();
    let mut macd_regime = HashMap::new();
    let mut ensemble = HashMap::new();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        let csm_sig = series_to_i32(CrossSectionalMomentum::new().predict(df)?);
        let macd_regime_sig = generate_macd_regime_signals(df)?;
        let turtle_regime_macd_sig = generate_turtle_regime_macd_signals(df, 20)?;
        let ensemble_sig = majority_vote(&[&csm_sig, &macd_regime_sig, &turtle_regime_macd_sig]);
        csm.insert(symbol.to_string(), csm_sig);
        macd_regime.insert(symbol.to_string(), macd_regime_sig);
        ensemble.insert(symbol.to_string(), ensemble_sig);
    }
    out.insert(StrategyKind::CrossSectionalMomentum, csm);
    out.insert(StrategyKind::MacdRegime, macd_regime);
    out.insert(StrategyKind::EnsembleMajority, ensemble);

    Ok(out)
}

fn generate_linear_alpha_signals(
    data_cache: &HashMap<String, DataFrame>,
    feature_set: &FeatureSet,
    symbols: &[&str],
) -> Result<HashMap<String, Vec<i32>>> {
    let n = min_rows(data_cache)?;
    let mut signal_map = HashMap::<String, Vec<i32>>::new();
    for &symbol in symbols {
        signal_map.insert(symbol.to_string(), vec![0; n]);
    }

    let mut open_map = HashMap::<String, Vec<f64>>::new();
    for &symbol in symbols {
        open_map.insert(
            symbol.to_string(),
            collect_f64(data_cache.get(symbol).unwrap(), "open")?,
        );
    }

    let feature_count = feature_set.names.len();
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

        let scaler = fit_scaler(&train_x, feature_count);
        let beta = fit_ridge_model(&train_x, &train_y, &scaler, RIDGE_LAMBDA)?;

        let mut scores = Vec::<(String, f64)>::new();
        for &symbol in symbols {
            let feats = &feature_set.per_symbol[symbol][i];
            let score = predict_with_beta(feats, &scaler, &beta);
            scores.push((symbol.to_string(), score));
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

    Ok(signal_map)
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

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    signal_map: &HashMap<StrategyKind, HashMap<String, Vec<i32>>>,
    strategy: StrategyKind,
    symbols: &[&str],
    windows: &[(usize, usize)],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    let by_symbol = signal_map.get(&strategy).unwrap();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        let signals = by_symbol.get(symbol).unwrap();
        for &(start, end) in windows {
            let result = backtest_fixed_hold_next_open_window(df, signals, start, end)?;
            aggregate.add_assign(&result);
        }
    }
    Ok(aggregate)
}

fn backtest_fixed_hold_next_open_window(
    df: &DataFrame,
    signals: &[i32],
    start: usize,
    end: usize,
) -> Result<StrategyResult> {
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let mut trade_returns = Vec::new();
    let mut i = WARMUP_BARS.max(start);
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
        i = exit_idx;
    }

    Ok(StrategyResult {
        total_return_pct: trade_returns.iter().sum(),
        trades: trade_returns.len(),
        wins: trade_returns.iter().filter(|&&r| r > 0.0).count(),
    })
}

fn quarter_windows(data_cache: &HashMap<String, DataFrame>) -> Result<Vec<(usize, usize)>> {
    let n = min_rows(data_cache)?;
    let quarter = n / 4;
    Ok((0..4)
        .map(|idx| {
            let start = idx * quarter;
            let end = if idx == 3 { n } else { (idx + 1) * quarter };
            (start, end)
        })
        .collect())
}

fn cpcv_style_windows(data_cache: &HashMap<String, DataFrame>) -> Result<Vec<Vec<(usize, usize)>>> {
    let n = min_rows(data_cache)?;
    let block = n / RESAMPLE_BLOCKS;
    let base: Vec<(usize, usize)> = (0..RESAMPLE_BLOCKS)
        .map(|idx| {
            let start = idx * block;
            let end = if idx == RESAMPLE_BLOCKS - 1 {
                n
            } else {
                (idx + 1) * block
            };
            (start, end)
        })
        .collect();

    let combos = choose_indices(RESAMPLE_BLOCKS, RESAMPLE_TRAIN_BLOCKS);
    Ok(combos
        .into_iter()
        .map(|combo| {
            base.iter()
                .enumerate()
                .filter_map(|(idx, window)| {
                    if combo.contains(&idx) {
                        None
                    } else {
                        Some(*window)
                    }
                })
                .collect()
        })
        .collect())
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

fn min_rows(data_cache: &HashMap<String, DataFrame>) -> Result<usize> {
    data_cache
        .iter()
        .filter(|(k, _)| k.as_str() != BENCHMARK)
        .map(|(_, df)| df.height())
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty data cache"))
}

fn collect_f64(df: &DataFrame, name: &str) -> Result<Vec<f64>> {
    Ok(df
        .column(name)?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect())
}

fn series_to_i32(series: Series) -> Vec<i32> {
    if let Ok(ca) = series.i32() {
        return ca.into_iter().map(|v| v.unwrap_or(0)).collect();
    }
    series
        .f64()
        .unwrap()
        .into_iter()
        .map(|v| v.unwrap_or(0.0).round() as i32)
        .collect()
}

fn majority_vote(parts: &[&Vec<i32>]) -> Vec<i32> {
    let n = parts.first().map(|v| v.len()).unwrap_or(0);
    let mut out = vec![0; n];
    for i in 0..n {
        let mut pos = 0;
        let mut neg = 0;
        for part in parts {
            match part.get(i).copied().unwrap_or(0) {
                1 => pos += 1,
                -1 => neg += 1,
                _ => {}
            }
        }
        if pos >= 2 {
            out[i] = 1;
        } else if neg >= 2 {
            out[i] = -1;
        }
    }
    out
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let close = collect_f64(df, "close")?;
    let macd = collect_f64(df, "macd")?;
    let signal = collect_f64(df, "macd_signal")?;
    let sma_200 = calculate_sma_vec(&close, 200);
    let mut out = vec![0; df.height()];
    for i in 0..df.height() {
        if close[i] > sma_200[i] && macd[i] > signal[i] {
            out[i] = 1;
        } else if close[i] < sma_200[i] && macd[i] < signal[i] {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = collect_f64(df, "close")?;
    let high = collect_f64(df, "high")?;
    let low = collect_f64(df, "low")?;
    let macd = collect_f64(df, "macd")?;
    let signal = collect_f64(df, "macd_signal")?;
    let sma_200 = calculate_sma_vec(&close, 200);
    let mut out = vec![0; df.height()];

    for i in period..df.height() {
        let highest = (i - period..i)
            .map(|j| high[j])
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest = (i - period..i)
            .map(|j| low[j])
            .fold(f64::INFINITY, f64::min);
        let long_ok = close[i] > sma_200[i] && close[i] > highest && macd[i] > signal[i];
        let short_ok = close[i] < sma_200[i] && close[i] < lowest && macd[i] < signal[i];
        if long_ok {
            out[i] = 1;
        } else if short_ok {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn calculate_sma_vec(values: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i >= period {
            sum -= values[i - period];
        }
        if i + 1 >= period {
            out[i] = sum / period as f64;
        }
    }
    out
}

fn rolling_log_vol(values: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in period..values.len() {
        let slice = &values[(i - period)..=i];
        let mut lrs = Vec::with_capacity(slice.len().saturating_sub(1));
        for j in 1..slice.len() {
            let prev = slice[j - 1];
            let curr = slice[j];
            if prev > 0.0 && curr > 0.0 {
                lrs.push((curr / prev).ln());
            }
        }
        if !lrs.is_empty() {
            let mean = lrs.iter().sum::<f64>() / lrs.len() as f64;
            let var = lrs
                .iter()
                .map(|r| {
                    let d = r - mean;
                    d * d
                })
                .sum::<f64>()
                / lrs.len() as f64;
            out[i] = var.sqrt();
        }
    }
    out
}

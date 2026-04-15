//! Eigenfactor residual momentum breadth benchmark.
//!
//! Goal:
//! - rebalance away from the recent linear-model trust loop and the run of failed
//!   peer / cluster / graph rescue attempts
//! - test a genuinely broader latent-factor construction instead of another local
//!   relational tweak
//! - ask whether symbols keep trending after outperforming or underperforming a
//!   universe-level latent eigenfactor after removing both BTC market mode and that
//!   shared latent factor
//! - keep the same fair daily harness used elsewhere so results stay comparable
//!
//! Construction:
//! - use an early fixed window to estimate a correlation matrix across the universe
//! - run simple power iteration to extract the dominant latent factor loading vector
//!   from that matrix
//! - convert those loadings into a daily eigenfactor return series from the basket
//! - estimate a rolling BTC beta and a rolling eigenfactor beta for each symbol
//! - compute a 21-bar residual momentum score after removing both the BTC move and
//!   the symbol's latent-factor exposure
//! - trade momentum: long the biggest positive residuals and short the biggest negative residuals
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
use std::{collections::HashMap, fs};

const BENCHMARK: &str = "BTCUSDT";
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 30;
const CS_LOOKBACK: usize = 63;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const WARMUP_BARS: usize = 252;
const CALIBRATION_BARS: usize = 900;
const RESIDUAL_LOOKBACK: usize = 21;
const BETA_LOOKBACK: usize = 252;
const POWER_ITERS: usize = 64;
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
    EigenfactorResidualMomentum,
    CrossSectionalMomentum,
    MacdRegime,
    EnsembleMajority,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::EigenfactorResidualMomentum => "EigenfactorResidualMomentum",
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::MacdRegime => "MACD+Regime",
            Self::EnsembleMajority => "Ensemble(Majority 2/3)",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::EigenfactorResidualMomentum,
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

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== EIGENFACTOR RESIDUAL MOMENTUM BREADTH BENCHMARK ===\n");
    println!("Benchmark anchor (for shared features only): {}", BENCHMARK);
    println!("Peer calibration window: {} bars", CALIBRATION_BARS);
    println!("Residual lookback: {} bars", RESIDUAL_LOOKBACK);
    println!("Power iterations: {}", POWER_ITERS);
    println!("Rolling beta lookback: {} bars", BETA_LOOKBACK);
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
        let signal_map = generate_all_signals(&data_cache, symbols)?;
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
            "{:<30} {:>10} {:>8} {:>8} {:>10} {:>12}",
            "Strategy", "Return%", "Trades", "Win%", "WF", "Resamples"
        );
        for row in &summary.rows {
            println!(
                "{:<30} {:>10.1} {:>8} {:>7.1}% {:>4}/{} {:>6}/{}",
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
    for (kind, wins) in &win_rows {
        println!(
            "- {:<30} {}/{} universes",
            kind.name(),
            wins,
            STRESS_UNIVERSES.len()
        );
    }

    write_snapshot_files(&summaries, &win_rows)?;

    println!("\nInterpretation:");
    println!("- EigenfactorResidualMomentum asks whether residual momentum survives after removing both BTC market mode and a universe-level latent factor.");
    println!("- If it fails, that is still useful: it means the current daily relational bucket is not rescued even by a broader latent-factor view.");
    println!("- If it survives selectively, that points toward richer factor / graph-diffusion models rather than more local peer heuristics.");

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
        cs_map.insert(symbol.to_string(), raw_cache.get(symbol).unwrap().clone());
    }
    compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;
    for (symbol, df) in cs_map {
        cache.insert(symbol, df);
    }
    Ok(cache)
}

fn generate_all_signals(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<HashMap<StrategyKind, HashMap<String, Vec<i32>>>> {
    let mut out = HashMap::new();
    out.insert(
        StrategyKind::EigenfactorResidualMomentum,
        generate_eigenfactor_residual_signals(data_cache, symbols)?,
    );

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

fn generate_eigenfactor_residual_signals(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<HashMap<String, Vec<i32>>> {
    let n = min_rows(data_cache)?;

    let mut signal_map = HashMap::<String, Vec<i32>>::new();
    let mut close_map = HashMap::<String, Vec<f64>>::new();
    for &symbol in symbols {
        let close = collect_f64(data_cache.get(symbol).unwrap(), "close")?;
        close_map.insert(symbol.to_string(), close);
        signal_map.insert(symbol.to_string(), vec![0; n]);
    }

    let btc_close = collect_f64(data_cache.get(BENCHMARK).unwrap(), "close")?;
    let mut ret_map = HashMap::<String, Vec<f64>>::new();
    ret_map.insert(BENCHMARK.to_string(), close_returns(&btc_close));
    for &symbol in symbols {
        let close = close_map.get(symbol).unwrap();
        ret_map.insert(symbol.to_string(), close_returns(close));
    }

    let loadings = dominant_factor_loadings(symbols, &ret_map);
    println!("  latent loadings:");
    for &symbol in symbols {
        let loading = loadings.get(symbol).copied().unwrap_or(0.0);
        println!("  {:<10} {:+.3}", symbol, loading);
    }

    let mut eigenfactor_series = vec![f64::NAN; n];
    for i in 0..n {
        eigenfactor_series[i] =
            weighted_factor_return(&close_map, &loadings, i, RESIDUAL_LOOKBACK).unwrap_or(f64::NAN);
    }

    for i in
        WARMUP_BARS.max(CALIBRATION_BARS).max(RESIDUAL_LOOKBACK)..(n.saturating_sub(HOLD_BARS + 1))
    {
        let mut scores = Vec::<(String, f64)>::new();
        for &symbol in symbols {
            let close = close_map.get(symbol).unwrap();
            let residual = eigenfactor_residual_return(
                close,
                &btc_close,
                &eigenfactor_series,
                i,
                RESIDUAL_LOOKBACK,
                BETA_LOOKBACK,
            );
            let Some(residual) = residual else {
                continue;
            };
            if residual.is_finite() {
                scores.push((symbol.to_string(), residual));
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

    Ok(signal_map)
}

fn weighted_factor_return(
    close_map: &HashMap<String, Vec<f64>>,
    loadings: &HashMap<String, f64>,
    i: usize,
    lookback: usize,
) -> Option<f64> {
    let mut weighted_sum = 0.0;
    let mut abs_weight_sum = 0.0;
    for (symbol, weight) in loadings {
        if let Some(close) = close_map.get(symbol) {
            let ret = lookback_return(close, i, lookback);
            if ret.is_finite() && weight.is_finite() && weight.abs() > 1e-9 {
                weighted_sum += weight * ret;
                abs_weight_sum += weight.abs();
            }
        }
    }
    if abs_weight_sum <= 1e-12 {
        None
    } else {
        Some(weighted_sum / abs_weight_sum)
    }
}

fn dominant_factor_loadings(
    symbols: &[&str],
    ret_map: &HashMap<String, Vec<f64>>,
) -> HashMap<String, f64> {
    let m = symbols.len();
    let mut corr = vec![vec![0.0; m]; m];
    for a in 0..m {
        corr[a][a] = 1.0;
        for b in (a + 1)..m {
            let sa = symbols[a];
            let sb = symbols[b];
            let Some(ra) = ret_map.get(sa) else {
                continue;
            };
            let Some(rb) = ret_map.get(sb) else {
                continue;
            };
            let end = CALIBRATION_BARS.min(ra.len()).min(rb.len());
            let value = correlation(&ra[..end], &rb[..end]);
            corr[a][b] = value;
            corr[b][a] = value;
        }
    }

    let mut v = vec![1.0 / (m as f64).sqrt(); m];
    for _ in 0..POWER_ITERS {
        let mut next = vec![0.0; m];
        for i in 0..m {
            for (j, vj) in v.iter().enumerate() {
                next[i] += corr[i][j] * *vj;
            }
        }
        let norm = next.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm <= 1e-12 {
            break;
        }
        for x in &mut next {
            *x /= norm;
        }
        v = next;
    }

    let sign = if v.iter().sum::<f64>() >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let mut out = HashMap::new();
    for (idx, &symbol) in symbols.iter().enumerate() {
        out.insert(symbol.to_string(), v[idx] * sign);
    }
    out
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n < 3 {
        return 0.0;
    }
    let a = &a[..n];
    let b = &b[..n];
    let mean_a = a.iter().sum::<f64>() / n as f64;
    let mean_b = b.iter().sum::<f64>() / n as f64;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for (&xa, &xb) in a.iter().zip(b.iter()) {
        cov += (xa - mean_a) * (xb - mean_b);
        var_a += (xa - mean_a) * (xa - mean_a);
        var_b += (xb - mean_b) * (xb - mean_b);
    }
    let denom = (var_a * var_b).sqrt();
    if denom <= 1e-12 {
        0.0
    } else {
        cov / denom
    }
}

fn close_returns(close: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; close.len()];
    for i in 1..close.len() {
        let prev = close[i - 1];
        let curr = close[i];
        if prev > 0.0 && curr > 0.0 {
            out[i] = curr / prev - 1.0;
        }
    }
    out
}

fn eigenfactor_residual_return(
    asset: &[f64],
    btc: &[f64],
    eigenfactor: &[f64],
    i: usize,
    lookback: usize,
    beta_lookback: usize,
) -> Option<f64> {
    let asset_ret = pct_return(asset, i, lookback)?;
    let btc_ret = pct_return(btc, i, lookback)?;
    let eigenfactor_ret = eigenfactor.get(i).copied()?;
    if !eigenfactor_ret.is_finite() {
        return None;
    }
    let (beta_btc, beta_eigenfactor) =
        rolling_two_factor_betas(asset, btc, eigenfactor, i, lookback, beta_lookback)?;
    Some(asset_ret - beta_btc * btc_ret - beta_eigenfactor * eigenfactor_ret)
}

fn rolling_two_factor_betas(
    asset: &[f64],
    btc: &[f64],
    cluster_factor: &[f64],
    i: usize,
    lookback: usize,
    beta_lookback: usize,
) -> Option<(f64, f64)> {
    if i < lookback + beta_lookback {
        return None;
    }
    let start = i - beta_lookback + 1;
    let mut x1 = Vec::with_capacity(beta_lookback);
    let mut x2 = Vec::with_capacity(beta_lookback);
    let mut y = Vec::with_capacity(beta_lookback);
    for t in start..=i {
        let asset_ret = pct_return(asset, t, lookback)?;
        let btc_ret = pct_return(btc, t, lookback)?;
        let cluster_ret = cluster_factor.get(t).copied()?;
        if !cluster_ret.is_finite() {
            return None;
        }
        x1.push(btc_ret);
        x2.push(cluster_ret);
        y.push(asset_ret);
    }
    Some(solve_two_factor_beta(&x1, &x2, &y))
}

fn solve_two_factor_beta(x1: &[f64], x2: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x1.len().min(x2.len()).min(y.len());
    if n < 8 {
        return (0.0, 0.0);
    }
    let (mut s11, mut s22, mut s12, mut t1, mut t2) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for idx in 0..n {
        s11 += x1[idx] * x1[idx];
        s22 += x2[idx] * x2[idx];
        s12 += x1[idx] * x2[idx];
        t1 += x1[idx] * y[idx];
        t2 += x2[idx] * y[idx];
    }
    let det = s11 * s22 - s12 * s12;
    if det.abs() < 1e-12 {
        return (0.0, 0.0);
    }
    let beta1 = (t1 * s22 - t2 * s12) / det;
    let beta2 = (s11 * t2 - s12 * t1) / det;
    (beta1, beta2)
}

fn pct_return(values: &[f64], i: usize, lookback: usize) -> Option<f64> {
    if i < lookback {
        return None;
    }
    let prev = values.get(i - lookback).copied().unwrap_or(0.0);
    let now = values.get(i).copied().unwrap_or(0.0);
    if prev <= 0.0 || now <= 0.0 {
        None
    } else {
        Some(now / prev - 1.0)
    }
}

fn lookback_return(close: &[f64], end: usize, lookback: usize) -> f64 {
    if end < lookback {
        return 0.0;
    }
    let past = close[end - lookback];
    let curr = close[end];
    if past > 0.0 && curr > 0.0 {
        curr / past - 1.0
    } else {
        0.0
    }
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

fn write_snapshot_files(
    summaries: &[UniverseSummary],
    win_rows: &[(StrategyKind, usize)],
) -> Result<()> {
    fs::create_dir_all("snapshots")?;
    let timestamp = std::process::Command::new("date")
        .arg("-u")
        .arg("+%Y%m%dT%H%M%SZ")
        .output()?;
    let ts = String::from_utf8_lossy(&timestamp.stdout)
        .trim()
        .to_string();

    let mut md = String::new();
    md.push_str("# Residual Momentum Benchmark\n\n");
    md.push_str("Residualized momentum score = symbol 21-bar return - rolling BTC beta × BTC 21-bar return.\n\n");
    for summary in summaries {
        md.push_str(&format!("## {}\n\n", summary.label));
        md.push_str("| Strategy | Return | Trades | Win rate | WF | Resamples |\n");
        md.push_str("|---|---:|---:|---:|---:|---:|\n");
        for row in &summary.rows {
            md.push_str(&format!(
                "| {} | {:.1}% | {} | {:.1}% | {}/{} | {}/{} |\n",
                row.kind.name(),
                row.full_return_pct,
                row.full_trades,
                row.full_win_rate * 100.0,
                row.wf_passed,
                row.wf_total,
                row.resample_passed,
                row.resample_total,
            ));
        }
        md.push('\n');
    }
    md.push_str("## Universe wins\n\n");
    for (kind, wins) in win_rows {
        md.push_str(&format!(
            "- {}: {}/{} universes\n",
            kind.name(),
            wins,
            STRESS_UNIVERSES.len()
        ));
    }

    let mut csv = String::from("universe,strategy,return_pct,trades,win_rate,wf_passed,wf_total,resample_passed,resample_total\n");
    for summary in summaries {
        for row in &summary.rows {
            csv.push_str(&format!(
                "{},{},{:.4},{},{:.6},{},{},{},{}\n",
                summary.label,
                row.kind.name(),
                row.full_return_pct,
                row.full_trades,
                row.full_win_rate,
                row.wf_passed,
                row.wf_total,
                row.resample_passed,
                row.resample_total,
            ));
        }
    }

    fs::write(
        "snapshots/eigenfactor_residual_momentum_benchmark_latest.md",
        &md,
    )?;
    fs::write(
        "snapshots/eigenfactor_residual_momentum_benchmark_latest.csv",
        &csv,
    )?;
    fs::write(
        format!(
            "snapshots/eigenfactor_residual_momentum_benchmark_{}.md",
            ts
        ),
        &md,
    )?;
    fs::write(
        format!(
            "snapshots/eigenfactor_residual_momentum_benchmark_{}.csv",
            ts
        ),
        &csv,
    )?;
    Ok(())
}

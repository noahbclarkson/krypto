//! Residualized stable-peer cross-impact benchmark.
//!
//! Purpose:
//! - follow up the surviving old-guard interaction niche without reopening a local threshold loop
//! - ask whether the residualized cross-impact signal gets cleaner when each target only listens
//!   to peers whose lagged relationship is stable across two calibration halves
//! - keep the same fair daily harness used elsewhere

use anyhow::Result;
use krypto::{
    algo::{strategies::CrossSectionalMomentum, SignalGenerator},
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::{HashMap, HashSet};

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 30;
const CS_LOOKBACK: usize = 63;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const WARMUP_BARS: usize = 200;
const PEER_LOOKBACK: usize = 3;
const BTC_BETA_LOOKBACK: usize = 252;
const STABLE_CALIBRATION_BARS: usize = 1200;
const STABLE_TOP_K: usize = 2;

const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
    "EOSUSDT", "BCHUSDT",
];

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
    CrossImpactResidualAll,
    CrossImpactResidualStableTop2,
    CrossImpactResidualStableConsensus,
    CrossSectionalMomentum,
    MacdRegime,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::CrossImpactResidualAll => "CrossImpact residual all",
            Self::CrossImpactResidualStableTop2 => "Residual stable top2",
            Self::CrossImpactResidualStableConsensus => "Residual stable consensus",
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::MacdRegime => "MACD+Regime",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::CrossImpactResidualAll,
            Self::CrossImpactResidualStableTop2,
            Self::CrossImpactResidualStableConsensus,
            Self::CrossSectionalMomentum,
            Self::MacdRegime,
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
    full_return_pct: f64,
    full_trades: usize,
    full_win_rate: f64,
    wf_passed: usize,
    wf_total: usize,
    resample_passed: usize,
    resample_total: usize,
}

#[derive(Clone)]
struct CrossImpactInputs {
    target_open: Vec<f64>,
    target_close: Vec<f64>,
    peer_closes: HashMap<String, Vec<f64>>,
    btc_close: Vec<f64>,
}

#[derive(Clone, Default)]
struct StablePeerSelection {
    stable_peers: HashSet<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== CROSS-IMPACT RESIDUAL STABLE-PEER BENCHMARK ===\n");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Goal: test whether old-guard interaction survives when peers must be stable across calibration halves\n");

    let loader = DataLoader::new(None, None);
    let mut base_cache = HashMap::<String, DataFrame>::new();
    for &symbol in LOAD_SYMBOLS {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", enriched.height());
        base_cache.insert(symbol.to_string(), enriched);
    }

    let mut cs_map = base_cache.clone();
    compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;

    let mut strategy_universe_wins = HashMap::<StrategyKind, usize>::new();
    for &strategy in StrategyKind::all() {
        strategy_universe_wins.insert(strategy, 0);
    }

    for &(label, symbols) in STRESS_UNIVERSES {
        let quarter_windows = quarter_windows(&cs_map, symbols)?;
        let resample_sets = cpcv_style_windows(&cs_map, symbols)?;
        let inputs = build_cross_impact_inputs(&cs_map, symbols)?;
        let stable = build_stable_peer_map(&inputs)?;

        let mut rows = Vec::new();
        for &strategy in StrategyKind::all() {
            let full = evaluate_strategy(&cs_map, &inputs, &stable, strategy, symbols)?;

            let mut wf_passed = 0usize;
            for &(start, end) in &quarter_windows {
                let eval = evaluate_strategy_on_windows(
                    &cs_map,
                    &inputs,
                    &stable,
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
                let eval = evaluate_strategy_on_windows(
                    &cs_map, &inputs, &stable, strategy, symbols, windows,
                )?;
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

        println!("\n=== {} ===", label);
        println!(
            "{:<31} {:>10} {:>8} {:>8} {:>10} {:>12}",
            "Strategy", "Return%", "Trades", "Win%", "WF", "Resamples"
        );
        for row in &rows {
            println!(
                "{:<31} {:>10.1} {:>8} {:>7.1}% {:>4}/{} {:>6}/{}",
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

        println!("Stable peer selections:");
        for &symbol in symbols {
            let peers = stable
                .get(symbol)
                .map(|s| {
                    let mut v: Vec<_> = s.stable_peers.iter().cloned().collect();
                    v.sort();
                    v.join(",")
                })
                .unwrap_or_else(|| "<none>".to_string());
            println!("- {:<12} {}", symbol, peers);
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
            "- {:<31} {}/{} universes",
            kind.name(),
            wins,
            STRESS_UNIVERSES.len()
        );
    }

    println!("\nInterpretation:");
    println!("- If stable-peer residual variants hold up on OldGuardNoBNB, the interaction niche is more structural than broad basket confirmation alone.");
    println!("- If they collapse versus residual-all, the surviving edge likely still depends on broad peer context rather than stable target-local links.");

    Ok(())
}

fn build_cross_impact_inputs(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<HashMap<String, CrossImpactInputs>> {
    let include: HashSet<&str> = symbols.iter().copied().collect();
    let mut closes = HashMap::<String, Vec<f64>>::new();
    let mut opens = HashMap::<String, Vec<f64>>::new();
    let n = min_rows(data_cache, &[symbols, &["BTCUSDT"]].concat())?;

    for &symbol in symbols.iter().chain(std::iter::once(&"BTCUSDT")) {
        let df = data_cache.get(symbol).unwrap();
        closes.insert(
            symbol.to_string(),
            df.column("close")?
                .f64()?
                .into_iter()
                .take(n)
                .map(|v| v.unwrap_or(0.0))
                .collect(),
        );
        opens.insert(
            symbol.to_string(),
            df.column("open")?
                .f64()?
                .into_iter()
                .take(n)
                .map(|v| v.unwrap_or(0.0))
                .collect(),
        );
    }

    let btc_close = closes.get("BTCUSDT").unwrap().clone();
    let mut out = HashMap::new();
    for &symbol in symbols {
        let mut peer_closes = HashMap::new();
        for &peer in symbols {
            if peer != symbol && include.contains(peer) {
                peer_closes.insert(peer.to_string(), closes.get(peer).unwrap().clone());
            }
        }
        out.insert(
            symbol.to_string(),
            CrossImpactInputs {
                target_open: opens.get(symbol).unwrap().clone(),
                target_close: closes.get(symbol).unwrap().clone(),
                peer_closes,
                btc_close: btc_close.clone(),
            },
        );
    }
    Ok(out)
}

fn build_stable_peer_map(
    inputs: &HashMap<String, CrossImpactInputs>,
) -> Result<HashMap<String, StablePeerSelection>> {
    let mut out = HashMap::new();
    for (symbol, inp) in inputs {
        let n = inp.target_close.len();
        let calibration_end = STABLE_CALIBRATION_BARS.min(n.saturating_sub(HOLD_BARS + 2));
        let half = calibration_end / 2;
        let first = stable_peer_rank(inp, PEER_LOOKBACK.max(BTC_BETA_LOOKBACK), half)?;
        let second = stable_peer_rank(inp, half, calibration_end)?;

        let mut stable = HashSet::new();
        for peer in first {
            if second.contains(&peer) {
                stable.insert(peer);
            }
        }
        out.insert(
            symbol.clone(),
            StablePeerSelection {
                stable_peers: stable,
            },
        );
    }
    Ok(out)
}

fn stable_peer_rank(inp: &CrossImpactInputs, start: usize, end: usize) -> Result<Vec<String>> {
    let mut scores = Vec::<(String, f64)>::new();
    for (peer, closes) in &inp.peer_closes {
        let mut sum = 0.0;
        let mut count = 0usize;
        for i in start.max(PEER_LOOKBACK.max(BTC_BETA_LOOKBACK))..end {
            if i + HOLD_BARS >= inp.target_close.len() {
                break;
            }
            let peer_ret =
                residualized_return(closes, &inp.btc_close, i, PEER_LOOKBACK, BTC_BETA_LOOKBACK);
            let fut = future_open_return(&inp.target_close, i, HOLD_BARS);
            if let (Some(a), Some(b)) = (peer_ret, fut) {
                sum += a * b;
                count += 1;
            }
        }
        if count > 50 {
            scores.push((peer.clone(), sum / count as f64));
        }
    }
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    Ok(scores
        .into_iter()
        .filter(|(_, score)| *score > 0.0)
        .take(STABLE_TOP_K)
        .map(|(peer, _)| peer)
        .collect())
}

fn future_open_return(target_close: &[f64], i: usize, hold_bars: usize) -> Option<f64> {
    if i + hold_bars >= target_close.len() {
        return None;
    }
    let entry = *target_close.get(i)?;
    let exit = *target_close.get(i + hold_bars)?;
    if entry > 0.0 && exit > 0.0 {
        Some(exit / entry - 1.0)
    } else {
        None
    }
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    cross_inputs: &HashMap<String, CrossImpactInputs>,
    stable_peers: &HashMap<String, StablePeerSelection>,
    strategy: StrategyKind,
    symbols: &[&str],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        let inputs = cross_inputs.get(symbol).unwrap();
        let stable = stable_peers.get(symbol).unwrap();
        let result = run_strategy(df, inputs, stable, strategy)?;
        aggregate.add_assign(&result);
    }
    Ok(aggregate)
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    cross_inputs: &HashMap<String, CrossImpactInputs>,
    stable_peers: &HashMap<String, StablePeerSelection>,
    strategy: StrategyKind,
    symbols: &[&str],
    windows: &[(usize, usize)],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        let inputs = cross_inputs.get(symbol).unwrap();
        let stable = stable_peers.get(symbol).unwrap();
        for &(start, end) in windows {
            let result = run_strategy_in_window(df, inputs, stable, strategy, start, end)?;
            aggregate.add_assign(&result);
        }
    }
    Ok(aggregate)
}

fn quarter_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<Vec<(usize, usize)>> {
    let n = min_rows(data_cache, &[symbols, &["BTCUSDT"]].concat())?;
    let quarter = n / 4;
    Ok((0..4)
        .map(|idx| {
            let start = idx * quarter;
            let end = if idx == 3 { n } else { (idx + 1) * quarter };
            (start, end)
        })
        .collect())
}

fn cpcv_style_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<Vec<Vec<(usize, usize)>>> {
    let n = min_rows(data_cache, &[symbols, &["BTCUSDT"]].concat())?;
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

fn min_rows(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    symbols
        .iter()
        .map(|symbol| data_cache.get(*symbol).map(|df| df.height()).unwrap_or(0))
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty symbol set"))
}

fn run_strategy(
    df: &DataFrame,
    inputs: &CrossImpactInputs,
    stable: &StablePeerSelection,
    strategy: StrategyKind,
) -> Result<StrategyResult> {
    let signals = generate_signals(df, inputs, stable, strategy)?;
    backtest_fixed_hold_next_open_window(inputs, &signals, 0, df.height())
}

fn run_strategy_in_window(
    df: &DataFrame,
    inputs: &CrossImpactInputs,
    stable: &StablePeerSelection,
    strategy: StrategyKind,
    start: usize,
    end: usize,
) -> Result<StrategyResult> {
    let signals = generate_signals(df, inputs, stable, strategy)?;
    backtest_fixed_hold_next_open_window(inputs, &signals, start, end)
}

fn generate_signals(
    df: &DataFrame,
    inputs: &CrossImpactInputs,
    stable: &StablePeerSelection,
    strategy: StrategyKind,
) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::CrossImpactResidualAll => {
            Ok(generate_cross_impact_signals(inputs, None, false))
        }
        StrategyKind::CrossImpactResidualStableTop2 => {
            Ok(generate_cross_impact_signals(inputs, Some(stable), false))
        }
        StrategyKind::CrossImpactResidualStableConsensus => {
            Ok(generate_cross_impact_signals(inputs, Some(stable), true))
        }
        StrategyKind::CrossSectionalMomentum => {
            let series = CrossSectionalMomentum::new().predict(df)?;
            if let Ok(ca) = series.i32() {
                Ok(ca.into_iter().map(|v| v.unwrap_or(0)).collect())
            } else {
                Ok(series
                    .f64()?
                    .into_iter()
                    .map(|v| v.unwrap_or(0.0).round() as i32)
                    .collect())
            }
        }
        StrategyKind::MacdRegime => Ok(generate_macd_regime_signals(df)?),
    }
}

fn generate_cross_impact_signals(
    inputs: &CrossImpactInputs,
    stable: Option<&StablePeerSelection>,
    consensus_only: bool,
) -> Vec<i32> {
    let n = inputs.target_close.len();
    let mut out = vec![0; n];

    for i in PEER_LOOKBACK.max(BTC_BETA_LOOKBACK)..n {
        let self_ret = residualized_return(
            &inputs.target_close,
            &inputs.btc_close,
            i,
            PEER_LOOKBACK,
            BTC_BETA_LOOKBACK,
        )
        .unwrap_or(0.0);
        if self_ret.abs() > 0.012 {
            continue;
        }

        let mut peer_sum = 0.0;
        let mut peer_count = 0usize;
        let mut sign_votes = 0i32;
        for (peer, closes) in &inputs.peer_closes {
            if let Some(stable_sel) = stable {
                if !stable_sel.stable_peers.contains(peer) {
                    continue;
                }
            }
            if let Some(ret) = residualized_return(
                closes,
                &inputs.btc_close,
                i,
                PEER_LOOKBACK,
                BTC_BETA_LOOKBACK,
            ) {
                peer_sum += ret;
                peer_count += 1;
                if ret > 0.0 {
                    sign_votes += 1;
                } else if ret < 0.0 {
                    sign_votes -= 1;
                }
            }
        }
        if peer_count < 2 {
            continue;
        }
        let peer_avg = peer_sum / peer_count as f64;
        let threshold = 0.020;
        let consensus_gate = if consensus_only {
            sign_votes.unsigned_abs() as usize == peer_count
        } else {
            sign_votes.unsigned_abs() as usize >= 2
        };
        if !consensus_gate {
            continue;
        }

        if peer_avg >= threshold {
            out[i] = 1;
        } else if peer_avg <= -threshold {
            out[i] = -1;
        }
    }

    out
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

fn residualized_return(
    asset: &[f64],
    btc: &[f64],
    i: usize,
    lookback: usize,
    beta_lookback: usize,
) -> Option<f64> {
    let asset_ret = pct_return(asset, i, lookback)?;
    let btc_ret = pct_return(btc, i, lookback)?;
    let beta = rolling_beta(asset, btc, i, lookback, beta_lookback)?;
    Some(asset_ret - beta * btc_ret)
}

fn rolling_beta(
    asset: &[f64],
    btc: &[f64],
    i: usize,
    lookback: usize,
    beta_lookback: usize,
) -> Option<f64> {
    if i < lookback + beta_lookback {
        return None;
    }
    let start = i - beta_lookback + 1;
    let mut xs = Vec::with_capacity(beta_lookback);
    let mut ys = Vec::with_capacity(beta_lookback);
    for t in start..=i {
        let x = pct_return(btc, t, lookback)?;
        let y = pct_return(asset, t, lookback)?;
        xs.push(x);
        ys.push(y);
    }
    let mean_x = xs.iter().sum::<f64>() / xs.len() as f64;
    let mean_y = ys.iter().sum::<f64>() / ys.len() as f64;
    let mut cov = 0.0;
    let mut var = 0.0;
    for (&x, &y) in xs.iter().zip(ys.iter()) {
        cov += (x - mean_x) * (y - mean_y);
        var += (x - mean_x) * (x - mean_x);
    }
    if var.abs() < 1e-12 {
        Some(0.0)
    } else {
        Some(cov / var)
    }
}

fn backtest_fixed_hold_next_open_window(
    inputs: &CrossImpactInputs,
    signals: &[i32],
    start: usize,
    end: usize,
) -> Result<StrategyResult> {
    let n = inputs.target_open.len();
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
        let entry = inputs.target_open.get(entry_idx).copied().unwrap_or(0.0);
        let exit = inputs.target_open.get(exit_idx).copied().unwrap_or(0.0);
        if entry <= 0.0 || exit <= 0.0 {
            i += 1;
            continue;
        }

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

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    let close = df.column("close")?.f64()?;

    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0; df.height()];
    for i in 0..df.height() {
        let m = macd.get(i).unwrap_or(0.0);
        let s = signal.get(i).unwrap_or(0.0);
        let c = close.get(i).unwrap_or(0.0);
        let ma = sma_200.get(i).copied().unwrap_or(0.0);
        if c > ma && m > s {
            out[i] = 1;
        } else if c < ma && m < s {
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

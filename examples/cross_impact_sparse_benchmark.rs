//! Sparse / peer-selected cross-impact benchmark.
//!
//! Goal:
//! - follow up the first cross-impact breadth result without more threshold polishing
//! - replace blunt all-peer averaging with a simple calibrated peer-selection rule
//! - test whether the old-guard cross-impact signal survives when each target only listens
//!   to its strongest historically predictive peers
//!
//! Calibration rule:
//! - use an early fixed calibration window only
//! - for each target, score each peer by the average product of:
//!   peer lookback return at time t  *  target forward HOLD_BARS return from next open
//! - keep the top-k positive-score peers as the sparse driver set
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
use std::collections::{HashMap, HashSet};

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 30;
const CS_LOOKBACK: usize = 63;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const WARMUP_BARS: usize = 200;
const CALIBRATION_BARS: usize = 900;

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
    CrossImpactSlowAll,
    CrossImpactSparse2,
    CrossImpactSparse3,
    CrossSectionalMomentum,
    MacdRegime,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::CrossImpactSlowAll => "CrossImpact slow all",
            Self::CrossImpactSparse2 => "CrossImpact sparse top2",
            Self::CrossImpactSparse3 => "CrossImpact sparse top3",
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::MacdRegime => "MACD+Regime",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::CrossImpactSlowAll,
            Self::CrossImpactSparse2,
            Self::CrossImpactSparse3,
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
    target_close: Vec<f64>,
    target_open: Vec<f64>,
    peer_closes: HashMap<String, Vec<f64>>,
    selected_top2: Vec<String>,
    selected_top3: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== SPARSE CROSS-IMPACT BENCHMARK ===\n");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Calibration: first {} bars choose peers via lagged-peer vs future-target alignment",
        CALIBRATION_BARS
    );
    println!("Goal: test whether old-guard cross-impact survives sparse peer selection\n");

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

        println!("\n=== {} ===", label);
        for &symbol in symbols {
            let selected = &inputs.get(symbol).unwrap().selected_top3;
            println!(
                "  {:<10} peers: {}",
                symbol,
                if selected.is_empty() {
                    "<none>".to_string()
                } else {
                    selected.join(", ")
                }
            );
        }

        let mut rows = Vec::new();
        for &strategy in StrategyKind::all() {
            let full = evaluate_strategy(&cs_map, &inputs, strategy, symbols)?;

            let mut wf_passed = 0usize;
            for &(start, end) in &quarter_windows {
                let eval = evaluate_strategy_on_windows(
                    &cs_map,
                    &inputs,
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
                let eval =
                    evaluate_strategy_on_windows(&cs_map, &inputs, strategy, symbols, windows)?;
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

        println!(
            "{:<24} {:>10} {:>8} {:>8} {:>10} {:>12}",
            "Strategy", "Return%", "Trades", "Win%", "WF", "Resamples"
        );
        for row in &rows {
            println!(
                "{:<24} {:>10.1} {:>8} {:>7.1}% {:>4}/{} {:>6}/{}",
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
            "- {:<24} {}/{} universes",
            kind.name(),
            wins,
            STRESS_UNIVERSES.len()
        );
    }

    println!("\nInterpretation:");
    println!("- Sparse variants only listen to a target's best early-sample peer drivers instead of the whole basket.");
    println!("- If sparse beats all-peer mainly on old-guard baskets, that supports the idea that the first result came from specific legacy links rather than broad alt-beta.");
    println!("- If sparse collapses, the earlier cross-impact win was probably too dependent on blunt basket averaging.");

    Ok(())
}

fn build_cross_impact_inputs(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<HashMap<String, CrossImpactInputs>> {
    let include: HashSet<&str> = symbols.iter().copied().collect();
    let mut closes = HashMap::<String, Vec<f64>>::new();
    let mut opens = HashMap::<String, Vec<f64>>::new();
    let n = min_rows(data_cache, symbols)?;

    for &symbol in symbols {
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

    let mut out = HashMap::new();
    for &symbol in symbols {
        let mut peer_closes = HashMap::new();
        for &peer in symbols {
            if peer != symbol && include.contains(peer) {
                peer_closes.insert(peer.to_string(), closes.get(peer).unwrap().clone());
            }
        }

        let target_close = closes.get(symbol).unwrap().clone();
        let target_open = opens.get(symbol).unwrap().clone();
        let ranked = rank_peer_scores(
            &target_close,
            &target_open,
            &peer_closes,
            3,
            CALIBRATION_BARS,
            n,
        );
        let selected_top2 = ranked
            .iter()
            .take(2)
            .map(|(peer, _)| peer.clone())
            .collect();
        let selected_top3 = ranked
            .iter()
            .take(3)
            .map(|(peer, _)| peer.clone())
            .collect();

        out.insert(
            symbol.to_string(),
            CrossImpactInputs {
                target_close,
                target_open,
                peer_closes,
                selected_top2,
                selected_top3,
            },
        );
    }
    Ok(out)
}

fn rank_peer_scores(
    target_close: &[f64],
    target_open: &[f64],
    peer_closes: &HashMap<String, Vec<f64>>,
    peer_lookback: usize,
    calibration_bars: usize,
    n: usize,
) -> Vec<(String, f64)> {
    let calib_end = calibration_bars.min(n.saturating_sub(HOLD_BARS + 1));
    let mut scores = Vec::new();

    for (peer, closes) in peer_closes {
        let mut sum = 0.0;
        let mut count = 0usize;
        for i in peer_lookback.max(WARMUP_BARS)..calib_end {
            let peer_ret = match pct_return(closes, i, peer_lookback) {
                Some(v) => v,
                None => continue,
            };
            let entry_idx = i + 1;
            let exit_idx = i + 1 + HOLD_BARS;
            let entry = target_open.get(entry_idx).copied().unwrap_or(0.0);
            let exit = target_open.get(exit_idx).copied().unwrap_or(0.0);
            if entry <= 0.0 || exit <= 0.0 {
                continue;
            }
            let fwd = exit / entry - 1.0;
            sum += peer_ret * fwd;
            count += 1;
        }
        let score = if count == 0 { 0.0 } else { sum / count as f64 };
        if score > 0.0 {
            scores.push((peer.clone(), score));
        }
    }

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scores
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    cross_inputs: &HashMap<String, CrossImpactInputs>,
    strategy: StrategyKind,
    symbols: &[&str],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        let inputs = cross_inputs.get(symbol).unwrap();
        let result = run_strategy(df, inputs, strategy)?;
        aggregate.add_assign(&result);
    }
    Ok(aggregate)
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    cross_inputs: &HashMap<String, CrossImpactInputs>,
    strategy: StrategyKind,
    symbols: &[&str],
    windows: &[(usize, usize)],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        let inputs = cross_inputs.get(symbol).unwrap();
        for &(start, end) in windows {
            let result = run_strategy_in_window(df, inputs, strategy, start, end)?;
            aggregate.add_assign(&result);
        }
    }
    Ok(aggregate)
}

fn quarter_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<Vec<(usize, usize)>> {
    let n = min_rows(data_cache, symbols)?;
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
    let n = min_rows(data_cache, symbols)?;
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
    strategy: StrategyKind,
) -> Result<StrategyResult> {
    let signals = generate_signals(df, inputs, strategy)?;
    backtest_fixed_hold_next_open_window(inputs, &signals, 0, df.height())
}

fn run_strategy_in_window(
    df: &DataFrame,
    inputs: &CrossImpactInputs,
    strategy: StrategyKind,
    start: usize,
    end: usize,
) -> Result<StrategyResult> {
    let signals = generate_signals(df, inputs, strategy)?;
    backtest_fixed_hold_next_open_window(inputs, &signals, start, end)
}

fn generate_signals(
    df: &DataFrame,
    inputs: &CrossImpactInputs,
    strategy: StrategyKind,
) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::CrossImpactSlowAll => {
            Ok(generate_cross_impact_signals(inputs, 3, 0.040, 0.015, None))
        }
        StrategyKind::CrossImpactSparse2 => Ok(generate_cross_impact_signals(
            inputs,
            3,
            0.040,
            0.015,
            Some(&inputs.selected_top2),
        )),
        StrategyKind::CrossImpactSparse3 => Ok(generate_cross_impact_signals(
            inputs,
            3,
            0.040,
            0.015,
            Some(&inputs.selected_top3),
        )),
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
    peer_lookback: usize,
    peer_threshold: f64,
    self_cap: f64,
    selected_peers: Option<&[String]>,
) -> Vec<i32> {
    let n = inputs.target_close.len();
    let mut out = vec![0; n];

    for i in peer_lookback..n {
        let self_ret = pct_return(&inputs.target_close, i, peer_lookback).unwrap_or(0.0);
        if self_ret.abs() > self_cap {
            continue;
        }

        let peer_names: Vec<&String> = match selected_peers {
            Some(v) if !v.is_empty() => v.iter().collect(),
            _ => inputs.peer_closes.keys().collect(),
        };

        let mut peer_sum = 0.0;
        let mut peer_count = 0usize;
        let mut same_sign = 0i32;
        for peer in peer_names {
            let Some(closes) = inputs.peer_closes.get(peer) else {
                continue;
            };
            if let Some(ret) = pct_return(closes, i, peer_lookback) {
                peer_sum += ret;
                peer_count += 1;
                if ret > 0.0 {
                    same_sign += 1;
                } else if ret < 0.0 {
                    same_sign -= 1;
                }
            }
        }
        if peer_count < 2 {
            continue;
        }

        let peer_avg = peer_sum / peer_count as f64;
        let sign_gate = if peer_count >= 3 { 2 } else { 1 };
        let raw_signal = if peer_avg >= peer_threshold && same_sign >= sign_gate {
            1
        } else if peer_avg <= -peer_threshold && same_sign <= -sign_gate {
            -1
        } else {
            0
        };

        out[i] = raw_signal;
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

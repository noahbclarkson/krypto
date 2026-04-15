//! Stress-test leader-laggard / cross-asset propagation variants under the same fair daily harness.
//!
//! Goal: give the lead-lag family a fairer follow-up than the single default configuration
//! from the earlier non-trend scan, and see whether any propagation-style expression is
//! robust enough to deserve more research time.
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry at next open
//! - exit at open after fixed 21-bar hold
//! - 0.1% taker fee on entry and exit

use anyhow::Result;
use krypto::{
    algo::{
        strategies::{CrossSectionalMomentum, LeadLagStrategy},
        SignalGenerator,
    },
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 30;
const CS_LOOKBACK: usize = 63;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const WARMUP_BARS: usize = 200;

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
    LeadLagBtcFast,
    LeadLagBtcBase,
    LeadLagBtcSlow,
    LeadLagEthBase,
    CrossSectionalMomentum,
    MacdRegime,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::LeadLagBtcFast => "LeadLag BTC fast",
            Self::LeadLagBtcBase => "LeadLag BTC base",
            Self::LeadLagBtcSlow => "LeadLag BTC slow",
            Self::LeadLagEthBase => "LeadLag ETH base",
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::MacdRegime => "MACD+Regime",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::LeadLagBtcFast,
            Self::LeadLagBtcBase,
            Self::LeadLagBtcSlow,
            Self::LeadLagEthBase,
            Self::CrossSectionalMomentum,
            Self::MacdRegime,
        ]
    }

    fn benchmark(&self) -> Option<&'static str> {
        match self {
            Self::LeadLagBtcFast | Self::LeadLagBtcBase | Self::LeadLagBtcSlow => Some("BTCUSDT"),
            Self::LeadLagEthBase => Some("ETHUSDT"),
            Self::CrossSectionalMomentum | Self::MacdRegime => Some("BTCUSDT"),
        }
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
    println!("=== LEADER-LAGGARD STRESS TEST ===\n");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Cross-sectional lookback (for CS momentum comparator): {} bars\n",
        CS_LOOKBACK
    );

    let loader = DataLoader::new(None, None);
    let mut base_cache = HashMap::<String, DataFrame>::new();
    for &symbol in LOAD_SYMBOLS {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", enriched.height());
        base_cache.insert(symbol.to_string(), enriched);
    }

    let mut benchmarked_cache = HashMap::<&'static str, HashMap<String, DataFrame>>::new();
    for &benchmark in &["BTCUSDT", "ETHUSDT"] {
        benchmarked_cache.insert(
            benchmark,
            prepare_benchmarked_cache(&base_cache, benchmark)?,
        );
    }

    let mut summaries = Vec::new();
    let mut strategy_universe_wins = HashMap::<StrategyKind, usize>::new();
    for &strategy in StrategyKind::all() {
        strategy_universe_wins.insert(strategy, 0);
    }

    for &(label, symbols) in STRESS_UNIVERSES {
        let quarter_windows = quarter_windows(&benchmarked_cache["BTCUSDT"], symbols)?;
        let resample_sets = cpcv_style_windows(&benchmarked_cache["BTCUSDT"], symbols)?;
        let mut rows = Vec::new();

        for &strategy in StrategyKind::all() {
            let benchmark = strategy.benchmark().unwrap();
            let data_cache = benchmarked_cache.get(benchmark).unwrap();
            let full = evaluate_strategy(data_cache, strategy, symbols)?;

            let mut wf_passed = 0usize;
            for &(start, end) in &quarter_windows {
                let eval =
                    evaluate_strategy_on_windows(data_cache, strategy, symbols, &[(start, end)])?;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    wf_passed += 1;
                }
            }

            let mut rs_passed = 0usize;
            for windows in &resample_sets {
                let eval = evaluate_strategy_on_windows(data_cache, strategy, symbols, windows)?;
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
            "{:<24} {:>10} {:>8} {:>8} {:>10} {:>12}",
            "Strategy", "Return%", "Trades", "Win%", "WF", "Resamples"
        );
        for row in &summary.rows {
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
    println!("- This is a breadth / propagation-family stress test, not a deployment decision.");
    println!("- If all lead-lag variants stay weak versus the same comparator set, the family probably does not deserve near-term priority under current daily assumptions.");
    println!("- If one variant survives legacy baskets materially better than the others, it earns deeper follow-up and attribution work.");

    Ok(())
}

fn prepare_benchmarked_cache(
    base_cache: &HashMap<String, DataFrame>,
    benchmark: &str,
) -> Result<HashMap<String, DataFrame>> {
    let bench_df = base_cache
        .get(benchmark)
        .ok_or_else(|| anyhow::anyhow!("missing benchmark {}", benchmark))?;

    let mut cache = HashMap::new();
    for (symbol, raw_df) in base_cache {
        let enriched = if symbol == benchmark {
            FeatureEngine::add_technicals(raw_df, None)?
        } else {
            FeatureEngine::add_technicals(raw_df, Some(bench_df))?
        };
        cache.insert(symbol.clone(), enriched);
    }

    let mut cs_map = HashMap::<String, DataFrame>::new();
    for (symbol, df) in &cache {
        if symbol != benchmark {
            cs_map.insert(symbol.clone(), df.clone());
        }
    }
    compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;
    for (symbol, df) in cs_map {
        cache.insert(symbol, df);
    }

    Ok(cache)
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
    symbols: &[&str],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        let result = run_strategy(df, strategy)?;
        aggregate.add_assign(&result);
    }
    Ok(aggregate)
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
    symbols: &[&str],
    windows: &[(usize, usize)],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        for &(start, end) in windows {
            let result = run_strategy_in_window(df, strategy, start, end)?;
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

fn run_strategy(df: &DataFrame, strategy: StrategyKind) -> Result<StrategyResult> {
    let signals = generate_signals(df, strategy)?;
    backtest_fixed_hold_next_open_window(df, &signals, 0, df.height())
}

fn run_strategy_in_window(
    df: &DataFrame,
    strategy: StrategyKind,
    start: usize,
    end: usize,
) -> Result<StrategyResult> {
    let signals = generate_signals(df, strategy)?;
    backtest_fixed_hold_next_open_window(df, &signals, start, end)
}

fn generate_signals(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    let series = match strategy {
        StrategyKind::LeadLagBtcFast => LeadLagStrategy {
            min_bench_move: 0.02,
            max_self_move: 0.004,
            decay: 2.0,
        }
        .predict(df)?,
        StrategyKind::LeadLagBtcBase | StrategyKind::LeadLagEthBase => {
            LeadLagStrategy::new().predict(df)?
        }
        StrategyKind::LeadLagBtcSlow => LeadLagStrategy {
            min_bench_move: 0.03,
            max_self_move: 0.01,
            decay: 5.0,
        }
        .predict(df)?,
        StrategyKind::CrossSectionalMomentum => CrossSectionalMomentum::new().predict(df)?,
        StrategyKind::MacdRegime => Series::new("signal", generate_macd_regime_signals(df)?),
    };

    if let Ok(ca) = series.i32() {
        return Ok(ca.into_iter().map(|v| v.unwrap_or(0)).collect::<Vec<i32>>());
    }

    Ok(series
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0).round() as i32)
        .collect::<Vec<i32>>())
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

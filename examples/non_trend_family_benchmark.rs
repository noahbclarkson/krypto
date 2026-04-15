//! Non-trend family benchmark under the same fair execution assumptions.
//!
//! Goal: broaden search beyond the MACD / Turtle family without abandoning
//! apples-to-apples discipline.
//!
//! Universe:
//! - Non-BTC majors/alts: ETH, SOL, XRP, DOGE, ADA
//! - BTC is used as the benchmark / leader for relative-strength and lead-lag features
//!
//! Execution assumptions:
//! - Signals generated from bar close and prior history only
//! - Entry at next bar open
//! - Exit at open after a fixed 21-bar hold
//! - 0.1% taker fee on entry and exit

use anyhow::Result;
use krypto::{
    algo::{
        strategies::{
            CrossSectionalMeanReversion, CrossSectionalMomentum, LeadLagStrategy,
            RelativeStrengthStrat,
        },
        SignalGenerator,
    },
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;

const BENCHMARK: &str = "BTCUSDT";
const SYMBOLS: &[&str] = &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];
const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 30;
const CS_LOOKBACK: usize = 63;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    RelativeStrength,
    LeadLag,
    CrossSectionalMomentum,
    CrossSectionalMeanReversion,
    Macd,
    MacdRegime,
    TurtleRegime,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::RelativeStrength => "RelativeStrength(BTC benchmark)",
            Self::LeadLag => "LeadLag(BTC benchmark)",
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::CrossSectionalMeanReversion => "CrossSectionalMeanReversion",
            Self::Macd => "MACD",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegime => "Turtle+Regime",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::RelativeStrength,
            Self::LeadLag,
            Self::CrossSectionalMomentum,
            Self::CrossSectionalMeanReversion,
            Self::Macd,
            Self::MacdRegime,
            Self::TurtleRegime,
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
    wf_avg_return_pct: f64,
    resample_passed: usize,
    resample_total: usize,
    resample_avg_return_pct: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== NON-TREND FAMILY BENCHMARK ===\n");
    println!("Universe: {}", SYMBOLS.join(", "));
    println!("Benchmark leader for relative features: {}", BENCHMARK);
    println!("Cross-sectional lookback: {} bars", CS_LOOKBACK);
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side\n", TAKER_FEE * 100.0);

    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;

    let mut data_cache = HashMap::<String, DataFrame>::new();
    data_cache.insert(BENCHMARK.to_string(), bench_df.clone());

    for &symbol in LOAD_SYMBOLS.iter().filter(|&&s| s != BENCHMARK) {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, Some(&bench_df))?;
        println!("{} bars", enriched.height());
        data_cache.insert(symbol.to_string(), enriched);
    }

    let mut cs_map = HashMap::<String, DataFrame>::new();
    for &symbol in SYMBOLS {
        cs_map.insert(symbol.to_string(), data_cache.get(symbol).unwrap().clone());
    }
    compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;
    for (symbol, df) in &cs_map {
        data_cache.insert(symbol.to_string(), df.clone());
    }

    let quarter_windows = quarter_windows(&data_cache)?;
    let resample_sets = cpcv_style_windows(&data_cache)?;
    let mut rows = Vec::new();

    for &strategy in StrategyKind::all() {
        let full = evaluate_strategy(&data_cache, strategy)?;

        let mut wf_passed = 0usize;
        let mut wf_sum = 0.0;
        for &(start, end) in &quarter_windows {
            let eval = evaluate_strategy_on_windows(&data_cache, strategy, &[(start, end)])?;
            wf_sum += eval.total_return_pct;
            if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                wf_passed += 1;
            }
        }

        let mut rs_passed = 0usize;
        let mut rs_sum = 0.0;
        for windows in &resample_sets {
            let eval = evaluate_strategy_on_windows(&data_cache, strategy, windows)?;
            rs_sum += eval.total_return_pct;
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
            wf_avg_return_pct: wf_sum / quarter_windows.len() as f64,
            resample_passed: rs_passed,
            resample_total: resample_sets.len(),
            resample_avg_return_pct: rs_sum / resample_sets.len() as f64,
        });
    }

    rows.sort_by(|a, b| {
        b.resample_passed
            .cmp(&a.resample_passed)
            .then_with(|| b.wf_passed.cmp(&a.wf_passed))
            .then_with(|| {
                b.resample_avg_return_pct
                    .partial_cmp(&a.resample_avg_return_pct)
                    .unwrap()
            })
            .then_with(|| b.full_return_pct.partial_cmp(&a.full_return_pct).unwrap())
    });

    println!("\n=== RESULTS ===");
    println!(
        "{:<32} {:>10} {:>8} {:>8} {:>10} {:>12} {:>12}",
        "Strategy", "Return%", "Trades", "Win%", "WF", "Resamples", "RS Avg%"
    );
    for row in &rows {
        println!(
            "{:<32} {:>10.1} {:>8} {:>7.1}% {:>4}/{} {:>6}/{} {:>12.1}",
            row.kind.name(),
            row.full_return_pct,
            row.full_trades,
            row.full_win_rate * 100.0,
            row.wf_passed,
            row.wf_total,
            row.resample_passed,
            row.resample_total,
            row.resample_avg_return_pct,
        );
    }

    println!("\nInterpretation:");
    println!("- This is a breadth scan, not a deployment decision.");
    println!("- Non-trend families that stay non-positive under quarter/resample stress are weak search directions under current daily assumptions.");
    println!("- If a new family is competitive with the trend yardsticks here, it earns deeper follow-up, not HOF status.");

    Ok(())
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in SYMBOLS {
        let df = data_cache.get(symbol).unwrap();
        let result = run_strategy(df, strategy)?;
        aggregate.add_assign(&result);
    }
    Ok(aggregate)
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
    windows: &[(usize, usize)],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in SYMBOLS {
        let df = data_cache.get(symbol).unwrap();
        for &(start, end) in windows {
            let result = run_strategy_in_window(df, strategy, start, end)?;
            aggregate.add_assign(&result);
        }
    }
    Ok(aggregate)
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
        StrategyKind::RelativeStrength => RelativeStrengthStrat::new().predict(df)?,
        StrategyKind::LeadLag => LeadLagStrategy::new().predict(df)?,
        StrategyKind::CrossSectionalMomentum => CrossSectionalMomentum::new().predict(df)?,
        StrategyKind::CrossSectionalMeanReversion => {
            CrossSectionalMeanReversion::new().predict(df)?
        }
        StrategyKind::Macd => Series::new("signal", generate_macd_signals(df)?),
        StrategyKind::MacdRegime => Series::new("signal", generate_macd_regime_signals(df)?),
        StrategyKind::TurtleRegime => {
            Series::new("signal", generate_turtle_regime_signals(df, 20)?)
        }
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
        i = exit_idx;
    }

    Ok(StrategyResult {
        total_return_pct: trade_returns.iter().sum(),
        trades: trade_returns.len(),
        wins: trade_returns.iter().filter(|&&r| r > 0.0).count(),
    })
}

fn generate_macd_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0; df.height()];
    for i in 0..df.height() {
        let m = macd.get(i).unwrap_or(0.0);
        let s = signal.get(i).unwrap_or(0.0);
        if m > s {
            out[i] = 1;
        } else if m < s {
            out[i] = -1;
        }
    }
    Ok(out)
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

fn generate_turtle_regime_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0; df.height()];

    for i in period..df.height() {
        let highest = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let c = close.get(i).unwrap_or(0.0);
        let ma = sma_200.get(i).copied().unwrap_or(0.0);
        if c > ma && c > highest {
            out[i] = 1;
        } else if c < ma && c < lowest {
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

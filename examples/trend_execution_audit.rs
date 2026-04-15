//! Execution / trust audit for the daily trend benchmark harness.
//!
//! Goal: stress benchmark leaders under alternative execution assumptions so we
//! improve trust in the lab instead of over-refining one leaderboard winner.
//!
//! Baseline harness assumptions (from head_to_head_comparison.rs):
//! - signal generated from bar i close using history up to i only
//! - entry at next bar open
//! - exit at open after fixed 21-bar hold
//! - 0.1% taker fee each side
//!
//! This audit keeps the same strategies and universe but varies:
//! - entry / exit timing latency
//! - slippage on top of fees
//! - warmup padding

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const PERIOD: usize = 20;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const MIN_TRADES_PER_WINDOW: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    Turtle,
    Macd,
    MacdRegime,
    TurtleMacd,
    TurtleRegime,
    TurtleRegimeMacd,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::Turtle => "Turtle20",
            Self::Macd => "MACD",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleMacd => "Turtle+MACD",
            Self::TurtleRegime => "Turtle+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::Turtle,
            Self::Macd,
            Self::MacdRegime,
            Self::TurtleMacd,
            Self::TurtleRegime,
            Self::TurtleRegimeMacd,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
struct Scenario {
    name: &'static str,
    description: &'static str,
    entry_delay_bars: usize,
    exit_delay_bars: usize,
    fee_each_side: f64,
    slippage_bps_each_side: f64,
    warmup_bars: usize,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "Baseline",
        description: "next-open entry/exit, 10 bps taker fee each side, no extra slippage",
        entry_delay_bars: 1,
        exit_delay_bars: 1,
        fee_each_side: 0.001,
        slippage_bps_each_side: 0.0,
        warmup_bars: 200,
    },
    Scenario {
        name: "Slip5bps",
        description: "baseline timing + extra 5 bps slippage each side",
        entry_delay_bars: 1,
        exit_delay_bars: 1,
        fee_each_side: 0.001,
        slippage_bps_each_side: 5.0,
        warmup_bars: 200,
    },
    Scenario {
        name: "Slip10bps",
        description: "baseline timing + extra 10 bps slippage each side",
        entry_delay_bars: 1,
        exit_delay_bars: 1,
        fee_each_side: 0.001,
        slippage_bps_each_side: 10.0,
        warmup_bars: 200,
    },
    Scenario {
        name: "EntryDelay1Bar",
        description: "signal at close, but enter one extra bar late and exit one extra bar late",
        entry_delay_bars: 2,
        exit_delay_bars: 2,
        fee_each_side: 0.001,
        slippage_bps_each_side: 0.0,
        warmup_bars: 200,
    },
    Scenario {
        name: "EntryDelay1BarSlip10bps",
        description: "one extra bar of latency plus 10 bps slippage each side",
        entry_delay_bars: 2,
        exit_delay_bars: 2,
        fee_each_side: 0.001,
        slippage_bps_each_side: 10.0,
        warmup_bars: 200,
    },
    Scenario {
        name: "Warmup250",
        description: "baseline timing with stricter warmup padding",
        entry_delay_bars: 1,
        exit_delay_bars: 1,
        fee_each_side: 0.001,
        slippage_bps_each_side: 0.0,
        warmup_bars: 250,
    },
];

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
struct StrategyEval {
    result: StrategyResult,
}

#[derive(Clone, Debug)]
struct AuditRow {
    scenario: &'static str,
    strategy: &'static str,
    total_return_pct: f64,
    trades: usize,
    win_rate: f64,
    resample_passed: usize,
    resample_total: usize,
    avg_resample_return_pct: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== TREND EXECUTION AUDIT ===\n");
    println!("Universe: {}", SYMBOLS.join(", "));
    println!(
        "Hold: {} bars | Resampling: {} blocks choose {} train / {} test\n",
        HOLD_BARS,
        RESAMPLE_BLOCKS,
        RESAMPLE_TRAIN_BLOCKS,
        RESAMPLE_BLOCKS - RESAMPLE_TRAIN_BLOCKS
    );

    let loader = DataLoader::new(None, None);
    let mut data_cache = HashMap::<String, DataFrame>::new();
    for symbol in SYMBOLS {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", df.height());
        data_cache.insert(symbol.to_string(), df);
    }

    let resample_sets = cpcv_style_windows(&data_cache)?;
    let mut rows = Vec::<AuditRow>::new();

    for scenario in SCENARIOS {
        println!("\n--- {} ---", scenario.name);
        println!("{}", scenario.description);

        let mut scenario_rows = Vec::<AuditRow>::new();
        for strategy in StrategyKind::all() {
            let full_eval = evaluate_strategy(&data_cache, *strategy, SYMBOLS, scenario)?;
            let mut resample_passed = 0usize;
            let mut resample_return_sum = 0.0;

            for windows in &resample_sets {
                let eval = evaluate_strategy_on_windows(
                    &data_cache,
                    *strategy,
                    SYMBOLS,
                    windows,
                    scenario,
                )?;
                if eval.result.total_return_pct > 0.0 && eval.result.trades >= MIN_TRADES_PER_WINDOW
                {
                    resample_passed += 1;
                }
                resample_return_sum += eval.result.total_return_pct;
            }

            let row = AuditRow {
                scenario: scenario.name,
                strategy: strategy.name(),
                total_return_pct: full_eval.result.total_return_pct,
                trades: full_eval.result.trades,
                win_rate: full_eval.result.win_rate() * 100.0,
                resample_passed,
                resample_total: resample_sets.len(),
                avg_resample_return_pct: resample_return_sum / resample_sets.len() as f64,
            };
            scenario_rows.push(row.clone());
            rows.push(row);
        }

        scenario_rows.sort_by(|a, b| {
            b.resample_passed
                .cmp(&a.resample_passed)
                .then_with(|| {
                    b.avg_resample_return_pct
                        .partial_cmp(&a.avg_resample_return_pct)
                        .unwrap()
                })
                .then_with(|| b.total_return_pct.partial_cmp(&a.total_return_pct).unwrap())
        });

        for row in &scenario_rows {
            println!(
                "{:<20} ret {:>8.1}% | trades {:>4} | win {:>5.1}% | resamples {}/{} | avg test {:>7.1}%",
                row.strategy,
                row.total_return_pct,
                row.trades,
                row.win_rate,
                row.resample_passed,
                row.resample_total,
                row.avg_resample_return_pct,
            );
        }
    }

    println!("\n=== DELTA VS BASELINE ===\n");
    for strategy in StrategyKind::all() {
        let baseline = rows
            .iter()
            .find(|r| r.scenario == "Baseline" && r.strategy == strategy.name())
            .unwrap();
        println!("{}", strategy.name());
        for scenario in SCENARIOS.iter().skip(1) {
            let row = rows
                .iter()
                .find(|r| r.scenario == scenario.name && r.strategy == strategy.name())
                .unwrap();
            println!(
                "  {:<24} Δret {:>8.1}% | Δtrades {:>4} | Δresamples {:>3}",
                scenario.name,
                row.total_return_pct - baseline.total_return_pct,
                row.trades as isize - baseline.trades as isize,
                row.resample_passed as isize - baseline.resample_passed as isize,
            );
        }
        println!();
    }

    Ok(())
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
    symbols: &[&str],
    scenario: &Scenario,
) -> Result<StrategyEval> {
    let mut aggregate = StrategyEval {
        result: StrategyResult::default(),
    };

    for &symbol in symbols {
        let df = data_cache.get(symbol).expect("missing symbol in cache");
        let eval = run_strategy(df, strategy, scenario)?;
        aggregate.result.add_assign(&eval.result);
    }

    Ok(aggregate)
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    strategy: StrategyKind,
    symbols: &[&str],
    windows: &[(usize, usize)],
    scenario: &Scenario,
) -> Result<StrategyEval> {
    let mut aggregate = StrategyEval {
        result: StrategyResult::default(),
    };

    for &(start, end) in windows {
        for &symbol in symbols {
            let df = data_cache.get(symbol).expect("missing symbol in cache");
            let eval = run_strategy_in_window(df, strategy, start, end, scenario)?;
            aggregate.result.add_assign(&eval.result);
        }
    }

    Ok(aggregate)
}

fn run_strategy(
    df: &DataFrame,
    strategy: StrategyKind,
    scenario: &Scenario,
) -> Result<StrategyEval> {
    let signals = generate_signals(df, strategy)?;
    backtest_fixed_hold(df, &signals, scenario)
}

fn run_strategy_in_window(
    df: &DataFrame,
    strategy: StrategyKind,
    start: usize,
    end: usize,
    scenario: &Scenario,
) -> Result<StrategyEval> {
    let signals = generate_signals(df, strategy)?;
    backtest_fixed_hold_window(df, &signals, start, end, scenario)
}

fn generate_signals(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::Turtle => generate_turtle_signals(df, PERIOD),
        StrategyKind::Macd => generate_macd_signals(df),
        StrategyKind::MacdRegime => generate_macd_regime_signals(df),
        StrategyKind::TurtleMacd => generate_turtle_macd_signals(df, PERIOD),
        StrategyKind::TurtleRegime => generate_turtle_regime_signals(df, PERIOD),
        StrategyKind::TurtleRegimeMacd => generate_turtle_regime_macd_signals(df, PERIOD),
    }
}

fn backtest_fixed_hold(
    df: &DataFrame,
    signals: &[i32],
    scenario: &Scenario,
) -> Result<StrategyEval> {
    backtest_fixed_hold_window(df, signals, 0, df.height(), scenario)
}

fn backtest_fixed_hold_window(
    df: &DataFrame,
    signals: &[i32],
    start: usize,
    end: usize,
    scenario: &Scenario,
) -> Result<StrategyEval> {
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let mut trade_returns = Vec::new();
    let mut i = scenario.warmup_bars.max(start);
    let end = end.min(n);

    while i + scenario.entry_delay_bars + HOLD_BARS + scenario.exit_delay_bars < end {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }

        let entry_idx = i + scenario.entry_delay_bars;
        let exit_idx = i + HOLD_BARS + scenario.exit_delay_bars;
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
        let cost_pct =
            2.0 * scenario.fee_each_side * 100.0 + 2.0 * scenario.slippage_bps_each_side / 100.0;
        let net = gross - cost_pct;
        trade_returns.push(net);
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
    })
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

fn min_rows(data_cache: &HashMap<String, DataFrame>) -> Result<usize> {
    data_cache
        .values()
        .map(|df| df.height())
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty data cache"))
}

fn cpcv_style_windows(data_cache: &HashMap<String, DataFrame>) -> Result<Vec<Vec<(usize, usize)>>> {
    let n = min_rows(data_cache)?;
    let block_size = n / RESAMPLE_BLOCKS;
    let mut blocks = Vec::new();

    for block in 0..RESAMPLE_BLOCKS {
        let start = block * block_size;
        let end = if block == RESAMPLE_BLOCKS - 1 {
            n
        } else {
            (block + 1) * block_size
        };
        blocks.push((start, end));
    }

    let train_sets = choose_indices(RESAMPLE_BLOCKS, RESAMPLE_TRAIN_BLOCKS);
    let mut out = Vec::new();
    for train in train_sets {
        let mut is_train = vec![false; RESAMPLE_BLOCKS];
        for idx in train {
            is_train[idx] = true;
        }
        let test_windows = blocks
            .iter()
            .enumerate()
            .filter_map(|(idx, window)| if is_train[idx] { None } else { Some(*window) })
            .collect::<Vec<_>>();
        out.push(test_windows);
    }
    Ok(out)
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

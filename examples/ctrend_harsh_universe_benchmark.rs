//! CTREND-style multi-horizon price+volume factor on the harsh-universe harness.
//!
//! Purpose:
//! - test a more principled trend-factor proxy instead of another nearby MACD/Turtle variant
//! - keep the exact same fair execution / chronology lens as the current harsh harness
//! - learn whether current survivors are just crude wrappers around a broader price+volume trend object
//!
//! This is intentionally simple and pre-declared:
//! - no optimizer
//! - no threshold sweep
//! - same next-open / fixed-hold harness already used for the current yardsticks

use anyhow::{Context, Result};
use krypto::{
    algo::{strategies::CrossSectionalMomentum, SignalGenerator},
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::{BTreeSet, HashMap};

const BENCHMARK: &str = "BTCUSDT";
const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
    "EOSUSDT", "BCHUSDT",
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 30;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const CS_LOOKBACK: usize = 63;
const TURTLE_PERIOD: usize = 20;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
    ),
    ("NoDOGE", &["ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
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
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"],
    ),
    ("Legacy3", &["XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    ("OldGuard4", &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    CTRend,
    MacdRegime,
    TurtleMACD,
    AdMomentum,
    CrossSectionalMomentum,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::CTRend,
            Self::MacdRegime,
            Self::TurtleMACD,
            Self::AdMomentum,
            Self::CrossSectionalMomentum,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::CTRend => "CTREND(price+volume)",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleMACD => "Turtle+MACD",
            Self::AdMomentum => "A/D Momentum",
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::CTRend => "multi-horizon trend proxy with volume confirmation",
            Self::MacdRegime => "filtered trend yardstick",
            Self::TurtleMACD => "breakout trend yardstick",
            Self::AdMomentum => "volume-pressure yardstick",
            Self::CrossSectionalMomentum => "breadth yardstick",
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
    println!("=== CTREND HARSH-UNIVERSE BENCHMARK ===\n");
    println!("Benchmark leader for relative features: {}", BENCHMARK);
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Universe stress sets: {}\n", UNIVERSES.len());

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
    for &symbol in LOAD_SYMBOLS.iter().filter(|&&s| s != BENCHMARK) {
        cs_map.insert(symbol.to_string(), data_cache.get(symbol).unwrap().clone());
    }
    compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;
    for (symbol, df) in cs_map {
        data_cache.insert(symbol, df);
    }

    let btc_close = bench_df.column("close")?.f64()?;
    let btc_sma_200 = calculate_sma(&btc_close, 200);

    let mut win_counts = HashMap::<StrategyKind, usize>::new();

    for &(universe_name, universe_symbols) in UNIVERSES {
        println!(
            "\n--- Universe: {} ({}) ---",
            universe_name,
            universe_symbols.join(", ")
        );
        let quarter_windows = quarter_windows(&data_cache, universe_symbols)?;
        let resample_sets = cpcv_style_windows(&data_cache, universe_symbols)?;

        let mut universe_rows = Vec::new();
        for &strategy in StrategyKind::all() {
            let full = evaluate_strategy(&data_cache, universe_symbols, strategy, &btc_sma_200)?;

            let mut wf_passed = 0usize;
            for &(start, end) in &quarter_windows {
                let eval = evaluate_strategy_on_windows(
                    &data_cache,
                    universe_symbols,
                    strategy,
                    &[(start, end)],
                    &btc_sma_200,
                )?;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    wf_passed += 1;
                }
            }

            let mut resample_passed = 0usize;
            for windows in &resample_sets {
                let eval = evaluate_strategy_on_windows(
                    &data_cache,
                    universe_symbols,
                    strategy,
                    windows,
                    &btc_sma_200,
                )?;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    resample_passed += 1;
                }
            }

            universe_rows.push(SummaryRow {
                kind: strategy,
                full_return_pct: full.total_return_pct,
                full_trades: full.trades,
                full_win_rate: full.win_rate(),
                wf_passed,
                wf_total: quarter_windows.len(),
                resample_passed,
                resample_total: resample_sets.len(),
            });
        }

        universe_rows.sort_by(|a, b| {
            b.resample_passed
                .cmp(&a.resample_passed)
                .then_with(|| b.wf_passed.cmp(&a.wf_passed))
                .then_with(|| b.full_return_pct.partial_cmp(&a.full_return_pct).unwrap())
        });

        for row in &universe_rows {
            println!(
                "{:<28} {:>10.1} {:>7} {:>7.1}% {:>4}/{} {:>6}/{}  {}",
                row.kind.name(),
                row.full_return_pct,
                row.full_trades,
                row.full_win_rate * 100.0,
                row.wf_passed,
                row.wf_total,
                row.resample_passed,
                row.resample_total,
                row.kind.description(),
            );
        }

        if let Some(best) = universe_rows.first() {
            *win_counts.entry(best.kind).or_insert(0) += 1;
        }
    }

    println!("\n=== UNIVERSE WIN COUNTS ===");
    for &strategy in StrategyKind::all() {
        println!(
            "{:<28} {:>2}/{}",
            strategy.name(),
            win_counts.get(&strategy).copied().unwrap_or(0),
            UNIVERSES.len()
        );
    }

    println!("\nInterpretation:");
    println!("- CTREND earns attention only if it improves chronology or hostile-basket breadth without another local tuning loop.");
    println!("- A higher full-sample return is not enough; the main question is whether a broader price+volume factor survives the same hostile universes as the current yardsticks.");
    println!("- This is a factor-audit result, not a promotion decision.");

    Ok(())
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
    strategy: StrategyKind,
    btc_sma_200: &[f64],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let result = run_strategy(df, strategy, btc_sma_200)?;
        aggregate.add_assign(&result);
    }
    Ok(aggregate)
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
    strategy: StrategyKind,
    windows: &[(usize, usize)],
    btc_sma_200: &[f64],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let result = run_strategy_on_windows(df, strategy, windows, btc_sma_200)?;
        aggregate.add_assign(&result);
    }
    Ok(aggregate)
}

fn run_strategy(
    df: &DataFrame,
    strategy: StrategyKind,
    btc_sma_200: &[f64],
) -> Result<StrategyResult> {
    let signal = signal_for_strategy(df, strategy, btc_sma_200)?;
    backtest_fixed_hold_next_open_window(df, &signal, 0, df.height())
}

fn run_strategy_on_windows(
    df: &DataFrame,
    strategy: StrategyKind,
    windows: &[(usize, usize)],
    btc_sma_200: &[f64],
) -> Result<StrategyResult> {
    let signal = signal_for_strategy(df, strategy, btc_sma_200)?;
    let mut out = StrategyResult::default();
    for &(start, end) in windows {
        let result = backtest_fixed_hold_next_open_window(df, &signal, start, end)?;
        out.add_assign(&result);
    }
    Ok(out)
}

fn signal_for_strategy(
    df: &DataFrame,
    strategy: StrategyKind,
    btc_sma_200: &[f64],
) -> Result<Vec<i32>> {
    let out = match strategy {
        StrategyKind::CTRend => generate_ctrend_signals(df)?,
        StrategyKind::MacdRegime => generate_macd_regime_signals(df)?,
        StrategyKind::TurtleMACD => generate_turtle_macd_signals(df, TURTLE_PERIOD)?,
        StrategyKind::AdMomentum => generate_ad_momentum_signals(df, AD_PERIOD)?,
        StrategyKind::CrossSectionalMomentum => CrossSectionalMomentum::new()
            .predict(df)?
            .f64()?
            .into_iter()
            .map(|v| match v.unwrap_or(0.0).partial_cmp(&0.0) {
                Some(std::cmp::Ordering::Greater) => 1,
                Some(std::cmp::Ordering::Less) => -1,
                _ => 0,
            })
            .collect::<Vec<_>>(),
    };
    let _ = btc_sma_200;
    Ok(out)
}

fn generate_ctrend_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let volume = df.column("volume")?.f64()?;
    let n = df.height();

    let vol_sma_20 = calculate_sma(&volume, 20);
    let vol_sma_63 = calculate_sma(&volume, 63);
    let ret_5 = rolling_return(&close, 5);
    let ret_21 = rolling_return(&close, 21);
    let ret_63 = rolling_return(&close, 63);
    let ret_126 = rolling_return(&close, 126);
    let rv_21 = rolling_realized_vol(&close, 21);
    let rv_63 = rolling_realized_vol(&close, 63);

    let mut out = vec![0i32; n];
    for i in 126..n {
        let short_vol = rv_21[i].max(1e-6);
        let med_vol = rv_63[i].max(1e-6);

        let price_score = 0.15 * (ret_5[i] / short_vol)
            + 0.35 * (ret_21[i] / short_vol)
            + 0.30 * (ret_63[i] / med_vol)
            + 0.20 * (ret_126[i] / med_vol);

        let vol_ratio_fast = if vol_sma_20[i] > 1e-9 {
            volume.get(i).unwrap_or(0.0) / vol_sma_20[i]
        } else {
            1.0
        };
        let vol_ratio_slow = if vol_sma_63[i] > 1e-9 {
            vol_sma_20[i] / vol_sma_63[i]
        } else {
            1.0
        };

        let price_dir = if ret_21[i] > 0.0 {
            1.0
        } else if ret_21[i] < 0.0 {
            -1.0
        } else {
            0.0
        };
        let short_dir = if ret_5[i] > 0.0 {
            1.0
        } else if ret_5[i] < 0.0 {
            -1.0
        } else {
            0.0
        };

        let volume_score = 0.20 * (vol_ratio_fast.ln()).clamp(-1.5, 1.5) * short_dir
            + 0.20 * (vol_ratio_slow.ln()).clamp(-1.5, 1.5) * price_dir;

        let score = price_score + volume_score;
        if score > 0.35 {
            out[i] = 1;
        } else if score < -0.35 {
            out[i] = -1;
        }
    }

    Ok(out)
}

fn rolling_return(values: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        let now = values.get(i).unwrap_or(0.0);
        let prev = values.get(i - lookback).unwrap_or(0.0);
        if now > 0.0 && prev > 0.0 {
            out[i] = (now / prev).ln();
        }
    }
    out
}

fn rolling_realized_vol(values: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut rets = vec![0.0; values.len()];
    for i in 1..values.len() {
        let now = values.get(i).unwrap_or(0.0);
        let prev = values.get(i - 1).unwrap_or(0.0);
        if now > 0.0 && prev > 0.0 {
            rets[i] = (now / prev).ln();
        }
    }

    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        for r in &rets[i - lookback + 1..=i] {
            sum += *r;
            sum_sq += r * r;
        }
        let n = lookback as f64;
        let mean = sum / n;
        let var = (sum_sq / n) - (mean * mean);
        out[i] = var.max(0.0).sqrt();
    }
    out
}

fn generate_ad_momentum_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let close = df.column("close")?.f64()?;
    let volume = df.column("volume")?.f64()?;

    let mut ad_line = vec![0.0; df.height()];
    for i in 0..df.height() {
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let c = close.get(i).unwrap_or(0.0);
        let v = volume.get(i).unwrap_or(0.0);
        let range = h - l;
        let mf = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        let flow = mf * v;
        ad_line[i] = if i == 0 { flow } else { ad_line[i - 1] + flow };
    }

    let mut out = vec![0i32; df.height()];
    for i in period..df.height() {
        let mom = ad_line[i] - ad_line[i - period];
        if mom > 0.0 {
            out[i] = 1;
        } else if mom < 0.0 {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let macd = df.column("macd")?.f64()?;
    let macd_signal = df.column("macd_signal")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);

    let mut out = vec![0i32; df.height()];
    for i in 0..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let macd_now = macd.get(i).unwrap_or(0.0);
        let macd_sig_now = macd_signal.get(i).unwrap_or(0.0);
        let sma_now = sma_200.get(i).copied().unwrap_or(0.0);
        if sma_now <= 0.0 {
            continue;
        }
        if macd_now > macd_sig_now && price > sma_now {
            out[i] = 1;
        } else if macd_now < macd_sig_now && price < sma_now {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let macd = df.column("macd")?.f64()?;
    let macd_signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0i32; df.height()];
    for i in period..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let macd_now = macd.get(i).unwrap_or(0.0);
        let macd_sig_now = macd_signal.get(i).unwrap_or(0.0);
        let macd_dir = if macd_now > macd_sig_now {
            1
        } else if macd_now < macd_sig_now {
            -1
        } else {
            0
        };

        let highest = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let turtle = if price > highest {
            1
        } else if price < lowest {
            -1
        } else {
            0
        };
        if turtle != 0 && turtle == macd_dir {
            out[i] = turtle;
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

fn backtest_fixed_hold_next_open_window(
    df: &DataFrame,
    signals: &[i32],
    start: usize,
    end: usize,
) -> Result<StrategyResult> {
    let open = df.column("open")?.f64()?;
    let mut out = StrategyResult::default();

    let mut i = start.max(1);
    while i + HOLD_BARS < end && i + HOLD_BARS < df.height() {
        let signal = signals[i - 1];
        if signal == 0 {
            i += 1;
            continue;
        }

        let entry = open.get(i).unwrap_or(0.0);
        let exit = open.get(i + HOLD_BARS).unwrap_or(0.0);
        if entry <= 0.0 || exit <= 0.0 {
            i += 1;
            continue;
        }

        let gross = if signal > 0 {
            (exit / entry) - 1.0
        } else {
            (entry / exit) - 1.0
        };
        let net = gross - (2.0 * TAKER_FEE);
        out.total_return_pct += net * 100.0;
        out.trades += 1;
        if net > 0.0 {
            out.wins += 1;
        }

        i += HOLD_BARS;
    }

    Ok(out)
}

fn quarter_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<Vec<(usize, usize)>> {
    let len = min_symbol_len(data_cache, symbols)?;
    let usable = len.saturating_sub(HOLD_BARS + 1);
    let quarter = usable / 4;
    let mut out = Vec::new();
    for k in 0..4 {
        let start = k * quarter;
        let end = if k == 3 { usable } else { (k + 1) * quarter };
        out.push((start, end));
    }
    Ok(out)
}

fn cpcv_style_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<Vec<Vec<(usize, usize)>>> {
    let len = min_symbol_len(data_cache, symbols)?;
    let usable = len.saturating_sub(HOLD_BARS + 1);
    let block = usable / RESAMPLE_BLOCKS;
    let ranges: Vec<(usize, usize)> = (0..RESAMPLE_BLOCKS)
        .map(|i| {
            let start = i * block;
            let end = if i == RESAMPLE_BLOCKS - 1 {
                usable
            } else {
                (i + 1) * block
            };
            (start, end)
        })
        .collect();

    let mut out = Vec::new();
    for test_blocks in combinations(RESAMPLE_BLOCKS, RESAMPLE_BLOCKS - RESAMPLE_TRAIN_BLOCKS) {
        let test_set: BTreeSet<usize> = test_blocks.into_iter().collect();
        let windows = ranges
            .iter()
            .enumerate()
            .filter_map(|(idx, range)| {
                if test_set.contains(&idx) {
                    Some(*range)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        out.push(windows);
    }
    Ok(out)
}

fn min_symbol_len(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    symbols
        .iter()
        .map(|symbol| {
            data_cache
                .get(*symbol)
                .map(|df| df.height())
                .with_context(|| format!("missing df for {}", symbol))
        })
        .collect::<Result<Vec<_>>>()
        .map(|lens| lens.into_iter().min().unwrap_or(0))
}

fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn rec(start: usize, n: usize, k: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for i in start..n {
            cur.push(i);
            rec(i + 1, n, k, cur, out);
            cur.pop();
        }
    }

    let mut out = Vec::new();
    rec(0, n, k, &mut Vec::new(), &mut out);
    out
}

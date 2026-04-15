//! Slow-pressure factor benchmark under the same harsh-universe lens.
//!
//! Purpose:
//! - deliberately broaden beyond the frozen three-sleeve/state neighborhood
//! - test slower aggregated signed-pressure variants rather than re-open dead OFI proxies
//! - compare them directly against current yardsticks under the same chronology-first harness
//!
//! Important honesty note:
//! - these are still OHLCV-derived slow-pressure proxies, not true taker-flow, liquidation, or LOB data
//! - the point is to learn whether slower aggregation creates a cleaner accessible flow lane
//! - this is a broadening audit, not a promotion result

use anyhow::{Context, Result};
use krypto::{
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

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 30;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const CS_LOOKBACK: usize = 63;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    WeeklyPressureAccel,
    SlowPressure728,
    SlowPressure1442,
    SlowPressure728Small,
    SmallByDollarVol,
    AdMomentum,
    CTRend,
    MacdRegime,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::WeeklyPressureAccel,
            Self::SlowPressure728,
            Self::SlowPressure1442,
            Self::SlowPressure728Small,
            Self::SmallByDollarVol,
            Self::AdMomentum,
            Self::CTRend,
            Self::MacdRegime,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::WeeklyPressureAccel => "WeeklyPressureAccel",
            Self::SlowPressure728 => "SlowPressure(7,14,28)",
            Self::SlowPressure1442 => "SlowPressure(14,28,42)",
            Self::SlowPressure728Small => "SlowPressure(7,14,28)+Small",
            Self::SmallByDollarVol => "FactorSmallByDollarVol",
            Self::AdMomentum => "A/D Momentum",
            Self::CTRend => "CTREND(price+volume)",
            Self::MacdRegime => "MACD+Regime",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::WeeklyPressureAccel => "weekly pressure with short-horizon acceleration",
            Self::SlowPressure728 => "7/14/28d slow signed-pressure aggregation",
            Self::SlowPressure1442 => "14/28/42d slower signed-pressure aggregation",
            Self::SlowPressure728Small => "slow pressure only when small/liquidity tilt agrees",
            Self::SmallByDollarVol => "small/liquidity proxy via inverse dollar volume",
            Self::AdMomentum => "volume-pressure yardstick",
            Self::CTRend => "broad trend-family yardstick",
            Self::MacdRegime => "filtered trend yardstick",
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
    println!("=== SLOW PRESSURE FACTOR BENCHMARK ===\n");
    println!("Benchmark: {}", BENCHMARK);
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Universe stress sets: {}", UNIVERSES.len());
    println!("Honesty note: slow pressure here is OHLCV-derived signed-pressure, not true taker-flow, liquidation, or order-book data.\n");

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

    let mut win_counts = HashMap::<StrategyKind, usize>::new();

    for &(universe_name, universe_symbols) in UNIVERSES {
        println!(
            "\n--- Universe: {} ({}) ---",
            universe_name,
            universe_symbols.join(", ")
        );
        let quarter_windows = quarter_windows(&data_cache, universe_symbols)?;
        let resample_sets = cpcv_style_windows(&data_cache, universe_symbols)?;
        let signal_map = build_universe_signals(&data_cache, universe_symbols)?;

        let mut universe_rows = Vec::new();
        for &strategy in StrategyKind::all() {
            let full = evaluate_strategy(&data_cache, universe_symbols, strategy, &signal_map)?;

            let mut wf_passed = 0usize;
            for &(start, end) in &quarter_windows {
                let eval = evaluate_strategy_on_windows(
                    &data_cache,
                    universe_symbols,
                    strategy,
                    &[(start, end)],
                    &signal_map,
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
                    &signal_map,
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
                "{:<26} {:>10.1} {:>7} {:>7.1}% {:>4}/{} {:>6}/{}  {}",
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
            "{:<26} {:>2}/{}",
            strategy.name(),
            win_counts.get(&strategy).copied().unwrap_or(0),
            UNIVERSES.len()
        );
    }

    println!("\nInterpretation:");
    println!("- This is a search-broadening slow-pressure audit, not a deployment decision.");
    println!("- If slower pressure aggregation cannot survive quarter/resample stress here, the accessible flow lane is still too weak.");
    println!("- If it does survive, it earns follow-up as a separate sleeve/state object, not immediate promotion.");

    Ok(())
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
    strategy: StrategyKind,
    signal_map: &HashMap<StrategyKind, HashMap<String, Vec<i32>>>,
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    let strategy_signals = signal_map
        .get(&strategy)
        .context("missing strategy signal map")?;
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let signals = strategy_signals
            .get(symbol)
            .context("missing symbol signals")?;
        aggregate.add_assign(&backtest_fixed_hold_next_open_window(
            df,
            signals,
            0,
            df.height(),
        )?);
    }
    Ok(aggregate)
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
    strategy: StrategyKind,
    windows: &[(usize, usize)],
    signal_map: &HashMap<StrategyKind, HashMap<String, Vec<i32>>>,
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    let strategy_signals = signal_map
        .get(&strategy)
        .context("missing strategy signal map")?;
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let signals = strategy_signals
            .get(symbol)
            .context("missing symbol signals")?;
        for &(start, end) in windows {
            aggregate.add_assign(&backtest_fixed_hold_next_open_window(
                df, signals, start, end,
            )?);
        }
    }
    Ok(aggregate)
}

fn build_universe_signals(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<HashMap<StrategyKind, HashMap<String, Vec<i32>>>> {
    let mut out = HashMap::new();
    out.insert(
        StrategyKind::WeeklyPressureAccel,
        generate_weekly_pressure_accel_signals(data_cache, symbols)?,
    );
    out.insert(
        StrategyKind::SlowPressure728,
        generate_slow_pressure_signals(data_cache, symbols, (7, 14, 28), (0.45, 0.35, 0.20))?,
    );
    out.insert(
        StrategyKind::SlowPressure1442,
        generate_slow_pressure_signals(data_cache, symbols, (14, 28, 42), (0.50, 0.30, 0.20))?,
    );
    out.insert(
        StrategyKind::SmallByDollarVol,
        generate_small_by_dollar_volume_signals(data_cache, symbols)?,
    );
    out.insert(
        StrategyKind::SlowPressure728Small,
        combine_agreeing(
            out.get(&StrategyKind::SlowPressure728).unwrap(),
            out.get(&StrategyKind::SmallByDollarVol).unwrap(),
        )?,
    );

    let mut ad_map = HashMap::new();
    let mut ctrend_map = HashMap::new();
    let mut macd_map = HashMap::new();
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        ad_map.insert(
            symbol.to_string(),
            generate_ad_momentum_signals(df, AD_PERIOD)?,
        );
        ctrend_map.insert(symbol.to_string(), generate_ctrend_signals(df)?);
        macd_map.insert(symbol.to_string(), generate_macd_regime_signals(df)?);
    }
    out.insert(StrategyKind::AdMomentum, ad_map);
    out.insert(StrategyKind::CTRend, ctrend_map);
    out.insert(StrategyKind::MacdRegime, macd_map);
    Ok(out)
}

fn combine_agreeing(
    left: &HashMap<String, Vec<i32>>,
    right: &HashMap<String, Vec<i32>>,
) -> Result<HashMap<String, Vec<i32>>> {
    let mut out = HashMap::new();
    for (symbol, ls) in left {
        let rs = right.get(symbol).context("missing paired symbol signals")?;
        let mut signals = vec![0i32; ls.len()];
        for i in 0..ls.len() {
            if ls[i] != 0 && ls[i] == rs[i] {
                signals[i] = ls[i];
            }
        }
        out.insert(symbol.clone(), signals);
    }
    Ok(out)
}

fn generate_slow_pressure_signals(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
    windows: (usize, usize, usize),
    weights: (f64, f64, f64),
) -> Result<HashMap<String, Vec<i32>>> {
    let n = min_rows(data_cache, symbols)?;
    let mut raw_scores = HashMap::<String, Vec<f64>>::new();
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let high = df.column("high")?.f64()?;
        let low = df.column("low")?.f64()?;
        let close = df.column("close")?.f64()?;
        let volume = df.column("volume")?.f64()?;
        let flow = signed_dollar_flow(high, low, close, volume);
        let flow_a = rolling_sum_vec(&flow, windows.0);
        let flow_b = rolling_sum_vec(&flow, windows.1);
        let flow_c = rolling_sum_vec(&flow, windows.2);
        let dv_a = rolling_dollar_volume(close, volume, windows.0.max(21));
        let dv_b = rolling_dollar_volume(close, volume, windows.1.max(21));
        let dv_c = rolling_dollar_volume(close, volume, windows.2.max(42));
        let mut scores = vec![0.0; n];
        let start = windows.2.max(63);
        for i in start..n {
            let slow_a = flow_a[i] / dv_a[i].max(1.0);
            let slow_b = flow_b[i] / dv_b[i].max(1.0);
            let slow_c = flow_c[i] / dv_c[i].max(1.0);
            scores[i] = weights.0 * slow_a + weights.1 * slow_b + weights.2 * slow_c;
        }
        raw_scores.insert(symbol.to_string(), scores);
    }
    rank_to_signals(&raw_scores, 0.35)
}

fn generate_weekly_pressure_accel_signals(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<HashMap<String, Vec<i32>>> {
    let n = min_rows(data_cache, symbols)?;
    let mut raw_scores = HashMap::<String, Vec<f64>>::new();
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let high = df.column("high")?.f64()?;
        let low = df.column("low")?.f64()?;
        let close = df.column("close")?.f64()?;
        let volume = df.column("volume")?.f64()?;
        let flow = signed_dollar_flow(high, low, close, volume);
        let flow_5 = rolling_sum_vec(&flow, 5);
        let flow_21 = rolling_sum_vec(&flow, 21);
        let dv_21 = rolling_dollar_volume(close, volume, 21);
        let mut scores = vec![0.0; n];
        for i in 63..n {
            let fast = flow_5[i] / dv_21[i].max(1.0);
            let slow = flow_21[i] / dv_21[i].max(1.0);
            scores[i] = 0.65 * fast + 0.35 * (fast - slow);
        }
        raw_scores.insert(symbol.to_string(), scores);
    }
    rank_to_signals(&raw_scores, 0.35)
}

fn signed_dollar_flow(
    high: &Float64Chunked,
    low: &Float64Chunked,
    close: &Float64Chunked,
    volume: &Float64Chunked,
) -> Vec<f64> {
    let mut out = vec![0.0; close.len()];
    for i in 0..close.len() {
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let c = close.get(i).unwrap_or(0.0);
        let v = volume.get(i).unwrap_or(0.0);
        let range = (h - l).abs();
        let clv = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        out[i] = clv * c.max(0.0) * v.max(0.0);
    }
    out
}

fn rolling_sum_vec(values: &[f64], lookback: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i >= lookback {
            sum -= values[i - lookback];
        }
        if i + 1 >= lookback {
            out[i] = sum;
        }
    }
    out
}

fn generate_small_by_dollar_volume_signals(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<HashMap<String, Vec<i32>>> {
    let n = min_rows(data_cache, symbols)?;
    let mut raw_scores = HashMap::<String, Vec<f64>>::new();
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let close = df.column("close")?.f64()?;
        let volume = df.column("volume")?.f64()?;
        let dollar_volume = rolling_dollar_volume(close, volume, 63);
        let mut scores = vec![0.0; n];
        for i in 63..n {
            let dv = dollar_volume[i].max(1.0);
            scores[i] = -dv.ln();
        }
        raw_scores.insert(symbol.to_string(), scores);
    }
    rank_to_signals(&raw_scores, 0.35)
}

fn rank_to_signals(
    raw_scores: &HashMap<String, Vec<f64>>,
    z_threshold: f64,
) -> Result<HashMap<String, Vec<i32>>> {
    let symbols = raw_scores.keys().cloned().collect::<Vec<_>>();
    let n = raw_scores.values().map(|v| v.len()).min().unwrap_or(0);
    let mut out = HashMap::<String, Vec<i32>>::new();
    for symbol in &symbols {
        out.insert(symbol.clone(), vec![0i32; n]);
    }

    for i in 0..n {
        let values = symbols
            .iter()
            .map(|s| raw_scores.get(s).unwrap()[i])
            .collect::<Vec<_>>();
        let mean = values.iter().sum::<f64>() / values.len().max(1) as f64;
        let var =
            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len().max(1) as f64;
        let std = var.max(1e-12).sqrt();

        for (idx, symbol) in symbols.iter().enumerate() {
            let z = (values[idx] - mean) / std;
            let signal = if z > z_threshold {
                1
            } else if z < -z_threshold {
                -1
            } else {
                0
            };
            out.get_mut(symbol).unwrap()[i] = signal;
        }
    }

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
        let sma_now = sma_200[i];
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

fn rolling_dollar_volume(
    close: &Float64Chunked,
    volume: &Float64Chunked,
    lookback: usize,
) -> Vec<f64> {
    let mut dollars = vec![0.0; close.len()];
    for i in 0..close.len() {
        dollars[i] = close.get(i).unwrap_or(0.0) * volume.get(i).unwrap_or(0.0);
    }
    rolling_mean(&dollars, lookback)
}

fn rolling_amihud(close: &Float64Chunked, volume: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut daily = vec![0.0; close.len()];
    for i in 1..close.len() {
        let now = close.get(i).unwrap_or(0.0);
        let prev = close.get(i - 1).unwrap_or(0.0);
        let dollar_volume = (now * volume.get(i).unwrap_or(0.0)).max(1.0);
        if now > 0.0 && prev > 0.0 {
            daily[i] = ((now / prev).ln()).abs() / dollar_volume;
        }
    }
    rolling_mean(&daily, lookback)
}

fn rolling_mean(values: &[f64], lookback: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i >= lookback {
            sum -= values[i - lookback];
        }
        if i + 1 >= lookback {
            out[i] = sum / lookback as f64;
        }
    }
    out
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
        let window = &rets[i - lookback + 1..=i];
        let n = lookback as f64;
        let mean = window.iter().sum::<f64>() / n;
        let var = window.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
        out[i] = var.max(0.0).sqrt();
    }
    out
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
    let capped_end = end.min(df.height()).min(signals.len());
    let mut i = start.max(1);

    while i + HOLD_BARS < capped_end {
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
            exit / entry - 1.0
        } else {
            entry / exit - 1.0
        };
        let net = gross - 2.0 * TAKER_FEE;
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
    let mut out = Vec::new();
    for held_out in held_out_combinations(RESAMPLE_BLOCKS, RESAMPLE_BLOCKS - RESAMPLE_TRAIN_BLOCKS)
    {
        let held: BTreeSet<_> = held_out.into_iter().collect();
        let windows = (0..RESAMPLE_BLOCKS)
            .filter(|idx| !held.contains(idx))
            .map(|idx| {
                let start = idx * block;
                let end = if idx + 1 == RESAMPLE_BLOCKS {
                    n
                } else {
                    (idx + 1) * block
                };
                (start, end)
            })
            .collect::<Vec<_>>();
        out.push(windows);
    }
    Ok(out)
}

fn held_out_combinations(total: usize, held_out: usize) -> Vec<Vec<usize>> {
    fn rec(
        start: usize,
        total: usize,
        choose: usize,
        cur: &mut Vec<usize>,
        out: &mut Vec<Vec<usize>>,
    ) {
        if cur.len() == choose {
            out.push(cur.clone());
            return;
        }
        for i in start..total {
            cur.push(i);
            rec(i + 1, total, choose, cur, out);
            cur.pop();
        }
    }

    let mut out = Vec::new();
    rec(0, total, held_out, &mut Vec::new(), &mut out);
    out
}

fn min_rows(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    symbols
        .iter()
        .map(|s| {
            data_cache
                .get(*s)
                .context("missing symbol data")
                .map(|df| df.height())
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .min()
        .context("no data")
}

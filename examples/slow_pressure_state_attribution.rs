//! Attribute current daily families by slow-pressure market state.
//!
//! Purpose:
//! - follow up the slow-pressure benchmark without opening another window/weight tuning loop
//! - ask where existing families earn returns when cross-sectional slow pressure is bullish,
//!   bearish, mixed, or quiet
//! - learn whether pressure is better used as hostile-basket / state context than as a default sleeve
//!
//! Honesty note:
//! - slow pressure here is still an OHLCV-derived signed-pressure proxy, not true taker-flow,
//!   liquidation, or order-book state
//! - this is a state-attribution audit, not a promotion result

use anyhow::{Context, Result};
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::{BTreeMap, HashMap};

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
const CS_LOOKBACK: usize = 63;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    SlowPressure728,
    WeeklyPressureAccel,
    SmallByDollarVol,
    AdMomentum,
    CTRend,
    MacdRegime,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::SlowPressure728,
            Self::WeeklyPressureAccel,
            Self::SmallByDollarVol,
            Self::AdMomentum,
            Self::CTRend,
            Self::MacdRegime,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::SlowPressure728 => "SlowPressure(7,14,28)",
            Self::WeeklyPressureAccel => "WeeklyPressureAccel",
            Self::SmallByDollarVol => "FactorSmallByDollarVol",
            Self::AdMomentum => "A/D Momentum",
            Self::CTRend => "CTREND(price+volume)",
            Self::MacdRegime => "MACD+Regime",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum PressureStateBucket {
    Bullish,
    Bearish,
    Mixed,
    Quiet,
}

impl PressureStateBucket {
    fn all() -> &'static [PressureStateBucket] {
        &[Self::Bullish, Self::Bearish, Self::Mixed, Self::Quiet]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Bullish => "PressureBullish",
            Self::Bearish => "PressureBearish",
            Self::Mixed => "PressureMixed",
            Self::Quiet => "PressureQuiet",
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

    fn avg_return_per_trade_pct(&self) -> f64 {
        if self.trades == 0 {
            0.0
        } else {
            self.total_return_pct / self.trades as f64
        }
    }

    fn add_assign(&mut self, other: &StrategyResult) {
        self.total_return_pct += other.total_return_pct;
        self.trades += other.trades;
        self.wins += other.wins;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== SLOW PRESSURE STATE ATTRIBUTION ===\n");
    println!("Goal: attribute current families by slow-pressure market state instead of opening another pressure parameter loop");
    println!("Benchmark: {}", BENCHMARK);
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("State anchor: cross-sectional SlowPressure(7,14,28) breadth across each universe");
    println!("Honesty note: slow pressure is still OHLCV-derived signed pressure, not true taker-flow.\n");

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

    for &(universe_name, universe_symbols) in UNIVERSES {
        println!(
            "\n--- Universe: {} ({}) ---",
            universe_name,
            universe_symbols.join(", ")
        );
        let signal_map = build_universe_signals(&data_cache, universe_symbols)?;
        let pressure_state = classify_pressure_state(
            signal_map
                .get(&StrategyKind::SlowPressure728)
                .context("missing slow pressure signals")?,
            universe_symbols,
        )?;

        let mut state_counts = BTreeMap::<PressureStateBucket, usize>::new();
        for bucket in PressureStateBucket::all() {
            state_counts.insert(*bucket, 0);
        }
        for bucket in &pressure_state {
            *state_counts.get_mut(bucket).unwrap() += 1;
        }

        println!("State mix:");
        for bucket in PressureStateBucket::all() {
            let days = state_counts.get(bucket).copied().unwrap_or(0);
            let share = days as f64 / pressure_state.len().max(1) as f64 * 100.0;
            println!("  {:<16} {:>5} days ({:>4.1}%)", bucket.name(), days, share);
        }

        let mut per_strategy = Vec::<(
            StrategyKind,
            StrategyResult,
            BTreeMap<PressureStateBucket, StrategyResult>,
        )>::new();
        for &strategy in StrategyKind::all() {
            let mut full = StrategyResult::default();
            let mut by_state = BTreeMap::<PressureStateBucket, StrategyResult>::new();
            for bucket in PressureStateBucket::all() {
                by_state.insert(*bucket, StrategyResult::default());
            }

            let strategy_signals = signal_map
                .get(&strategy)
                .context("missing strategy signals")?;
            for &symbol in universe_symbols {
                let df = data_cache.get(symbol).context("missing symbol data")?;
                let signals = strategy_signals
                    .get(symbol)
                    .context("missing symbol signals")?;
                let (symbol_full, symbol_state) =
                    backtest_with_pressure_attribution(df, signals, &pressure_state)?;
                full.add_assign(&symbol_full);
                for bucket in PressureStateBucket::all() {
                    by_state
                        .get_mut(bucket)
                        .unwrap()
                        .add_assign(symbol_state.get(bucket).unwrap());
                }
            }

            per_strategy.push((strategy, full, by_state));
        }

        per_strategy.sort_by(|a, b| {
            b.1.total_return_pct
                .partial_cmp(&a.1.total_return_pct)
                .unwrap()
                .then_with(|| b.1.trades.cmp(&a.1.trades))
        });

        println!(
            "\n{:<24} {:>10} {:>7} {:>7}",
            "Strategy", "Return%", "Trades", "Win%"
        );
        for (strategy, full, _) in &per_strategy {
            println!(
                "{:<24} {:>10.1} {:>7} {:>6.1}",
                strategy.name(),
                full.total_return_pct,
                full.trades,
                full.win_rate() * 100.0,
            );
        }

        println!("\nBy slow-pressure state:");
        for (strategy, _, by_state) in &per_strategy {
            println!("\n{}", strategy.name());
            for bucket in PressureStateBucket::all() {
                let stats = by_state.get(bucket).unwrap();
                println!(
                    "  {:<16} {:>10.1} {:>7} {:>6.1}%  avg/trade {:>6.2}",
                    bucket.name(),
                    stats.total_return_pct,
                    stats.trades,
                    stats.win_rate() * 100.0,
                    stats.avg_return_per_trade_pct(),
                );
            }
        }
    }

    println!("\nInterpretation:");
    println!("- If slow-pressure families mainly earn in mixed/bearish states while trend families dominate bullish states, pressure is better treated as hostile-basket context than as a default promoted sleeve.");
    println!("- If state differences are tiny, then slow pressure still has not earned contextual authority.");
    println!("- This is a state-attribution audit, not a deployment decision.");
    Ok(())
}

fn build_universe_signals(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<HashMap<StrategyKind, HashMap<String, Vec<i32>>>> {
    let mut out = HashMap::new();
    out.insert(
        StrategyKind::SlowPressure728,
        generate_slow_pressure_signals(data_cache, symbols, (7, 14, 28), (0.45, 0.35, 0.20))?,
    );
    out.insert(
        StrategyKind::WeeklyPressureAccel,
        generate_weekly_pressure_accel_signals(data_cache, symbols)?,
    );
    out.insert(
        StrategyKind::SmallByDollarVol,
        generate_small_by_dollar_volume_signals(data_cache, symbols)?,
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

fn classify_pressure_state(
    slow_pressure_signals: &HashMap<String, Vec<i32>>,
    symbols: &[&str],
) -> Result<Vec<PressureStateBucket>> {
    let len = slow_pressure_signals
        .values()
        .map(|v| v.len())
        .min()
        .context("no signals")?;
    let mut out = vec![PressureStateBucket::Quiet; len];
    for i in 0..len {
        let mut longs = 0usize;
        let mut shorts = 0usize;
        for &symbol in symbols {
            match slow_pressure_signals
                .get(symbol)
                .context("missing slow-pressure symbol")?[i]
            {
                1 => longs += 1,
                -1 => shorts += 1,
                _ => {}
            }
        }
        let active_share = (longs + shorts) as f64 / symbols.len().max(1) as f64;
        let imbalance = (longs as f64 - shorts as f64) / symbols.len().max(1) as f64;
        out[i] = if active_share < 0.35 {
            PressureStateBucket::Quiet
        } else if imbalance >= 0.40 {
            PressureStateBucket::Bullish
        } else if imbalance <= -0.40 {
            PressureStateBucket::Bearish
        } else {
            PressureStateBucket::Mixed
        };
    }
    Ok(out)
}

fn backtest_with_pressure_attribution(
    df: &DataFrame,
    signals: &[i32],
    pressure_state: &[PressureStateBucket],
) -> Result<(
    StrategyResult,
    BTreeMap<PressureStateBucket, StrategyResult>,
)> {
    let open = df.column("open")?.f64()?;
    let n = df.height().min(signals.len()).min(pressure_state.len());
    let mut full = StrategyResult::default();
    let mut by_state = BTreeMap::<PressureStateBucket, StrategyResult>::new();
    for bucket in PressureStateBucket::all() {
        by_state.insert(*bucket, StrategyResult::default());
    }

    for i in 0..n.saturating_sub(HOLD_BARS + 1) {
        let sig = signals[i];
        if sig == 0 {
            continue;
        }
        let entry = open.get(i + 1).unwrap_or(0.0);
        let exit = open.get(i + 1 + HOLD_BARS).unwrap_or(0.0);
        if entry <= 0.0 || exit <= 0.0 {
            continue;
        }
        let gross = match sig {
            1 => exit / entry - 1.0,
            -1 => entry / exit - 1.0,
            _ => 0.0,
        };
        let net = gross - (2.0 * TAKER_FEE);
        full.total_return_pct += net * 100.0;
        full.trades += 1;
        if net > 0.0 {
            full.wins += 1;
        }

        let bucket = pressure_state[i];
        let state_stats = by_state.get_mut(&bucket).unwrap();
        state_stats.total_return_pct += net * 100.0;
        state_stats.trades += 1;
        if net > 0.0 {
            state_stats.wins += 1;
        }
    }

    Ok((full, by_state))
}

fn min_rows(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    let mut min_rows: Option<usize> = None;
    for &symbol in symbols {
        let rows = data_cache
            .get(symbol)
            .map(|df| df.height())
            .with_context(|| format!("missing symbol {symbol}"))?;
        min_rows = Some(match min_rows {
            Some(current) => current.min(rows),
            None => rows,
        });
    }
    min_rows.context("empty symbol set")
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
            scores[i] = -dollar_volume[i].max(1.0).ln();
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

fn calculate_sma(series: &Float64Chunked, period: usize) -> Vec<f64> {
    let mut out = vec![0.0; series.len()];
    let mut sum = 0.0;
    for i in 0..series.len() {
        sum += series.get(i).unwrap_or(0.0);
        if i >= period {
            sum -= series.get(i - period).unwrap_or(0.0);
        }
        if i + 1 >= period {
            out[i] = sum / period as f64;
        }
    }
    out
}

fn rolling_return(close: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut out = vec![0.0; close.len()];
    for i in lookback..close.len() {
        let now = close.get(i).unwrap_or(0.0);
        let prev = close.get(i - lookback).unwrap_or(0.0);
        if prev > 0.0 {
            out[i] = now / prev - 1.0;
        }
    }
    out
}

fn rolling_realized_vol(close: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut ret = vec![0.0; close.len()];
    for i in 1..close.len() {
        let now = close.get(i).unwrap_or(0.0);
        let prev = close.get(i - 1).unwrap_or(0.0);
        if now > 0.0 && prev > 0.0 {
            ret[i] = (now / prev).ln();
        }
    }
    let mut out = vec![0.0; close.len()];
    for i in lookback..close.len() {
        let window = &ret[i + 1 - lookback..=i];
        let mean = window.iter().sum::<f64>() / lookback as f64;
        let var = window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / lookback as f64;
        out[i] = var.sqrt() * (252.0f64).sqrt();
    }
    out
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

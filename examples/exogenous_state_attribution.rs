//! Macro-state attribution across existing daily strategy families.
//!
//! Goal:
//! - stop optimizing new switch rules and instead measure where each family earns its returns
//! - use lagged macro state known strictly before the crypto session date
//! - keep the same fair daily next-open / fixed-hold assumptions used elsewhere

use anyhow::{Context, Result};
use chrono::{NaiveDate, TimeZone, Utc};
use krypto::{
    algo::{strategies::CrossSectionalMomentum, SignalGenerator},
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
];

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const CS_LOOKBACK: usize = 63;
const TURTLE_PERIOD: usize = 20;
const SP500_CSV: &str = "data/exogenous/sp500.csv";
const VIX_CSV: &str = "data/exogenous/vixcls.csv";
const DXY_CSV: &str = "data/exogenous/dxy.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    CrossSectionalMomentum,
    MacdRegime,
    TurtleRegimeMacd,
    EnsembleMajority3,
    SwitchMacroRiskOnMajorityElseMacd,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::CrossSectionalMomentum,
            Self::MacdRegime,
            Self::TurtleRegimeMacd,
            Self::EnsembleMajority3,
            Self::SwitchMacroRiskOnMajorityElseMacd,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
            Self::EnsembleMajority3 => "Ensemble(Majority 2/3)",
            Self::SwitchMacroRiskOnMajorityElseMacd => "Switch(MacroRiskOn->Majority, else MACD)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum MacroStateBucket {
    RiskOn,
    Stress,
    Neutral,
}

impl MacroStateBucket {
    fn all() -> &'static [MacroStateBucket] {
        &[Self::RiskOn, Self::Stress, Self::Neutral]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::RiskOn => "MacroRiskOn",
            Self::Stress => "MacroStress",
            Self::Neutral => "MacroNeutral",
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
struct MarketState {
    macro_risk_on: Vec<bool>,
    macro_stress: Vec<bool>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== EXOGENOUS STATE ATTRIBUTION ===\n");
    println!("Goal: attribute existing family returns by lagged macro state instead of testing another switch-rule variant");
    println!("Benchmark state anchor: {}", BENCHMARK);
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side\n", TAKER_FEE * 100.0);

    let macro_cache = fetch_macro_series()?;

    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;
    let market_state = build_macro_state(&bench_df, &macro_cache)?;

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
        let mut per_strategy: Vec<(
            StrategyKind,
            StrategyResult,
            BTreeMap<MacroStateBucket, StrategyResult>,
        )> = Vec::new();

        for &strategy in StrategyKind::all() {
            let mut full = StrategyResult::default();
            let mut by_state = BTreeMap::<MacroStateBucket, StrategyResult>::new();
            for bucket in MacroStateBucket::all() {
                by_state.insert(*bucket, StrategyResult::default());
            }

            for &symbol in universe_symbols {
                let df = data_cache.get(symbol).context("missing symbol data")?;
                let signal = signal_for_strategy(df, strategy, &market_state)?;
                let (symbol_full, symbol_states) =
                    backtest_with_macro_attribution(df, &signal, &market_state)?;
                full.add_assign(&symbol_full);
                for bucket in MacroStateBucket::all() {
                    if let Some(dest) = by_state.get_mut(bucket) {
                        dest.add_assign(symbol_states.get(bucket).unwrap());
                    }
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
            "{:<38} {:>10} {:>7} {:>7}",
            "Strategy", "Return%", "Trades", "Win%"
        );
        for (strategy, full, _) in &per_strategy {
            println!(
                "{:<38} {:>10.1} {:>7} {:>6.1}",
                strategy.name(),
                full.total_return_pct,
                full.trades,
                full.win_rate() * 100.0,
            );
        }

        println!("\nBy lagged macro state:");
        for (strategy, _, by_state) in &per_strategy {
            println!("\n{}", strategy.name());
            for bucket in MacroStateBucket::all() {
                let stats = by_state.get(bucket).unwrap();
                let share = if stats.trades == 0 {
                    0.0
                } else {
                    stats.total_return_pct / stats.trades as f64
                };
                println!(
                    "  {:<12} {:>10.1} {:>7} {:>6.1}%  avg/trade {:>6.2}",
                    bucket.name(),
                    stats.total_return_pct,
                    stats.trades,
                    stats.win_rate() * 100.0,
                    share,
                );
            }
        }
    }

    println!("\nInterpretation:");
    println!("- If a family only works in one macro bucket, exogenous context is probably more useful for attribution / sizing than for naive binary switching.");
    println!("- If the same family is healthy across risk-on, stress, and neutral states, macro-state overlays are less likely to add much.");
    println!("- This is still a research audit, not a promotion decision.");

    Ok(())
}

fn signal_for_strategy(
    df: &DataFrame,
    strategy: StrategyKind,
    market_state: &MarketState,
) -> Result<Vec<i32>> {
    let macd_regime = generate_macd_regime_signals(df)?;
    let turtle_regime_macd = generate_turtle_regime_macd_signals(df, TURTLE_PERIOD)?;
    let cross_sectional = CrossSectionalMomentum::new()
        .predict(df)?
        .f64()?
        .into_iter()
        .map(|v| match v.unwrap_or(0.0).partial_cmp(&0.0) {
            Some(std::cmp::Ordering::Greater) => 1,
            Some(std::cmp::Ordering::Less) => -1,
            _ => 0,
        })
        .collect::<Vec<_>>();
    let majority = consensus_signal(&[&macd_regime, &turtle_regime_macd, &cross_sectional], 2);

    Ok(match strategy {
        StrategyKind::CrossSectionalMomentum => cross_sectional,
        StrategyKind::MacdRegime => macd_regime,
        StrategyKind::TurtleRegimeMacd => turtle_regime_macd,
        StrategyKind::EnsembleMajority3 => majority,
        StrategyKind::SwitchMacroRiskOnMajorityElseMacd => {
            switch_signal(&market_state.macro_risk_on, &majority, &macd_regime)
        }
    })
}

fn backtest_with_macro_attribution(
    df: &DataFrame,
    signals: &[i32],
    market_state: &MarketState,
) -> Result<(StrategyResult, BTreeMap<MacroStateBucket, StrategyResult>)> {
    let open = df.column("open")?.f64()?;
    let mut full = StrategyResult::default();
    let mut by_state = BTreeMap::<MacroStateBucket, StrategyResult>::new();
    for bucket in MacroStateBucket::all() {
        by_state.insert(*bucket, StrategyResult::default());
    }

    let mut i = 1usize;
    while i + HOLD_BARS < df.height() {
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

        full.total_return_pct += net * 100.0;
        full.trades += 1;
        if net > 0.0 {
            full.wins += 1;
        }

        let bucket = macro_bucket(market_state, i - 1);
        let state_stats = by_state.get_mut(&bucket).unwrap();
        state_stats.total_return_pct += net * 100.0;
        state_stats.trades += 1;
        if net > 0.0 {
            state_stats.wins += 1;
        }

        i += HOLD_BARS;
    }

    Ok((full, by_state))
}

fn macro_bucket(market_state: &MarketState, idx: usize) -> MacroStateBucket {
    if market_state
        .macro_risk_on
        .get(idx)
        .copied()
        .unwrap_or(false)
    {
        MacroStateBucket::RiskOn
    } else if market_state.macro_stress.get(idx).copied().unwrap_or(false) {
        MacroStateBucket::Stress
    } else {
        MacroStateBucket::Neutral
    }
}

fn switch_signal(state: &[bool], when_true: &[i32], when_false: &[i32]) -> Vec<i32> {
    let len = state.len().min(when_true.len()).min(when_false.len());
    let mut out = vec![0i32; len];
    for i in 0..len {
        out[i] = if state[i] {
            when_true[i]
        } else {
            when_false[i]
        };
    }
    out
}

fn consensus_signal(signals: &[&Vec<i32>], min_agree: usize) -> Vec<i32> {
    let len = signals.first().map(|s| s.len()).unwrap_or(0);
    let mut out = vec![0i32; len];
    for i in 0..len {
        let long_votes = signals.iter().filter(|sig| sig[i] > 0).count();
        let short_votes = signals.iter().filter(|sig| sig[i] < 0).count();
        if long_votes >= min_agree {
            out[i] = 1;
        } else if short_votes >= min_agree {
            out[i] = -1;
        }
    }
    out
}

fn fetch_macro_series() -> Result<HashMap<&'static str, BTreeMap<NaiveDate, f64>>> {
    let specs = [
        ("SP500", SP500_CSV),
        ("VIXCLS", VIX_CSV),
        ("DTWEXBGS", DXY_CSV),
    ];
    let mut out = HashMap::new();
    for (name, path) in specs {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {} from {}", name, path))?;
        let series = parse_fred_csv(&text, name)?;
        println!(
            "Loaded macro series {} from {} ({} rows)",
            name,
            path,
            series.len()
        );
        out.insert(name, series);
    }
    Ok(out)
}

fn parse_fred_csv(csv: &str, value_col: &str) -> Result<BTreeMap<NaiveDate, f64>> {
    let mut map = BTreeMap::new();
    for line in csv.lines().skip(1) {
        let mut parts = line.split(',');
        let date_str = parts.next().unwrap_or("").trim();
        let value_str = parts.next().unwrap_or("").trim();
        if date_str.is_empty() || value_str.is_empty() || value_str == "." {
            continue;
        }
        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .with_context(|| format!("bad {} date {}", value_col, date_str))?;
        let value: f64 = value_str
            .parse()
            .with_context(|| format!("bad {} value {}", value_col, value_str))?;
        map.insert(date, value);
    }
    Ok(map)
}

fn build_macro_state(
    bench_df: &DataFrame,
    macro_cache: &HashMap<&'static str, BTreeMap<NaiveDate, f64>>,
) -> Result<MarketState> {
    let dates = crypto_dates(bench_df)?;
    let spx_prev = align_previous_macro(&dates, macro_cache.get("SP500").context("missing SP500")?);
    let vix_prev =
        align_previous_macro(&dates, macro_cache.get("VIXCLS").context("missing VIXCLS")?);
    let dxy_prev = align_previous_macro(
        &dates,
        macro_cache.get("DTWEXBGS").context("missing DTWEXBGS")?,
    );
    let spx_sma50 = rolling_sma_option(&spx_prev, 50);
    let vix_sma20 = rolling_sma_option(&vix_prev, 20);
    let dxy_sma50 = rolling_sma_option(&dxy_prev, 50);

    let mut macro_risk_on = vec![false; bench_df.height()];
    let mut macro_stress = vec![false; bench_df.height()];
    for i in 0..bench_df.height() {
        macro_risk_on[i] = matches!((spx_prev[i], spx_sma50[i], vix_prev[i], vix_sma20[i], dxy_prev[i], dxy_sma50[i]),
            (Some(spx), Some(spx_sma), Some(vix), Some(vix_sma), Some(dxy), Some(dxy_sma))
                if spx > spx_sma && vix < vix_sma && dxy < dxy_sma);
        macro_stress[i] = matches!((vix_prev[i], vix_sma20[i]), (Some(vix), Some(vix_sma)) if vix > vix_sma)
            || matches!((dxy_prev[i], dxy_sma50[i]), (Some(dxy), Some(dxy_sma)) if dxy > dxy_sma);
    }

    Ok(MarketState {
        macro_risk_on,
        macro_stress,
    })
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

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_regime_signals(df, period)?;
    let macd = df.column("macd")?.f64()?;
    let macd_signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0i32; df.height()];
    for i in 0..df.height() {
        let t = turtle[i];
        let m = if macd.get(i).unwrap_or(0.0) > macd_signal.get(i).unwrap_or(0.0) {
            1
        } else if macd.get(i).unwrap_or(0.0) < macd_signal.get(i).unwrap_or(0.0) {
            -1
        } else {
            0
        };
        if t == m {
            out[i] = t;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; df.height()];
    for i in period..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let sma_now = sma_200.get(i).copied().unwrap_or(0.0);
        if sma_now <= 0.0 {
            continue;
        }

        let highest = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);

        if price > highest && price > sma_now {
            out[i] = 1;
        } else if price < lowest && price < sma_now {
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

fn crypto_dates(df: &DataFrame) -> Result<Vec<NaiveDate>> {
    let time = df.column("time")?.datetime()?;
    let mut out = Vec::with_capacity(time.len());
    for i in 0..time.len() {
        let ts = time.get(i).context("missing crypto timestamp")?;
        let dt = Utc
            .timestamp_millis_opt(ts)
            .single()
            .context("bad crypto timestamp")?;
        out.push(dt.date_naive());
    }
    Ok(out)
}

fn align_previous_macro(
    dates: &[NaiveDate],
    series: &BTreeMap<NaiveDate, f64>,
) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(dates.len());
    for date in dates {
        out.push(series.range(..*date).next_back().map(|(_, v)| *v));
    }
    out
}

fn rolling_sma_option(series: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; series.len()];
    for i in 0..series.len() {
        if i + 1 < period {
            continue;
        }
        let slice = &series[i + 1 - period..=i];
        if slice.iter().all(|v| v.is_some()) {
            let sum: f64 = slice.iter().map(|v| v.unwrap()).sum();
            out[i] = Some(sum / period as f64);
        }
    }
    out
}

//! Exogenous meta-allocation benchmark across existing daily strategy families.
//!
//! Goal:
//! - test whether lagged macro state can help choose between already-studied families
//! - go beyond blunt macro long-gates without fitting weights or optimizing thresholds
//! - keep the same fair daily assumptions used elsewhere so results are comparable
//!
//! Meta principle:
//! - no optimizer or learned weights in the loop
//! - use only lagged macro state known strictly before the crypto session date
//! - switch between already-studied families rather than inventing another local variant

use anyhow::{Context, Result};
use chrono::{NaiveDate, TimeZone, Utc};
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
const SP500_CSV: &str = "data/exogenous/sp500.csv";
const VIX_CSV: &str = "data/exogenous/vixcls.csv";
const DXY_CSV: &str = "data/exogenous/dxy.csv";

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    CrossSectionalMomentum,
    MacdRegime,
    TurtleRegimeMacd,
    EnsembleMajority3,
    SwitchMacroRiskOnMajorityElseMacd,
    SwitchMacroRiskOnTurtleElseMacd,
    SwitchMacroStressMacdElseCsm,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::CrossSectionalMomentum,
            Self::MacdRegime,
            Self::TurtleRegimeMacd,
            Self::EnsembleMajority3,
            Self::SwitchMacroRiskOnMajorityElseMacd,
            Self::SwitchMacroRiskOnTurtleElseMacd,
            Self::SwitchMacroStressMacdElseCsm,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
            Self::EnsembleMajority3 => "Ensemble(Majority 2/3)",
            Self::SwitchMacroRiskOnMajorityElseMacd => "Switch(Bull->Majority, else MACD)",
            Self::SwitchMacroRiskOnTurtleElseMacd => "Switch(Bull->TurtleCombo, else MACD)",
            Self::SwitchMacroStressMacdElseCsm => "Switch(HighVol->MACD, else CSM)",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::CrossSectionalMomentum => "breadth baseline",
            Self::MacdRegime => "trend filtered baseline",
            Self::TurtleRegimeMacd => "trend combo baseline",
            Self::EnsembleMajority3 => "2-of-3 vote across MACD+Regime, Turtle+Regime+MACD, and CSM",
            Self::SwitchMacroRiskOnMajorityElseMacd => "if lagged SP500 > SMA50 and lagged VIX < SMA20 and lagged DXY < SMA50, use majority ensemble; otherwise filtered MACD",
            Self::SwitchMacroRiskOnTurtleElseMacd => "if lagged macro risk-on is true, use Turtle+Regime+MACD; otherwise filtered MACD",
            Self::SwitchMacroStressMacdElseCsm => "if lagged VIX > SMA20 or lagged DXY > SMA50, use filtered MACD; otherwise use cross-sectional momentum",
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

struct MarketState {
    macro_risk_on: Vec<bool>,
    macro_stress: Vec<bool>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== EXOGENOUS META-ALLOCATION BREADTH BENCHMARK ===\n");
    println!("Crypto alignment anchor: {}", BENCHMARK);
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Meta-allocation rules are pre-declared, not fit by optimizer.\n");

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
            let full = evaluate_strategy(&data_cache, universe_symbols, strategy, &market_state)?;

            let mut wf_passed = 0usize;
            for &(start, end) in &quarter_windows {
                let eval = evaluate_strategy_on_windows(
                    &data_cache,
                    universe_symbols,
                    strategy,
                    &[(start, end)],
                    &market_state,
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
                    &market_state,
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
                "{:<36} {:>10.1} {:>7} {:>7.1}% {:>4}/{} {:>6}/{}  {}",
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
            "{:<36} {:>2}/{}",
            strategy.name(),
            win_counts.get(&strategy).copied().unwrap_or(0),
            UNIVERSES.len()
        );
    }

    println!("\nInterpretation:");
    println!("- Exogenous meta-allocation earns attention only if lagged macro state improves breadth honestly without fitted weights.");
    println!("- If macro-state switching still fails to beat the ingredients, that is a useful negative result: richer context is not automatically better context.");
    println!("- This is a breadth experiment, not a promotion decision.");

    Ok(())
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
    strategy: StrategyKind,
    market_state: &MarketState,
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let result = run_strategy(df, strategy, market_state)?;
        aggregate.add_assign(&result);
    }
    Ok(aggregate)
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
    strategy: StrategyKind,
    windows: &[(usize, usize)],
    market_state: &MarketState,
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let result = run_strategy_on_windows(df, strategy, windows, market_state)?;
        aggregate.add_assign(&result);
    }
    Ok(aggregate)
}

fn run_strategy(
    df: &DataFrame,
    strategy: StrategyKind,
    market_state: &MarketState,
) -> Result<StrategyResult> {
    let signal = signal_for_strategy(df, strategy, market_state)?;
    backtest_fixed_hold_next_open_window(df, &signal, 0, df.height())
}

fn run_strategy_on_windows(
    df: &DataFrame,
    strategy: StrategyKind,
    windows: &[(usize, usize)],
    market_state: &MarketState,
) -> Result<StrategyResult> {
    let signal = signal_for_strategy(df, strategy, market_state)?;
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

    let out = match strategy {
        StrategyKind::CrossSectionalMomentum => cross_sectional,
        StrategyKind::MacdRegime => macd_regime,
        StrategyKind::TurtleRegimeMacd => turtle_regime_macd,
        StrategyKind::EnsembleMajority3 => majority,
        StrategyKind::SwitchMacroRiskOnMajorityElseMacd => {
            switch_signal(&market_state.macro_risk_on, &majority, &macd_regime)
        }
        StrategyKind::SwitchMacroRiskOnTurtleElseMacd => switch_signal(
            &market_state.macro_risk_on,
            &turtle_regime_macd,
            &macd_regime,
        ),
        StrategyKind::SwitchMacroStressMacdElseCsm => {
            switch_signal(&market_state.macro_stress, &macd_regime, &cross_sectional)
        }
    };

    Ok(out)
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

fn fetch_macro_series() -> Result<HashMap<&'static str, std::collections::BTreeMap<NaiveDate, f64>>>
{
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

fn parse_fred_csv(
    csv: &str,
    value_col: &str,
) -> Result<std::collections::BTreeMap<NaiveDate, f64>> {
    let mut map = std::collections::BTreeMap::new();
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
    macro_cache: &HashMap<&'static str, std::collections::BTreeMap<NaiveDate, f64>>,
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

fn percent_change(values: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        let now = values.get(i).unwrap_or(0.0);
        let past = values.get(i - lookback).unwrap_or(0.0);
        if now > 0.0 && past > 0.0 {
            out[i] = now / past - 1.0;
        }
    }
    out
}

fn realized_volatility(values: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut returns = vec![0.0; values.len()];
    for i in 1..values.len() {
        let prev = values.get(i - 1).unwrap_or(0.0);
        let now = values.get(i).unwrap_or(0.0);
        if prev > 0.0 && now > 0.0 {
            returns[i] = (now / prev).ln();
        }
    }

    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        let slice = &returns[i - lookback + 1..=i];
        let mean = slice.iter().sum::<f64>() / slice.len() as f64;
        let var = slice.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / slice.len() as f64;
        out[i] = var.sqrt();
    }
    out
}

fn rolling_median(values: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in period - 1..values.len() {
        let mut window = values[i + 1 - period..=i].to_vec();
        window.sort_by(|a, b| a.partial_cmp(b).unwrap());
        out[i] = window[window.len() / 2];
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
    series: &std::collections::BTreeMap<NaiveDate, f64>,
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
        let window = &series[i + 1 - period..=i];
        if window.iter().all(|v| v.is_some()) {
            let sum: f64 = window.iter().filter_map(|v| *v).sum();
            out[i] = Some(sum / period as f64);
        }
    }
    out
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

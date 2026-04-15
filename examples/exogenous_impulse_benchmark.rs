//! Exogenous cross-market impulse benchmark.
//!
//! Goal:
//! - test a genuinely different exogenous family than the earlier macro gates / switchers
//! - ask whether lagged SPX / VIX / DXY shocks have standalone directional value for crypto
//! - keep the same fair daily harness used elsewhere: signal at close, next-open entry,
//!   fixed-hold exit, realistic taker fees, chronology stress
//!
//! Conservative alignment rule:
//! - crypto bar on day D may only use the latest macro close strictly before D
//! - this intentionally lags macro information by at least one calendar day

use anyhow::{Context, Result};
use chrono::{NaiveDate, TimeZone, Utc};
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::collections::{BTreeMap, HashMap};

const LOAD_SYMBOLS: &[&str] = &[
    "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
    "BCHUSDT",
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
const MIN_TRADES_PER_WINDOW: usize = 30;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const MACRO_RET_LOOKBACK: usize = 5;
const MACRO_Z_WINDOW: usize = 63;
const IMPULSE_Z_THRESHOLD: f64 = 1.0;

const SP500_CSV: &str = "data/exogenous/sp500.csv";
const VIX_CSV: &str = "data/exogenous/vixcls.csv";
const DXY_CSV: &str = "data/exogenous/dxy.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    SpxFollow,
    VixInverse,
    DxyInverse,
    ComboMajority,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::SpxFollow,
            Self::VixInverse,
            Self::DxyInverse,
            Self::ComboMajority,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::SpxFollow => "SPX impulse follow",
            Self::VixInverse => "VIX impulse inverse",
            Self::DxyInverse => "DXY impulse inverse",
            Self::ComboMajority => "Macro impulse combo",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::SpxFollow => "long after positive lagged SPX shock, short after negative shock",
            Self::VixInverse => "short after positive lagged VIX shock, long after negative shock",
            Self::DxyInverse => "short after positive lagged DXY shock, long after negative shock",
            Self::ComboMajority => "majority vote across SPX-follow, VIX-inverse, DXY-inverse",
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

#[derive(Clone, Debug)]
struct MacroContext {
    spx_signal: Vec<i8>,
    vix_signal: Vec<i8>,
    dxy_signal: Vec<i8>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== EXOGENOUS IMPULSE BENCHMARK ===\n");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Macro impulse: {}-day macro return, {}-day rolling z-score, threshold {:.1}",
        MACRO_RET_LOOKBACK, MACRO_Z_WINDOW, IMPULSE_Z_THRESHOLD
    );
    println!("Macro alignment: use latest macro close strictly before each crypto bar date\n");

    let macro_cache = fetch_macro_series().await?;
    let loader = DataLoader::new(None, None);
    let mut data_cache = HashMap::<String, DataFrame>::new();
    let mut macro_contexts = HashMap::<String, MacroContext>::new();

    for &symbol in LOAD_SYMBOLS {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", enriched.height());
        let context = build_macro_context(&enriched, &macro_cache)?;
        macro_contexts.insert(symbol.to_string(), context);
        data_cache.insert(symbol.to_string(), enriched);
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
        let mut rows = Vec::new();

        for &strategy in StrategyKind::all() {
            let full = evaluate_strategy(&data_cache, &macro_contexts, universe_symbols, strategy)?;

            let mut wf_passed = 0usize;
            for &(start, end) in &quarter_windows {
                let eval = evaluate_strategy_on_windows(
                    &data_cache,
                    &macro_contexts,
                    universe_symbols,
                    strategy,
                    &[(start, end)],
                )?;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    wf_passed += 1;
                }
            }

            let mut resample_passed = 0usize;
            for windows in &resample_sets {
                let eval = evaluate_strategy_on_windows(
                    &data_cache,
                    &macro_contexts,
                    universe_symbols,
                    strategy,
                    windows,
                )?;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    resample_passed += 1;
                }
            }

            rows.push(SummaryRow {
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

        rows.sort_by(|a, b| {
            b.resample_passed
                .cmp(&a.resample_passed)
                .then_with(|| b.wf_passed.cmp(&a.wf_passed))
                .then_with(|| b.full_return_pct.partial_cmp(&a.full_return_pct).unwrap())
        });

        for row in &rows {
            println!(
                "{:<24} {:>10.1} {:>7} {:>7.1}% {:>4}/{} {:>6}/{}  {}",
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

        if let Some(best) = rows.first() {
            *win_counts.entry(best.kind).or_insert(0) += 1;
        }
    }

    println!("\n=== UNIVERSE WIN COUNTS ===");
    for &strategy in StrategyKind::all() {
        println!(
            "{:<24} {:>2}/{}",
            strategy.name(),
            win_counts.get(&strategy).copied().unwrap_or(0),
            UNIVERSES.len()
        );
    }

    println!("\nInterpretation:");
    println!("- This is a standalone macro-impulse family test, not another filter on the existing trend yardsticks.");
    println!("- If lagged exogenous shocks cannot survive chronology here, that argues the macro bucket is more useful for attribution/risk overlays than direct signal generation under this daily fixed-hold lens.");
    println!("- Any positive result here would earn follow-up, not promotion.");

    Ok(())
}

async fn fetch_macro_series() -> Result<HashMap<&'static str, BTreeMap<NaiveDate, f64>>> {
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

fn build_macro_context(
    df: &DataFrame,
    macro_cache: &HashMap<&'static str, BTreeMap<NaiveDate, f64>>,
) -> Result<MacroContext> {
    let dates = crypto_dates(df)?;
    let spx_prev = align_previous_macro(&dates, macro_cache.get("SP500").unwrap());
    let vix_prev = align_previous_macro(&dates, macro_cache.get("VIXCLS").unwrap());
    let dxy_prev = align_previous_macro(&dates, macro_cache.get("DTWEXBGS").unwrap());

    Ok(MacroContext {
        spx_signal: impulse_signal(&spx_prev, 1.0),
        vix_signal: impulse_signal(&vix_prev, -1.0),
        dxy_signal: impulse_signal(&dxy_prev, -1.0),
    })
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
        let prev = series.range(..*date).next_back().map(|(_, v)| *v);
        out.push(prev);
    }
    out
}

fn impulse_signal(series: &[Option<f64>], direction_sign: f64) -> Vec<i8> {
    let mut raw_returns = vec![None; series.len()];
    for i in MACRO_RET_LOOKBACK..series.len() {
        if let (Some(now), Some(prev)) = (series[i], series[i - MACRO_RET_LOOKBACK]) {
            if prev != 0.0 {
                raw_returns[i] = Some(now / prev - 1.0);
            }
        }
    }

    let zscores = rolling_zscore(&raw_returns, MACRO_Z_WINDOW);
    zscores
        .into_iter()
        .map(|z| match z {
            Some(v) if v >= IMPULSE_Z_THRESHOLD => direction_sign.signum() as i8,
            Some(v) if v <= -IMPULSE_Z_THRESHOLD => -(direction_sign.signum() as i8),
            _ => 0,
        })
        .collect()
}

fn rolling_zscore(values: &[Option<f64>], window: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    for i in 0..values.len() {
        if i + 1 < window {
            continue;
        }
        let slice = &values[i + 1 - window..=i];
        let clean: Vec<f64> = slice.iter().filter_map(|v| *v).collect();
        if clean.len() < window / 2 {
            continue;
        }
        let mean = clean.iter().sum::<f64>() / clean.len() as f64;
        let var = clean.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / clean.len() as f64;
        let std = var.sqrt();
        if std <= 1e-12 {
            continue;
        }
        if let Some(v) = values[i] {
            out[i] = Some((v - mean) / std);
        }
    }
    out
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    macro_contexts: &HashMap<String, MacroContext>,
    universe_symbols: &[&str],
    strategy: StrategyKind,
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in universe_symbols {
        let df = data_cache.get(symbol).unwrap();
        let ctx = macro_contexts.get(symbol).unwrap();
        let result = run_strategy(df, ctx, strategy)?;
        aggregate.add_assign(&result);
    }
    Ok(aggregate)
}

fn evaluate_strategy_on_windows(
    data_cache: &HashMap<String, DataFrame>,
    macro_contexts: &HashMap<String, MacroContext>,
    universe_symbols: &[&str],
    strategy: StrategyKind,
    windows: &[(usize, usize)],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in universe_symbols {
        let df = data_cache.get(symbol).unwrap();
        let ctx = macro_contexts.get(symbol).unwrap();
        for &(start, end) in windows {
            let result = run_strategy_in_window(df, ctx, strategy, start, end)?;
            aggregate.add_assign(&result);
        }
    }
    Ok(aggregate)
}

fn run_strategy(
    df: &DataFrame,
    ctx: &MacroContext,
    strategy: StrategyKind,
) -> Result<StrategyResult> {
    run_strategy_in_window(df, ctx, strategy, 0, df.height())
}

fn run_strategy_in_window(
    df: &DataFrame,
    ctx: &MacroContext,
    strategy: StrategyKind,
    start: usize,
    end: usize,
) -> Result<StrategyResult> {
    let opens: Vec<f64> = df
        .column("open")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let n = opens
        .len()
        .min(ctx.spx_signal.len())
        .min(ctx.vix_signal.len())
        .min(ctx.dxy_signal.len());
    let end = end.min(n);
    let signal_start = start.max(1);
    let mut result = StrategyResult::default();

    for i in signal_start..end.saturating_sub(HOLD_BARS + 1) {
        let signal = strategy_signal(ctx, strategy, i - 1);
        if signal == 0 {
            continue;
        }
        let entry = opens[i];
        let exit = opens[i + HOLD_BARS];
        if entry <= 0.0 || exit <= 0.0 {
            continue;
        }
        let gross = if signal > 0 {
            exit / entry - 1.0
        } else {
            entry / exit - 1.0
        };
        let net = gross - 2.0 * TAKER_FEE;
        result.total_return_pct += net * 100.0;
        result.trades += 1;
        if net > 0.0 {
            result.wins += 1;
        }
    }

    Ok(result)
}

fn strategy_signal(ctx: &MacroContext, strategy: StrategyKind, idx: usize) -> i8 {
    match strategy {
        StrategyKind::SpxFollow => ctx.spx_signal[idx],
        StrategyKind::VixInverse => ctx.vix_signal[idx],
        StrategyKind::DxyInverse => ctx.dxy_signal[idx],
        StrategyKind::ComboMajority => {
            let vote = ctx.spx_signal[idx] as i32
                + ctx.vix_signal[idx] as i32
                + ctx.dxy_signal[idx] as i32;
            if vote >= 2 {
                1
            } else if vote <= -2 {
                -1
            } else {
                0
            }
        }
    }
}

fn quarter_windows(
    data_cache: &HashMap<String, DataFrame>,
    universe_symbols: &[&str],
) -> Result<Vec<(usize, usize)>> {
    let n = min_rows(data_cache, universe_symbols)?;
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
    universe_symbols: &[&str],
) -> Result<Vec<Vec<(usize, usize)>>> {
    let n = min_rows(data_cache, universe_symbols)?;
    let block = n / RESAMPLE_BLOCKS;
    let mut blocks = Vec::new();
    for idx in 0..RESAMPLE_BLOCKS {
        let start = idx * block;
        let end = if idx == RESAMPLE_BLOCKS - 1 {
            n
        } else {
            (idx + 1) * block
        };
        blocks.push((start, end));
    }

    let mut out = Vec::new();
    for a in 0..RESAMPLE_BLOCKS {
        for b in (a + 1)..RESAMPLE_BLOCKS {
            if RESAMPLE_BLOCKS - 2 != RESAMPLE_TRAIN_BLOCKS {
                continue;
            }
            out.push(vec![blocks[a], blocks[b]]);
        }
    }
    Ok(out)
}

fn min_rows(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    symbols
        .iter()
        .map(|symbol| {
            data_cache
                .get(*symbol)
                .map(|df| df.height())
                .with_context(|| format!("missing dataframe for {}", symbol))
        })
        .collect::<Result<Vec<_>>>()
        .map(|heights| heights.into_iter().min().unwrap_or(0))
}

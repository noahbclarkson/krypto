//! Exogenous-context benchmark under the same fair daily execution assumptions.
//!
//! Goal:
//! - test whether simple macro / cross-market context filters add anything real
//!   to the current filtered-trend yardstick
//! - open the roadmap's exogenous-data bucket without pretending one session can
//!   solve the entire problem
//!
//! Conservative alignment rule:
//! - crypto signals on day D may only use the latest macro close strictly before D
//! - this intentionally lags SP500 / VIX / DXY by at least one calendar day to
//!   avoid same-day close leakage

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

const SP500_CSV: &str = "data/exogenous/sp500.csv";
const VIX_CSV: &str = "data/exogenous/vixcls.csv";
const DXY_CSV: &str = "data/exogenous/dxy.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    MacdRegime,
    MacdRegimeSpxRiskOn,
    MacdRegimeVixGuard,
    MacdRegimeDxyWeak,
    MacdRegimeMacroCombo,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::MacdRegime,
            Self::MacdRegimeSpxRiskOn,
            Self::MacdRegimeVixGuard,
            Self::MacdRegimeDxyWeak,
            Self::MacdRegimeMacroCombo,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::MacdRegime => "MACD+Regime",
            Self::MacdRegimeSpxRiskOn => "MACD+Regime+SPX risk-on",
            Self::MacdRegimeVixGuard => "MACD+Regime+VIX guard",
            Self::MacdRegimeDxyWeak => "MACD+Regime+DXY weak",
            Self::MacdRegimeMacroCombo => "MACD+Regime+macro combo",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::MacdRegime => "baseline filtered trend yardstick",
            Self::MacdRegimeSpxRiskOn => {
                "only take longs when lagged SP500 > SMA50; keep shorts unchanged"
            }
            Self::MacdRegimeVixGuard => {
                "only take longs when lagged VIX < SMA20; keep shorts unchanged"
            }
            Self::MacdRegimeDxyWeak => {
                "only take longs when lagged DXY < SMA50; keep shorts unchanged"
            }
            Self::MacdRegimeMacroCombo => {
                "longs require SP500 > SMA50 AND VIX < SMA20 AND DXY < SMA50; keep shorts unchanged"
            }
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
    spx_prev: Vec<Option<f64>>,
    spx_sma50_prev: Vec<Option<f64>>,
    vix_prev: Vec<Option<f64>>,
    vix_sma20_prev: Vec<Option<f64>>,
    dxy_prev: Vec<Option<f64>>,
    dxy_sma50_prev: Vec<Option<f64>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== EXOGENOUS CONTEXT BENCHMARK ===\n");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Macro alignment: use latest macro close strictly before each crypto bar date");
    println!("Stress universes: {}\n", UNIVERSES.len());

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

        if let Some(best) = rows.first() {
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
    println!("- This is a first exogenous-data benchmark, not a claim that macro closes can be naively pasted onto crypto and trusted.");
    println!("- If simple lagged macro gates cannot improve the filtered-trend yardstick, the value of exogenous data probably requires richer alignment / modeling than one-bit risk-on filters.");
    println!("- If one of these filters helps on harsher baskets without collapsing chronology, it earns deeper follow-up, not promotion.");

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
        spx_sma50_prev: rolling_sma(&spx_prev, 50),
        vix_sma20_prev: rolling_sma(&vix_prev, 20),
        dxy_sma50_prev: rolling_sma(&dxy_prev, 50),
        spx_prev,
        vix_prev,
        dxy_prev,
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

fn rolling_sma(series: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
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

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    macro_contexts: &HashMap<String, MacroContext>,
    symbols: &[&str],
    strategy: StrategyKind,
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
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
    symbols: &[&str],
    strategy: StrategyKind,
    windows: &[(usize, usize)],
) -> Result<StrategyResult> {
    let mut aggregate = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        let ctx = macro_contexts.get(symbol).unwrap();
        for &(start, end) in windows {
            let result = run_strategy_in_window(df, ctx, strategy, start, end)?;
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
                anyhow::bail!("unexpected CPCV configuration");
            }
            out.push(vec![blocks[a], blocks[b]]);
        }
    }
    Ok(out)
}

fn min_rows(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    let mut min_rows: Option<usize> = None;
    for &symbol in symbols {
        let rows = data_cache
            .get(symbol)
            .with_context(|| format!("missing data for {}", symbol))?
            .height();
        min_rows = Some(min_rows.map_or(rows, |curr| curr.min(rows)));
    }
    min_rows.context("no symbols provided")
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
    let signals = generate_signals(df, ctx, strategy)?;
    let open = df.column("open")?.f64()?;

    let start = start.max(1);
    let end = end.min(df.height());
    let mut result = StrategyResult::default();

    for i in start..end.saturating_sub(HOLD_BARS + 1) {
        let sig = signals[i - 1];
        if sig == 0 {
            continue;
        }

        let entry_idx = i;
        let exit_idx = i + HOLD_BARS;
        if exit_idx >= end {
            continue;
        }

        let entry = open.get(entry_idx).unwrap_or(0.0);
        let exit = open.get(exit_idx).unwrap_or(0.0);
        if entry <= 0.0 || exit <= 0.0 {
            continue;
        }

        let gross = if sig > 0 {
            (exit / entry) - 1.0
        } else {
            (entry / exit) - 1.0
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

fn generate_signals(
    df: &DataFrame,
    ctx: &MacroContext,
    strategy: StrategyKind,
) -> Result<Vec<i32>> {
    let base = generate_macd_regime_signals(df)?;
    let mut out = vec![0i32; base.len()];

    for i in 0..base.len() {
        let sig = base[i];
        if sig == 0 {
            continue;
        }
        if sig < 0 {
            out[i] = sig;
            continue;
        }

        let allow_long = match strategy {
            StrategyKind::MacdRegime => true,
            StrategyKind::MacdRegimeSpxRiskOn => gate_spx_risk_on(ctx, i),
            StrategyKind::MacdRegimeVixGuard => gate_vix_guard(ctx, i),
            StrategyKind::MacdRegimeDxyWeak => gate_dxy_weak(ctx, i),
            StrategyKind::MacdRegimeMacroCombo => {
                gate_spx_risk_on(ctx, i) && gate_vix_guard(ctx, i) && gate_dxy_weak(ctx, i)
            }
        };

        if allow_long {
            out[i] = sig;
        }
    }

    Ok(out)
}

fn gate_spx_risk_on(ctx: &MacroContext, i: usize) -> bool {
    matches!((ctx.spx_prev[i], ctx.spx_sma50_prev[i]), (Some(v), Some(sma)) if v > sma)
}

fn gate_vix_guard(ctx: &MacroContext, i: usize) -> bool {
    matches!((ctx.vix_prev[i], ctx.vix_sma20_prev[i]), (Some(v), Some(sma)) if v < sma)
}

fn gate_dxy_weak(ctx: &MacroContext, i: usize) -> bool {
    matches!((ctx.dxy_prev[i], ctx.dxy_sma50_prev[i]), (Some(v), Some(sma)) if v < sma)
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
    }
    Ok(signals)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = generate_macd_signals(df)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(close, 200);
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

fn calculate_sma(series: &Float64Chunked, period: usize) -> Vec<Option<f64>> {
    let n = series.len();
    let mut sma = vec![None; n];
    for i in period..n {
        let sum: f64 = (0..period).filter_map(|j| series.get(i - j)).sum();
        sma[i] = Some(sum / period as f64);
    }
    sma
}

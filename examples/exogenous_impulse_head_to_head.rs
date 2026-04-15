//! Apples-to-apples benchmark for the strongest direct exogenous impulse signals
//! against the current daily yardsticks.
//!
//! Goal:
//! - test whether the new macro-impulse family survives direct comparison rather than
//!   being judged only in its own isolated breadth table
//! - keep the same fair daily harness used elsewhere: signal at close, next-open entry,
//!   fixed 21-bar hold, realistic taker fees, chronology stress
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
const TURTLE_PERIOD: usize = 20;

const SP500_CSV: &str = "data/exogenous/sp500.csv";
const VIX_CSV: &str = "data/exogenous/vixcls.csv";
const DXY_CSV: &str = "data/exogenous/dxy.csv";
const SNAPSHOT_LATEST_MD: &str = "snapshots/exogenous_impulse_head_to_head_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/exogenous_impulse_head_to_head_latest.csv";
const SNAPSHOT_DIR: &str = "snapshots";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    SpxFollow,
    VixInverse,
    MacdRegime,
    TurtleRegimeMacd,
    EnsembleMajority3,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::SpxFollow,
            Self::VixInverse,
            Self::MacdRegime,
            Self::TurtleRegimeMacd,
            Self::EnsembleMajority3,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::SpxFollow => "SPX impulse follow",
            Self::VixInverse => "VIX impulse inverse",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
            Self::EnsembleMajority3 => "Ensemble(Majority 2/3)",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::SpxFollow => "direct exogenous signal: long after positive lagged SPX shock, short after negative shock",
            Self::VixInverse => "direct exogenous signal: short after positive lagged VIX shock, long after negative shock",
            Self::MacdRegime => "current filtered-trend yardstick",
            Self::TurtleRegimeMacd => "current trend-combo yardstick",
            Self::EnsembleMajority3 => "current breadth-combination yardstick",
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
}

#[derive(Clone, Debug)]
struct UniverseTopRow {
    universe: &'static str,
    symbols: &'static [&'static str],
    best: StrategyKind,
    best_return_pct: f64,
    second: StrategyKind,
    second_return_pct: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== EXOGENOUS IMPULSE HEAD-TO-HEAD ===\n");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Macro impulse: {}-day macro return, {}-day rolling z-score, threshold {:.1}",
        MACRO_RET_LOOKBACK, MACRO_Z_WINDOW, IMPULSE_Z_THRESHOLD
    );
    println!("Macro alignment: use latest macro close strictly before each crypto bar date");
    println!("Comparison set: SPX impulse, VIX impulse, MACD+Regime, Turtle+Regime+MACD, Majority ensemble\n");

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

    let mut universe_wins = HashMap::<StrategyKind, usize>::new();
    let mut all_rows: Vec<(&'static str, SummaryRow)> = Vec::new();
    let mut universe_tops = Vec::<UniverseTopRow>::new();

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
            *universe_wins.entry(best.kind).or_insert(0) += 1;
        }
        if rows.len() >= 2 {
            universe_tops.push(UniverseTopRow {
                universe: universe_name,
                symbols: universe_symbols,
                best: rows[0].kind,
                best_return_pct: rows[0].full_return_pct,
                second: rows[1].kind,
                second_return_pct: rows[1].full_return_pct,
            });
        }
        for row in rows {
            all_rows.push((universe_name, row));
        }
    }

    println!("\n=== UNIVERSE WIN COUNTS ===");
    for &strategy in StrategyKind::all() {
        println!(
            "{:<24} {:>2}/{}",
            strategy.name(),
            universe_wins.get(&strategy).copied().unwrap_or(0),
            UNIVERSES.len()
        );
    }

    write_snapshot(&all_rows, &universe_wins, &universe_tops)?;

    println!("\nInterpretation:");
    println!("- This is the honest next step after the first exogenous-impulse breadth screen: compare the strongest direct macro legs against the current daily yardsticks under the SAME universes and chronology rules.");
    println!("- If SPX/VIX impulse survives here, it earns attribution follow-up; if not, the earlier result was mostly an isolated breadth-table mirage.");
    println!("- Even a win here is still a research result, not a promotion decision.");

    Ok(())
}

fn write_snapshot(
    rows: &[(&'static str, SummaryRow)],
    universe_wins: &HashMap<StrategyKind, usize>,
    universe_tops: &[UniverseTopRow],
) -> Result<()> {
    std::fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!(
        "{}/exogenous_impulse_head_to_head_{}.md",
        SNAPSHOT_DIR, timestamp
    );
    let archive_csv = format!(
        "{}/exogenous_impulse_head_to_head_{}.csv",
        SNAPSHOT_DIR, timestamp
    );

    let mut md = String::new();
    md.push_str("# Exogenous Impulse Head-to-Head\n\n");
    md.push_str("Same fair daily harness for all rows: signal at close, next-open entry, fixed 21-bar hold, 0.1% taker each side.\n\n");
    md.push_str("## Universe winners\n\n");
    for top in universe_tops {
        md.push_str(&format!(
            "- **{}** ({}): **{}** {:.1}% vs **{}** {:.1}%\n",
            top.universe,
            top.symbols.join(", "),
            top.best.name(),
            top.best_return_pct,
            top.second.name(),
            top.second_return_pct,
        ));
    }
    md.push_str("\n## Win counts\n\n");
    for &strategy in StrategyKind::all() {
        md.push_str(&format!(
            "- **{}**: {}/{} universes\n",
            strategy.name(),
            universe_wins.get(&strategy).copied().unwrap_or(0),
            UNIVERSES.len()
        ));
    }
    md.push_str("\n## Per-universe detail\n\n");
    let mut current_universe = "";
    for (universe, row) in rows {
        if *universe != current_universe {
            current_universe = universe;
            md.push_str(&format!("### {}\n\n", universe));
        }
        md.push_str(&format!(
            "- **{}**: return {:.1}%, trades {}, win rate {:.1}%, WF {}/{}, CPCV {}/{}\n",
            row.kind.name(),
            row.full_return_pct,
            row.full_trades,
            row.full_win_rate * 100.0,
            row.wf_passed,
            row.wf_total,
            row.resample_passed,
            row.resample_total,
        ));
    }

    let mut csv = String::from("universe,strategy,full_return_pct,full_trades,full_win_rate,wf_passed,wf_total,resample_passed,resample_total\n");
    for (universe, row) in rows {
        csv.push_str(&format!(
            "{},{},{:.6},{},{:.6},{},{},{},{}\n",
            universe,
            row.kind.name(),
            row.full_return_pct,
            row.full_trades,
            row.full_win_rate,
            row.wf_passed,
            row.wf_total,
            row.resample_passed,
            row.resample_total,
        ));
    }

    std::fs::write(SNAPSHOT_LATEST_MD, &md)?;
    std::fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    std::fs::write(archive_md, md)?;
    std::fs::write(archive_csv, csv)?;
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

    Ok(MacroContext {
        spx_signal: impulse_signal(&spx_prev, 1.0),
        vix_signal: impulse_signal(&vix_prev, -1.0),
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
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let ctx = macro_contexts
            .get(symbol)
            .context("missing macro context")?;
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
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let ctx = macro_contexts
            .get(symbol)
            .context("missing macro context")?;
        let result = run_strategy_on_windows(df, ctx, strategy, windows)?;
        aggregate.add_assign(&result);
    }
    Ok(aggregate)
}

fn run_strategy(
    df: &DataFrame,
    ctx: &MacroContext,
    strategy: StrategyKind,
) -> Result<StrategyResult> {
    let signal = signal_for_strategy(df, ctx, strategy)?;
    backtest_fixed_hold_next_open_window(df, &signal, 0, df.height())
}

fn run_strategy_on_windows(
    df: &DataFrame,
    ctx: &MacroContext,
    strategy: StrategyKind,
    windows: &[(usize, usize)],
) -> Result<StrategyResult> {
    let signal = signal_for_strategy(df, ctx, strategy)?;
    let mut out = StrategyResult::default();
    for &(start, end) in windows {
        let result = backtest_fixed_hold_next_open_window(df, &signal, start, end)?;
        out.add_assign(&result);
    }
    Ok(out)
}

fn signal_for_strategy(
    df: &DataFrame,
    ctx: &MacroContext,
    strategy: StrategyKind,
) -> Result<Vec<i32>> {
    let macd_regime = generate_macd_regime_signals(df)?;
    let turtle_regime_macd = generate_turtle_regime_macd_signals(df, TURTLE_PERIOD)?;
    let spx = ctx.spx_signal.iter().map(|&v| v as i32).collect::<Vec<_>>();
    let vix = ctx.vix_signal.iter().map(|&v| v as i32).collect::<Vec<_>>();
    let ensemble_majority = consensus_signal(&[&macd_regime, &turtle_regime_macd, &spx], 2);

    Ok(match strategy {
        StrategyKind::SpxFollow => spx,
        StrategyKind::VixInverse => vix,
        StrategyKind::MacdRegime => macd_regime,
        StrategyKind::TurtleRegimeMacd => turtle_regime_macd,
        StrategyKind::EnsembleMajority3 => ensemble_majority,
    })
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
    let len = min_rows(data_cache, symbols)?;
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
    let len = min_rows(data_cache, symbols)?;
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
        let windows = test_blocks
            .into_iter()
            .map(|idx| ranges[idx])
            .collect::<Vec<_>>();
        out.push(windows);
    }
    Ok(out)
}

fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn recur(
        start: usize,
        n: usize,
        k: usize,
        current: &mut Vec<usize>,
        out: &mut Vec<Vec<usize>>,
    ) {
        if current.len() == k {
            out.push(current.clone());
            return;
        }
        for i in start..n {
            current.push(i);
            recur(i + 1, n, k, current, out);
            current.pop();
        }
    }

    let mut out = Vec::new();
    recur(0, n, k, &mut Vec::new(), &mut out);
    out
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

//! Symbol and macro-state attribution for the surviving direct exogenous impulse niches.
//!
//! Goal:
//! - avoid local parameter tuning after the first exogenous impulse head-to-head
//! - ask where the surviving SPX/VIX impulse rows actually earn their returns
//! - keep the same fair daily harness: signal at close, next-open entry,
//!   fixed 21-bar hold, 0.1% taker each side
//!
//! Conservative alignment rule:
//! - crypto bar on day D may only use the latest macro close strictly before D
//! - macro state is also determined only from lagged macro information

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
const MACRO_RET_LOOKBACK: usize = 5;
const MACRO_Z_WINDOW: usize = 63;
const IMPULSE_Z_THRESHOLD: f64 = 1.0;

const SP500_CSV: &str = "data/exogenous/sp500.csv";
const VIX_CSV: &str = "data/exogenous/vixcls.csv";
const DXY_CSV: &str = "data/exogenous/dxy.csv";
const SNAPSHOT_LATEST_MD: &str = "snapshots/exogenous_impulse_attribution_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/exogenous_impulse_attribution_latest.csv";
const SNAPSHOT_DIR: &str = "snapshots";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    SpxFollow,
    VixInverse,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[Self::SpxFollow, Self::VixInverse]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::SpxFollow => "SPX impulse follow",
            Self::VixInverse => "VIX impulse inverse",
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
struct MacroContext {
    spx_signal: Vec<i8>,
    vix_signal: Vec<i8>,
    macro_risk_on: Vec<bool>,
    macro_stress: Vec<bool>,
}

#[derive(Clone, Debug)]
struct SymbolAttributionRow {
    universe: &'static str,
    strategy: StrategyKind,
    symbol: String,
    total_return_pct: f64,
    trades: usize,
    win_rate: f64,
}

#[derive(Clone, Debug)]
struct StateAttributionRow {
    universe: &'static str,
    strategy: StrategyKind,
    bucket: MacroStateBucket,
    total_return_pct: f64,
    trades: usize,
    win_rate: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== EXOGENOUS IMPULSE ATTRIBUTION ===\n");
    println!("Goal: attribute the surviving direct exogenous impulse niches by symbol and lagged macro state instead of tuning them.");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Macro impulse: {}-day return, {}-day rolling z-score, threshold {:.1}",
        MACRO_RET_LOOKBACK, MACRO_Z_WINDOW, IMPULSE_Z_THRESHOLD
    );
    println!("Macro alignment: use latest macro close strictly before each crypto bar date\n");

    let macro_cache = fetch_macro_series()?;
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

    let mut symbol_rows = Vec::<SymbolAttributionRow>::new();
    let mut state_rows = Vec::<StateAttributionRow>::new();

    for &(universe_name, universe_symbols) in UNIVERSES {
        println!(
            "\n--- Universe: {} ({}) ---",
            universe_name,
            universe_symbols.join(", ")
        );
        for &strategy in StrategyKind::all() {
            println!("\n{}", strategy.name());
            let mut universe_total = StrategyResult::default();
            let mut by_state = BTreeMap::<MacroStateBucket, StrategyResult>::new();
            for bucket in MacroStateBucket::all() {
                by_state.insert(*bucket, StrategyResult::default());
            }

            let mut symbol_stats = Vec::<(String, StrategyResult)>::new();
            for &symbol in universe_symbols {
                let df = data_cache.get(symbol).context("missing symbol data")?;
                let ctx = macro_contexts
                    .get(symbol)
                    .context("missing macro context")?;
                let signal = signal_for_strategy(ctx, strategy);
                let (full, state_breakdown) = backtest_with_macro_attribution(df, &signal, ctx)?;
                universe_total.add_assign(&full);
                for bucket in MacroStateBucket::all() {
                    by_state
                        .get_mut(bucket)
                        .unwrap()
                        .add_assign(state_breakdown.get(bucket).unwrap());
                }
                symbol_stats.push((symbol.to_string(), full.clone()));
            }

            symbol_stats.sort_by(|a, b| {
                b.1.total_return_pct
                    .partial_cmp(&a.1.total_return_pct)
                    .unwrap()
                    .then_with(|| b.1.trades.cmp(&a.1.trades))
            });

            println!(
                "  Total: {:>10.1}% {:>5} trades {:>5.1}% win",
                universe_total.total_return_pct,
                universe_total.trades,
                universe_total.win_rate() * 100.0
            );
            println!("  Top symbols:");
            for (symbol, stats) in symbol_stats.iter().take(3) {
                println!(
                    "    {:<8} {:>10.1}% {:>5} trades {:>5.1}% win",
                    symbol,
                    stats.total_return_pct,
                    stats.trades,
                    stats.win_rate() * 100.0
                );
            }
            println!("  Bottom symbols:");
            for (symbol, stats) in symbol_stats.iter().rev().take(2) {
                println!(
                    "    {:<8} {:>10.1}% {:>5} trades {:>5.1}% win",
                    symbol,
                    stats.total_return_pct,
                    stats.trades,
                    stats.win_rate() * 100.0
                );
            }
            println!("  By lagged macro state:");
            for bucket in MacroStateBucket::all() {
                let stats = by_state.get(bucket).unwrap();
                println!(
                    "    {:<12} {:>10.1}% {:>5} trades {:>5.1}% win",
                    bucket.name(),
                    stats.total_return_pct,
                    stats.trades,
                    stats.win_rate() * 100.0
                );
                state_rows.push(StateAttributionRow {
                    universe: universe_name,
                    strategy,
                    bucket: *bucket,
                    total_return_pct: stats.total_return_pct,
                    trades: stats.trades,
                    win_rate: stats.win_rate(),
                });
            }

            for (symbol, stats) in symbol_stats {
                symbol_rows.push(SymbolAttributionRow {
                    universe: universe_name,
                    strategy,
                    symbol,
                    total_return_pct: stats.total_return_pct,
                    trades: stats.trades,
                    win_rate: stats.win_rate(),
                });
            }
        }
    }

    write_snapshot(&symbol_rows, &state_rows)?;

    println!("\nInterpretation:");
    println!("- If the surviving exogenous niches are broad across symbols and concentrated in a coherent macro bucket, they earn deeper structural follow-up.");
    println!("- If they are mostly one- or two-symbol stories, they stay secondary breadth candidates rather than new benchmark leaders.");
    println!("- This is an attribution audit, not a promotion decision.");

    Ok(())
}

fn write_snapshot(
    symbol_rows: &[SymbolAttributionRow],
    state_rows: &[StateAttributionRow],
) -> Result<()> {
    std::fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!(
        "{}/exogenous_impulse_attribution_{}.md",
        SNAPSHOT_DIR, timestamp
    );
    let archive_csv = format!(
        "{}/exogenous_impulse_attribution_{}.csv",
        SNAPSHOT_DIR, timestamp
    );

    let mut md = String::new();
    md.push_str("# Exogenous Impulse Attribution\n\n");
    md.push_str("Same fair daily harness for all rows: signal at close, next-open entry, fixed 21-bar hold, 0.1% taker each side.\n\n");

    for &(universe_name, _) in UNIVERSES {
        md.push_str(&format!("## {}\n\n", universe_name));
        for &strategy in StrategyKind::all() {
            md.push_str(&format!("### {}\n\n", strategy.name()));
            md.push_str("**Top symbols**\n\n");
            let mut universe_symbols = symbol_rows
                .iter()
                .filter(|r| r.universe == universe_name && r.strategy == strategy)
                .collect::<Vec<_>>();
            universe_symbols.sort_by(|a, b| {
                b.total_return_pct
                    .partial_cmp(&a.total_return_pct)
                    .unwrap()
                    .then_with(|| b.trades.cmp(&a.trades))
            });
            for row in universe_symbols.iter().take(3) {
                md.push_str(&format!(
                    "- {}: {:.1}% return, {} trades, {:.1}% win rate\n",
                    row.symbol,
                    row.total_return_pct,
                    row.trades,
                    row.win_rate * 100.0,
                ));
            }
            md.push_str("\n**Bottom symbols**\n\n");
            for row in universe_symbols.iter().rev().take(2) {
                md.push_str(&format!(
                    "- {}: {:.1}% return, {} trades, {:.1}% win rate\n",
                    row.symbol,
                    row.total_return_pct,
                    row.trades,
                    row.win_rate * 100.0,
                ));
            }
            md.push_str("\n**By lagged macro state**\n\n");
            for bucket in MacroStateBucket::all() {
                let row = state_rows
                    .iter()
                    .find(|r| {
                        r.universe == universe_name && r.strategy == strategy && r.bucket == *bucket
                    })
                    .context("missing state row")?;
                md.push_str(&format!(
                    "- {}: {:.1}% return, {} trades, {:.1}% win rate\n",
                    bucket.name(),
                    row.total_return_pct,
                    row.trades,
                    row.win_rate * 100.0,
                ));
            }
            md.push_str("\n");
        }
    }

    let mut csv =
        String::from("kind,universe,strategy,bucket_or_symbol,total_return_pct,trades,win_rate\n");
    for row in state_rows {
        csv.push_str(&format!(
            "state,{},{},{},{:.6},{},{:.6}\n",
            row.universe,
            row.strategy.name(),
            row.bucket.name(),
            row.total_return_pct,
            row.trades,
            row.win_rate,
        ));
    }
    for row in symbol_rows {
        csv.push_str(&format!(
            "symbol,{},{},{},{:.6},{},{:.6}\n",
            row.universe,
            row.strategy.name(),
            row.symbol,
            row.total_return_pct,
            row.trades,
            row.win_rate,
        ));
    }

    std::fs::write(SNAPSHOT_LATEST_MD, &md)?;
    std::fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    std::fs::write(archive_md, md)?;
    std::fs::write(archive_csv, csv)?;
    Ok(())
}

fn signal_for_strategy(ctx: &MacroContext, strategy: StrategyKind) -> Vec<i32> {
    match strategy {
        StrategyKind::SpxFollow => ctx.spx_signal.iter().map(|&v| v as i32).collect(),
        StrategyKind::VixInverse => ctx.vix_signal.iter().map(|&v| v as i32).collect(),
    }
}

fn backtest_with_macro_attribution(
    df: &DataFrame,
    signals: &[i32],
    ctx: &MacroContext,
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

        let bucket = macro_bucket(ctx, i - 1);
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

fn macro_bucket(ctx: &MacroContext, idx: usize) -> MacroStateBucket {
    if ctx.macro_risk_on.get(idx).copied().unwrap_or(false) {
        MacroStateBucket::RiskOn
    } else if ctx.macro_stress.get(idx).copied().unwrap_or(false) {
        MacroStateBucket::Stress
    } else {
        MacroStateBucket::Neutral
    }
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

fn build_macro_context(
    df: &DataFrame,
    macro_cache: &HashMap<&'static str, BTreeMap<NaiveDate, f64>>,
) -> Result<MacroContext> {
    let dates = crypto_dates(df)?;
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

    let mut macro_risk_on = vec![false; df.height()];
    let mut macro_stress = vec![false; df.height()];
    for i in 0..df.height() {
        macro_risk_on[i] = matches!((spx_prev[i], spx_sma50[i], vix_prev[i], vix_sma20[i], dxy_prev[i], dxy_sma50[i]),
            (Some(spx), Some(spx_sma), Some(vix), Some(vix_sma), Some(dxy), Some(dxy_sma))
                if spx > spx_sma && vix < vix_sma && dxy < dxy_sma);
        macro_stress[i] = matches!((vix_prev[i], vix_sma20[i]), (Some(vix), Some(vix_sma)) if vix > vix_sma)
            || matches!((dxy_prev[i], dxy_sma50[i]), (Some(dxy), Some(dxy_sma)) if dxy > dxy_sma);
    }

    Ok(MacroContext {
        spx_signal: impulse_signal(&spx_prev, 1.0),
        vix_signal: impulse_signal(&vix_prev, -1.0),
        macro_risk_on,
        macro_stress,
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
        out.push(series.range(..*date).next_back().map(|(_, v)| *v));
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

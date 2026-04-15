//! Audit the surviving direct exogenous impulse signals under the realistic capped-book
//! portfolio lens instead of summed-trade headlines.
//!
//! Purpose:
//! - rebalance away from isolated exogenous breadth tables and ask the more honest question:
//!   do SPX/VIX impulse signals still matter once capital is allocated through a top-3 book?
//! - compare them directly against the incumbent daily yardsticks under the same execution model
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry at next open
//! - exit at open after fixed 21-bar hold
//! - 0.1% taker fee on entry and exit
//! - top-3 strength-capped portfolio book
//! - lagged macro close only (strictly before each crypto session date)

use anyhow::{Context, Result};
use chrono::{NaiveDate, TimeZone, Utc};
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
};

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
const POSITION_CAP: usize = 3;
const WARMUP_BARS: usize = 200;
const CS_LOOKBACK: usize = 63;
const TURTLE_PERIOD: usize = 20;
const MACRO_RET_LOOKBACK: usize = 5;
const MACRO_Z_WINDOW: usize = 63;
const IMPULSE_Z_THRESHOLD: f64 = 1.0;

const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/exogenous_impulse_portfolio_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/exogenous_impulse_portfolio_latest.csv";
const SP500_CSV: &str = "data/exogenous/sp500.csv";
const VIX_CSV: &str = "data/exogenous/vixcls.csv";
const DXY_CSV: &str = "data/exogenous/dxy.csv";

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum MacroStateBucket {
    RiskOn,
    Stress,
    Neutral,
}

impl MacroStateBucket {
    fn name(&self) -> &'static str {
        match self {
            Self::RiskOn => "MacroRiskOn",
            Self::Stress => "MacroStress",
            Self::Neutral => "MacroNeutral",
        }
    }
}

#[derive(Clone, Debug)]
struct MacroContext {
    spx_signal: Vec<i8>,
    vix_signal: Vec<i8>,
}

#[derive(Clone, Debug)]
struct MarketState {
    macro_risk_on: Vec<bool>,
    macro_stress: Vec<bool>,
}

#[derive(Clone, Debug)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    gross_return: f64,
    net_return: f64,
}

#[derive(Clone, Debug)]
struct SymbolPlan {
    trades: Vec<TradeWindow>,
}

#[derive(Clone, Debug, Default)]
struct PortfolioStats {
    aligned_return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate_pct: f64,
    avg_active_positions: f64,
    idle_days_pct: f64,
    state_return_pct: BTreeMap<MacroStateBucket, f64>,
}

#[derive(Clone, Debug)]
struct ResultRow {
    strategy: StrategyKind,
    stats: PortfolioStats,
}

#[derive(Clone, Debug)]
struct UniverseSummary {
    label: String,
    rows: Vec<ResultRow>,
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== EXOGENOUS IMPULSE PORTFOLIO AUDIT ===\n");
    println!("Goal: test whether direct exogenous impulse families survive the realistic capped-book portfolio lens");
    println!("Benchmark state anchor: {}", BENCHMARK);
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Portfolio lens: top-{} strength-capped book", POSITION_CAP);
    println!("Macro alignment: use latest macro close strictly before the crypto session date\n");

    let macro_cache = fetch_macro_series()?;
    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;
    let market_state = build_macro_state(&bench_df, &macro_cache)?;

    let mut data_cache = HashMap::<String, DataFrame>::new();
    let mut macro_contexts = HashMap::<String, MacroContext>::new();
    data_cache.insert(BENCHMARK.to_string(), bench_df.clone());
    macro_contexts.insert(
        BENCHMARK.to_string(),
        build_macro_context(&bench_df, &macro_cache)?,
    );

    for &symbol in LOAD_SYMBOLS.iter().filter(|&&s| s != BENCHMARK) {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, Some(&bench_df))?;
        println!("{} bars", enriched.height());
        let ctx = build_macro_context(&enriched, &macro_cache)?;
        macro_contexts.insert(symbol.to_string(), ctx);
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

    let mut universe_summaries = Vec::new();
    for &(label, universe_symbols) in UNIVERSES {
        println!(
            "\n--- Universe: {} ({}) ---",
            label,
            universe_symbols.join(", ")
        );
        let universe = aligned_universe(&data_cache, universe_symbols)?;
        let mut rows = Vec::new();

        for &strategy in StrategyKind::all() {
            let plans = build_symbol_plans(&universe, &macro_contexts, strategy)?;
            let stats = simulate_portfolio(&plans, universe.steps, &market_state);
            rows.push(ResultRow { strategy, stats });
        }

        rows.sort_by(|a, b| {
            b.stats
                .sharpe
                .partial_cmp(&a.stats.sharpe)
                .unwrap()
                .then_with(|| {
                    b.stats
                        .aligned_return_pct
                        .partial_cmp(&a.stats.aligned_return_pct)
                        .unwrap()
                })
        });

        println!(
            "{:<24} {:>11} {:>8} {:>8} {:>7} {:>7}",
            "Strategy", "Ret%", "Sharpe", "MaxDD", "Trades", "Win%"
        );
        println!("{}", "-".repeat(82));
        for row in &rows {
            println!(
                "{:<24} {:>10.1} {:>8.2} {:>7.1} {:>7} {:>6.1}",
                row.strategy.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
            );
        }

        universe_summaries.push(UniverseSummary {
            label: label.to_string(),
            rows,
        });
    }

    let (archive_md, archive_csv) = write_snapshot(&universe_summaries)?;
    println!("\nSnapshots written:");
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);
    println!("- {}", archive_md);
    println!("- {}", archive_csv);

    println!("\nInterpretation:");
    println!("- If SPX/VIX impulse rows stay competitive here, they are more than summed-trade curiosities.");
    println!("- If they fade sharply under the capped-book lens, the earlier breadth result was mostly a trade-aggregation artifact.");
    println!("- This is still a trust / breadth audit, not a promotion result.");

    Ok(())
}

fn aligned_universe(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<UniverseData> {
    let min_len = symbols
        .iter()
        .filter_map(|symbol| data_cache.get(*symbol).map(|df| df.height()))
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty universe"))?;
    let steps = min_len.saturating_sub(1);
    if steps == 0 {
        anyhow::bail!("not enough data")
    }

    let mut data = Vec::new();
    for &symbol in symbols {
        let df = data_cache
            .get(symbol)
            .with_context(|| format!("missing symbol {symbol}"))?
            .slice(0, min_len);
        data.push((symbol.to_string(), df));
    }

    Ok(UniverseData { data, steps })
}

fn build_symbol_plans(
    universe: &UniverseData,
    macro_contexts: &HashMap<String, MacroContext>,
    strategy: StrategyKind,
) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(symbol, df)| {
            let ctx = macro_contexts
                .get(symbol)
                .with_context(|| format!("missing macro context for {symbol}"))?;
            build_symbol_plan(df, ctx, strategy)
        })
        .collect()
}

fn build_symbol_plan(
    df: &DataFrame,
    ctx: &MacroContext,
    strategy: StrategyKind,
) -> Result<SymbolPlan> {
    let signals = signal_for_strategy(df, ctx, strategy)?;
    let strengths = strengths_for_strategy(df, ctx, strategy)?;
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let mut trades = Vec::new();
    let mut i = WARMUP_BARS;

    while i + HOLD_BARS + 1 < n {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }

        let entry_idx = i + 1;
        let exit_idx = i + 1 + HOLD_BARS;
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

        let gross_return = if signal > 0 {
            exit / entry - 1.0
        } else {
            entry / exit - 1.0
        };
        let net_return = gross_return - 2.0 * TAKER_FEE;
        trades.push(TradeWindow {
            entry_idx,
            exit_idx,
            strength: strengths.get(i).copied().unwrap_or(0.0).abs(),
            gross_return,
            net_return,
        });
        i = exit_idx;
    }

    Ok(SymbolPlan { trades })
}

fn simulate_portfolio(
    plans: &[SymbolPlan],
    steps: usize,
    market_state: &MarketState,
) -> PortfolioStats {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut trade_count = 0usize;
    let mut wins = 0usize;
    let mut state_return_pct = BTreeMap::<MacroStateBucket, f64>::new();

    for bucket in [
        MacroStateBucket::RiskOn,
        MacroStateBucket::Stress,
        MacroStateBucket::Neutral,
    ] {
        state_return_pct.insert(bucket, 0.0);
    }

    for plan in plans {
        for trade in &plan.trades {
            if trade.net_return > 0.0 {
                wins += 1;
            }
            trade_count += 1;
        }
    }

    for day in 0..steps {
        let mut active = Vec::<(f64, f64)>::new();
        for plan in plans {
            for trade in &plan.trades {
                if day == trade.entry_idx {
                    active.push((trade.strength, -TAKER_FEE));
                }
                if day >= trade.entry_idx && day < trade.exit_idx {
                    let span = (trade.exit_idx - trade.entry_idx) as f64;
                    if span > 0.0 {
                        active.push((trade.strength, trade.gross_return / span));
                    }
                }
                if day == trade.exit_idx {
                    active.push((trade.strength, -TAKER_FEE));
                }
            }
        }

        active_counts[day] = active.len();
        let state = macro_bucket(market_state, day.saturating_sub(1));

        if active.is_empty() {
            daily_returns[day] = 0.0;
            equity_curve[day + 1] = equity_curve[day];
            continue;
        }

        active.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let selected = &active[..POSITION_CAP.min(active.len())];
        let avg_ret = selected.iter().map(|(_, r)| *r).sum::<f64>() / selected.len() as f64;
        daily_returns[day] = avg_ret;
        equity_curve[day + 1] = equity_curve[day] * (1.0 + avg_ret);
        *state_return_pct.get_mut(&state).unwrap() += avg_ret * 100.0;
    }
    active_counts[steps] = active_counts[steps.saturating_sub(1)];

    let aligned_return_pct = (equity_curve.last().copied().unwrap_or(1.0) - 1.0) * 100.0;
    let sharpe = calc_sharpe_from_returns(&daily_returns);
    let max_dd_pct = calc_max_drawdown_pct(&equity_curve);
    let avg_active_positions =
        active_counts.iter().sum::<usize>() as f64 / active_counts.len() as f64;
    let idle_days_pct = active_counts.iter().filter(|&&c| c == 0).count() as f64
        / active_counts.len() as f64
        * 100.0;
    let win_rate_pct = if trade_count == 0 {
        0.0
    } else {
        wins as f64 / trade_count as f64 * 100.0
    };

    PortfolioStats {
        aligned_return_pct,
        sharpe,
        max_dd_pct,
        trades: trade_count,
        win_rate_pct,
        avg_active_positions,
        idle_days_pct,
        state_return_pct,
    }
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
    let ensemble = consensus_signal(&[&macd_regime, &turtle_regime_macd, &spx], 2);

    Ok(match strategy {
        StrategyKind::SpxFollow => spx,
        StrategyKind::VixInverse => vix,
        StrategyKind::MacdRegime => macd_regime,
        StrategyKind::TurtleRegimeMacd => turtle_regime_macd,
        StrategyKind::EnsembleMajority3 => ensemble,
    })
}

fn strengths_for_strategy(
    df: &DataFrame,
    ctx: &MacroContext,
    strategy: StrategyKind,
) -> Result<Vec<f64>> {
    match strategy {
        StrategyKind::SpxFollow => Ok(ctx.spx_signal.iter().map(|&v| (v as f64).abs()).collect()),
        StrategyKind::VixInverse => Ok(ctx.vix_signal.iter().map(|&v| (v as f64).abs()).collect()),
        StrategyKind::MacdRegime | StrategyKind::EnsembleMajority3 => generate_macd_strengths(df),
        StrategyKind::TurtleRegimeMacd => generate_turtle_strengths(df, TURTLE_PERIOD),
    }
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
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {} from {}", name, path))?;
        let series = parse_fred_csv(&text, name)?;
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

    Ok(MacroContext {
        spx_signal: impulse_signal(&spx_prev, 1.0),
        vix_signal: impulse_signal(&vix_prev, -1.0),
    })
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
    dates
        .iter()
        .map(|date| series.range(..*date).next_back().map(|(_, v)| *v))
        .collect()
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

fn rolling_sma_option(values: &[Option<f64>], window: usize) -> Vec<Option<f64>> {
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
        out[i] = Some(clean.iter().sum::<f64>() / clean.len() as f64);
    }
    out
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

fn generate_macd_strengths(df: &DataFrame) -> Result<Vec<f64>> {
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0.0; macd.len()];
    for i in 0..macd.len() {
        out[i] = (macd.get(i).unwrap_or(0.0) - signal.get(i).unwrap_or(0.0)).abs();
    }
    Ok(out)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = generate_macd_signals(df)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
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

fn generate_turtle_strengths(df: &DataFrame, period: usize) -> Result<Vec<f64>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();
    let mut strengths = vec![0.0; n];

    for i in period..n {
        let period_high = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let period_low = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let current_close = close.get(i).unwrap_or(0.0);
        let breakout_up = if period_high.is_finite() && period_high > 0.0 {
            (current_close / period_high - 1.0).max(0.0)
        } else {
            0.0
        };
        let breakout_down = if period_low.is_finite() && period_low > 0.0 {
            (period_low / current_close - 1.0).max(0.0)
        } else {
            0.0
        };
        strengths[i] = breakout_up.max(breakout_down);
    }
    Ok(strengths)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_regime_signals(df, period)?;
    let macd = generate_macd_signals(df)?;
    let mut out = vec![0i32; df.height()];
    for i in 0..df.height() {
        if turtle[i] != 0 && turtle[i] == macd[i] {
            out[i] = turtle[i];
        }
    }
    Ok(out)
}

fn generate_turtle_regime_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_signals(df, period)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; df.height()];
    for i in 0..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200[i].unwrap_or(0.0);
        if sma <= 0.0 {
            continue;
        }
        if turtle[i] > 0 && price > sma {
            out[i] = 1;
        } else if turtle[i] < 0 && price < sma {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn calculate_sma(series: &Float64Chunked, period: usize) -> Vec<Option<f64>> {
    let n = series.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    let mut sum = 0.0;
    let mut window = std::collections::VecDeque::<f64>::new();
    for i in 0..n {
        let v = series.get(i).unwrap_or(0.0);
        window.push_back(v);
        sum += v;
        if window.len() > period {
            if let Some(old) = window.pop_front() {
                sum -= old;
            }
        }
        if window.len() == period {
            out[i] = Some(sum / period as f64);
        }
    }
    out
}

fn calc_sharpe_from_returns(returns: &[f64]) -> f64 {
    if returns.len() < 2 {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    let std = variance.sqrt();
    if std <= 1e-12 {
        return 0.0;
    }
    mean / std * 252.0f64.sqrt()
}

fn calc_max_drawdown_pct(equity_curve: &[f64]) -> f64 {
    let mut peak = equity_curve.first().copied().unwrap_or(1.0);
    let mut max_dd: f64 = 0.0;
    for &eq in equity_curve {
        peak = peak.max(eq);
        if peak > 0.0 {
            max_dd = max_dd.max(1.0 - eq / peak);
        }
    }
    max_dd * 100.0
}

fn write_snapshot(summaries: &[UniverseSummary]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!(
        "{}/exogenous_impulse_portfolio_{}.md",
        SNAPSHOT_DIR, timestamp
    );
    let archive_csv = format!(
        "{}/exogenous_impulse_portfolio_{}.csv",
        SNAPSHOT_DIR, timestamp
    );

    let md = render_markdown(summaries);
    let csv = render_csv(summaries);
    fs::write(SNAPSHOT_LATEST_MD, &md)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, md)?;
    fs::write(&archive_csv, csv)?;
    Ok((archive_md, archive_csv))
}

fn render_markdown(summaries: &[UniverseSummary]) -> String {
    let mut out = String::new();
    out.push_str("# Exogenous Impulse Portfolio Audit\n\n");
    out.push_str("Fair daily harness: signal at close, next-open entry, fixed 21-bar hold, 0.1% taker each side, top-3 strength-capped book.\n\n");
    for summary in summaries {
        out.push_str(&format!("## {}\n\n", summary.label));
        out.push_str("| Strategy | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Active | Idle % | RiskOn % | Stress % | Neutral % |\n");
        out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
        for row in &summary.rows {
            out.push_str(&format!(
                "| {} | {:.1} | {:.2} | {:.1} | {} | {:.1} | {:.2} | {:.1} | {:.1} | {:.1} | {:.1} |\n",
                row.strategy.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
                row.stats.state_return_pct.get(&MacroStateBucket::RiskOn).copied().unwrap_or(0.0),
                row.stats.state_return_pct.get(&MacroStateBucket::Stress).copied().unwrap_or(0.0),
                row.stats.state_return_pct.get(&MacroStateBucket::Neutral).copied().unwrap_or(0.0),
            ));
        }
        out.push('\n');
    }
    out
}

fn render_csv(summaries: &[UniverseSummary]) -> String {
    let mut out = String::from("universe,strategy,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_active_positions,idle_days_pct,risk_on_return_pct,stress_return_pct,neutral_return_pct\n");
    for summary in summaries {
        for row in &summary.rows {
            out.push_str(&format!(
                "{},{},{:.4},{:.4},{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                summary.label,
                row.strategy.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
                row.stats
                    .state_return_pct
                    .get(&MacroStateBucket::RiskOn)
                    .copied()
                    .unwrap_or(0.0),
                row.stats
                    .state_return_pct
                    .get(&MacroStateBucket::Stress)
                    .copied()
                    .unwrap_or(0.0),
                row.stats
                    .state_return_pct
                    .get(&MacroStateBucket::Neutral)
                    .copied()
                    .unwrap_or(0.0),
            ));
        }
    }
    out
}

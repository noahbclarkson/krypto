//! Audit whether lagged macro state is useful for capped-book sizing / risk control.
//!
//! This is a bridge between Track A (trust / portfolio realism) and Track C (exogenous context):
//! instead of searching for another switch-rule winner, test whether macro state improves
//! capped-book portfolio behaviour for the current daily family yardsticks.
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry at next open
//! - exit at open after fixed 21-bar hold
//! - 0.1% taker fee on entry and exit
//! - top-3 strength-capped book
//! - lagged macro state from SP500 / VIX / DXY, known strictly before the crypto session date

use anyhow::{Context, Result};
use chrono::{NaiveDate, TimeZone, Utc};
use krypto::{
    algo::{strategies::CrossSectionalMomentum, SignalGenerator},
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
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/exogenous_portfolio_overlay_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/exogenous_portfolio_overlay_latest.csv";
const SP500_CSV: &str = "data/exogenous/sp500.csv";
const VIX_CSV: &str = "data/exogenous/vixcls.csv";
const DXY_CSV: &str = "data/exogenous/dxy.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    CrossSectionalMomentum,
    MacdRegime,
    TurtleRegimeMacd,
    EnsembleMajority3,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::CrossSectionalMomentum,
            Self::MacdRegime,
            Self::TurtleRegimeMacd,
            Self::EnsembleMajority3,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
            Self::EnsembleMajority3 => "Ensemble(Majority 2/3)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum OverlayKind {
    Baseline,
    StressTilt,
    StressOnly,
}

impl OverlayKind {
    fn all() -> &'static [OverlayKind] {
        &[Self::Baseline, Self::StressTilt, Self::StressOnly]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Baseline => "Baseline(1.0 all states)",
            Self::StressTilt => "StressTilt(1.0 stress / 0.5 else)",
            Self::StressOnly => "StressOnly(1.0 stress / 0.0 else)",
        }
    }

    fn exposure(&self, state: MacroStateBucket) -> f64 {
        match self {
            Self::Baseline => 1.0,
            Self::StressTilt => match state {
                MacroStateBucket::Stress => 1.0,
                MacroStateBucket::RiskOn | MacroStateBucket::Neutral => 0.5,
            },
            Self::StressOnly => match state {
                MacroStateBucket::Stress => 1.0,
                MacroStateBucket::RiskOn | MacroStateBucket::Neutral => 0.0,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum MacroStateBucket {
    RiskOn,
    Stress,
    Neutral,
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
    avg_exposure: f64,
    state_return_pct: BTreeMap<MacroStateBucket, f64>,
}

#[derive(Clone, Debug)]
struct ResultRow {
    strategy: StrategyKind,
    overlay: OverlayKind,
    stats: PortfolioStats,
}

#[derive(Clone, Debug)]
struct UniverseSummary {
    label: String,
    rows: Vec<ResultRow>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== EXOGENOUS PORTFOLIO OVERLAY AUDIT ===\n");
    println!("Goal: test whether lagged macro state helps capped-book sizing / drawdown control");
    println!("Benchmark state anchor: {}", BENCHMARK);
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Portfolio lens: top-{} strength-capped book\n",
        POSITION_CAP
    );

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
            let plans = build_symbol_plans(&universe, strategy)?;
            for &overlay in OverlayKind::all() {
                let stats = simulate_portfolio(&plans, universe.steps, &market_state, overlay);
                rows.push(ResultRow {
                    strategy,
                    overlay,
                    stats,
                });
            }
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
            "{:<28} {:<34} {:>10} {:>8} {:>8} {:>7} {:>7}",
            "Strategy", "Overlay", "Ret%", "Sharpe", "MaxDD", "Trades", "Exp"
        );
        println!("{}", "-".repeat(112));
        for row in &rows {
            println!(
                "{:<28} {:<34} {:>9.1} {:>8.2} {:>7.1} {:>7} {:>6.2}",
                row.strategy.name(),
                row.overlay.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.avg_exposure,
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
    println!("- If stress-aware overlays improve Sharpe / DD without killing trade count, macro context may be more useful for sizing than for family switching.");
    println!("- If StressOnly collapses or becomes too sparse, that means the macro-stress insight is real but too blunt to trade as a hard gate.");
    println!("- This is a portfolio-risk audit, not a new strategy promotion result.");

    Ok(())
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

fn aligned_universe(
    raw_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<UniverseData> {
    let min_len = symbols
        .iter()
        .filter_map(|symbol| raw_cache.get(*symbol).map(|df| df.height()))
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty universe"))?;
    let steps = min_len.saturating_sub(1);
    if steps == 0 {
        anyhow::bail!("not enough data");
    }

    let mut data = Vec::new();
    for &symbol in symbols {
        let df = raw_cache
            .get(symbol)
            .ok_or_else(|| anyhow::anyhow!("missing symbol {symbol}"))?
            .slice(0, min_len);
        data.push((symbol.to_string(), df));
    }

    Ok(UniverseData { data, steps })
}

fn build_symbol_plans(universe: &UniverseData, strategy: StrategyKind) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(_, df)| build_symbol_plan(df, strategy))
        .collect()
}

fn build_symbol_plan(df: &DataFrame, strategy: StrategyKind) -> Result<SymbolPlan> {
    let signals = signal_for_strategy(df, strategy)?;
    let strengths = strengths_for_strategy(df, strategy)?;
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
    overlay: OverlayKind,
) -> PortfolioStats {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut exposure_sum = 0.0;
    let mut state_return_pct = BTreeMap::<MacroStateBucket, f64>::new();
    let mut trade_count = 0usize;
    let mut wins = 0usize;

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
        let exposure = overlay.exposure(state);
        exposure_sum += exposure;

        if active.is_empty() || exposure <= 0.0 {
            daily_returns[day] = 0.0;
            equity_curve[day + 1] = equity_curve[day];
            continue;
        }

        active.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let selected_len = POSITION_CAP.min(active.len());
        let selected = &active[..selected_len];
        let avg_ret = selected.iter().map(|(_, r)| *r).sum::<f64>() / selected.len() as f64;
        let scaled_ret = avg_ret * exposure;
        daily_returns[day] = scaled_ret;
        equity_curve[day + 1] = equity_curve[day] * (1.0 + scaled_ret);
        *state_return_pct.get_mut(&state).unwrap() += scaled_ret * 100.0;
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
    let avg_exposure = exposure_sum / steps.max(1) as f64;

    PortfolioStats {
        aligned_return_pct,
        sharpe,
        max_dd_pct,
        trades: trade_count,
        win_rate_pct,
        avg_active_positions,
        idle_days_pct,
        avg_exposure,
        state_return_pct,
    }
}

fn signal_for_strategy(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
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
    })
}

fn strengths_for_strategy(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<f64>> {
    match strategy {
        StrategyKind::CrossSectionalMomentum => {
            let ranks = df.column("cs_momentum_rank")?.f64()?;
            Ok((0..ranks.len())
                .map(|i| (ranks.get(i).unwrap_or(0.5) - 0.5).abs())
                .collect())
        }
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

fn generate_macd_strengths(df: &DataFrame) -> Result<Vec<f64>> {
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0.0; macd.len()];
    for i in 0..macd.len() {
        out[i] = (macd.get(i).unwrap_or(0.0) - signal.get(i).unwrap_or(0.0)).abs();
    }
    Ok(out)
}

fn generate_turtle_strengths(df: &DataFrame, period: usize) -> Result<Vec<f64>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();
    let mut out = vec![0.0; n];
    for i in period..n {
        let period_high = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let period_low = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let current_close = close.get(i).unwrap_or(0.0);
        let range = (period_high - period_low).abs().max(1e-9);
        if current_close > period_high {
            out[i] = (current_close - period_high) / range;
        } else if current_close < period_low {
            out[i] = (period_low - current_close) / range;
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

fn calc_sharpe_from_returns(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns
        .iter()
        .map(|r| {
            let d = r - mean;
            d * d
        })
        .sum::<f64>()
        / returns.len() as f64;
    let std = var.sqrt();
    if std <= 1e-12 {
        0.0
    } else {
        mean / std * 365.0f64.sqrt()
    }
}

fn calc_max_drawdown_pct(equity_curve: &[f64]) -> f64 {
    let mut peak = equity_curve.first().copied().unwrap_or(1.0);
    let mut max_dd = 0.0;
    for &value in equity_curve {
        if value > peak {
            peak = value;
        }
        let dd = (value / peak - 1.0) * 100.0;
        if dd < max_dd {
            max_dd = dd;
        }
    }
    max_dd.abs()
}

fn write_snapshot(universes: &[UniverseSummary]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!(
        "{}/exogenous_portfolio_overlay_{}.md",
        SNAPSHOT_DIR, timestamp
    );
    let archive_csv = format!(
        "{}/exogenous_portfolio_overlay_{}.csv",
        SNAPSHOT_DIR, timestamp
    );

    let mut markdown = String::new();
    markdown.push_str("# Exogenous Portfolio Overlay Audit\n\n");
    markdown.push_str(&format!("- Timestamp (UTC): {}\n", Utc::now().to_rfc3339()));
    markdown.push_str(&format!("- Top-{} strength-capped book\n", POSITION_CAP));
    markdown.push_str(&format!("- Hold bars: {}\n", HOLD_BARS));
    markdown.push_str(&format!("- Fee each side: {:.3}%\n\n", TAKER_FEE * 100.0));

    for universe in universes {
        markdown.push_str(&format!("## {}\n\n", universe.label));
        markdown.push_str("| Rank | Strategy | Overlay | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Exposure | Stress Ret % | RiskOn Ret % | Neutral Ret % |\n");
        markdown.push_str("|------|----------|---------|---------:|-------:|--------:|-------:|------:|-------------:|-------------:|-------------:|--------------:|\n");
        for (idx, row) in universe.rows.iter().enumerate() {
            markdown.push_str(&format!(
                "| {} | {} | {} | {:.1} | {:.2} | {:.1} | {} | {:.1} | {:.2} | {:.1} | {:.1} | {:.1} |\n",
                idx + 1,
                row.strategy.name(),
                row.overlay.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_exposure,
                row.stats.state_return_pct.get(&MacroStateBucket::Stress).copied().unwrap_or(0.0),
                row.stats.state_return_pct.get(&MacroStateBucket::RiskOn).copied().unwrap_or(0.0),
                row.stats.state_return_pct.get(&MacroStateBucket::Neutral).copied().unwrap_or(0.0),
            ));
        }
        markdown.push('\n');
    }

    let mut csv = String::from("universe,rank,strategy,overlay,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_active_positions,idle_days_pct,avg_exposure,stress_return_pct,riskon_return_pct,neutral_return_pct\n");
    for universe in universes {
        for (idx, row) in universe.rows.iter().enumerate() {
            csv.push_str(&format!(
                "{},{},{},{},{:.4},{:.4},{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                universe.label,
                idx + 1,
                row.strategy.name(),
                row.overlay.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
                row.stats.avg_exposure,
                row.stats
                    .state_return_pct
                    .get(&MacroStateBucket::Stress)
                    .copied()
                    .unwrap_or(0.0),
                row.stats
                    .state_return_pct
                    .get(&MacroStateBucket::RiskOn)
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

    fs::write(SNAPSHOT_LATEST_MD, &markdown)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, markdown)?;
    fs::write(&archive_csv, csv)?;
    Ok((archive_md, archive_csv))
}

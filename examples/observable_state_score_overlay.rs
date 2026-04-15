//! Observable state score overlay for the frozen three-sleeve reference book.
//!
//! Purpose:
//! - answer the still-open PLAN item directly: build an explicit observable state score instead of
//!   drifting into another hidden-state / sleeve-weight tuning loop
//! - combine a few pre-declared, observable context inputs we already trust enough to measure:
//!   breadth, BTC vol stress, and internal connectedness
//! - judge the state score only as allocator context on top of the frozen
//!   `DDBudget(A/D,MACD,Small)` reference book
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry next open
//! - exit after fixed 21-bar hold at open
//! - 0.1% taker each side
//! - top-3 strength-capped book within each sleeve
//! - DDHard-style family-level exposure budgeting only
//!
//! Honesty note:
//! - this is a hand-built state map on cached daily Binance history
//! - it is a trust / allocation audit, not a promoted live allocator

use anyhow::{Context, Result};
use chrono::Utc;
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::{collections::HashMap, fs};

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
const POSITION_CAP: usize = 3;
const WARMUP_BARS: usize = 200;
const CS_LOOKBACK: usize = 63;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const CONN_LOOKBACK: usize = 63;
const VOL_LOOKBACK: usize = 21;
const BREADTH_LOOKBACK: usize = 63;
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/observable_state_score_overlay_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/observable_state_score_overlay_latest.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    AdMomentum,
    MacdRegime,
    SmallByDollarVol,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[Self::AdMomentum, Self::MacdRegime, Self::SmallByDollarVol]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::AdMomentum => "A/D Momentum",
            Self::MacdRegime => "MACD+Regime",
            Self::SmallByDollarVol => "FactorSmallByDollarVol",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PortfolioKind {
    DdBudgetBaseline,
    ObservableTilt,
    ObservableRiskOff,
}

impl PortfolioKind {
    fn all() -> &'static [PortfolioKind] {
        &[
            Self::DdBudgetBaseline,
            Self::ObservableTilt,
            Self::ObservableRiskOff,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::DdBudgetBaseline => "DDBudget(A/D,MACD,Small)",
            Self::ObservableTilt => "ObsTilt(A/D,MACD,Small)",
            Self::ObservableRiskOff => "ObsRiskOff(A/D,MACD,Small)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ObservableState {
    Healthy,
    Neutral,
    Stressed,
}

impl ObservableState {
    fn name(&self) -> &'static str {
        match self {
            Self::Healthy => "Healthy",
            Self::Neutral => "Neutral",
            Self::Stressed => "Stressed",
        }
    }
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
    stressed_days_pct: f64,
}

#[derive(Clone, Debug)]
struct ResultRow {
    portfolio: PortfolioKind,
    stats: PortfolioStats,
}

#[derive(Clone, Debug)]
struct UniverseSummary {
    label: String,
    rows: Vec<ResultRow>,
    state_counts: [usize; 3],
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

#[derive(Clone, Debug)]
struct FamilySleeve {
    strategy: StrategyKind,
    plans: Vec<SymbolPlan>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== OBSERVABLE STATE SCORE OVERLAY ===\n");
    println!("Question: can a simple observable state score improve the frozen three-sleeve book without another local sleeve loop?");
    println!("Baseline: DDBudget(A/D,MACD,Small) as the current risk-adjusted reference book.");
    println!(
        "State score inputs: breadth participation, BTC vol stress, and internal connectedness."
    );
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Portfolio lens: top-{} capped book inside each sleeve, then sleeve-level capital budgeting", POSITION_CAP);
    println!(
        "Honesty note: this is a hand-built observable state map, not a hidden-state model.\n"
    );

    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;

    let mut data_cache = HashMap::<String, DataFrame>::new();
    data_cache.insert(BENCHMARK.to_string(), bench_df.clone());
    for &symbol in LOAD_SYMBOLS.iter().filter(|&&s| s != BENCHMARK) {
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, Some(&bench_df))?;
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

    let bench_close = series_to_vec(bench_df.column("close")?.f64()?);
    let mut universe_summaries = Vec::new();
    for &(label, universe_symbols) in UNIVERSES {
        println!(
            "\n--- Universe: {} ({}) ---",
            label,
            universe_symbols.join(", ")
        );
        let universe = aligned_universe(&data_cache, universe_symbols)?;
        let small_signal_map = generate_small_by_dollar_volume_signal_strengths(&universe)?;
        let sleeves = StrategyKind::all()
            .iter()
            .map(|&strategy| {
                let plans = build_symbol_plans(&universe, strategy, &small_signal_map)?;
                Ok(FamilySleeve { strategy, plans })
            })
            .collect::<Result<Vec<_>>>()?;
        let state = build_observable_state(&universe, &bench_close, &sleeves)?;

        let mut rows = Vec::new();
        for &portfolio in PortfolioKind::all() {
            let stats = simulate_family_portfolio(&sleeves, universe.steps, &state, portfolio);
            rows.push(ResultRow { portfolio, stats });
        }

        rows.sort_by(|a, b| {
            a.stats
                .max_dd_pct
                .partial_cmp(&b.stats.max_dd_pct)
                .unwrap()
                .then_with(|| b.stats.sharpe.partial_cmp(&a.stats.sharpe).unwrap())
                .then_with(|| {
                    b.stats
                        .aligned_return_pct
                        .partial_cmp(&a.stats.aligned_return_pct)
                        .unwrap()
                })
        });

        let mut state_counts = [0usize; 3];
        for s in &state {
            match s {
                ObservableState::Healthy => state_counts[0] += 1,
                ObservableState::Neutral => state_counts[1] += 1,
                ObservableState::Stressed => state_counts[2] += 1,
            }
        }

        println!(
            "State mix: Healthy {:>5.1}% | Neutral {:>5.1}% | Stressed {:>5.1}%",
            state_counts[0] as f64 / state.len() as f64 * 100.0,
            state_counts[1] as f64 / state.len() as f64 * 100.0,
            state_counts[2] as f64 / state.len() as f64 * 100.0,
        );
        println!(
            "{:<28} {:>10} {:>8} {:>8} {:>7} {:>7} {:>9}",
            "Portfolio", "Ret%", "Sharpe", "MaxDD", "Trades", "Exp", "Stress"
        );
        println!("{}", "-".repeat(92));
        for row in &rows {
            println!(
                "{:<28} {:>9.1} {:>8.2} {:>7.1} {:>7} {:>6.2} {:>7.1}%",
                row.portfolio.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.avg_exposure,
                row.stats.stressed_days_pct,
            );
        }

        universe_summaries.push(UniverseSummary {
            label: label.to_string(),
            rows,
            state_counts,
        });
    }

    let (archive_md, archive_csv) = write_snapshot(&universe_summaries)?;
    println!("\nSnapshots written:");
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);
    println!("- {}", archive_md);
    println!("- {}", archive_csv);
    Ok(())
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

fn generate_small_by_dollar_volume_signal_strengths(
    universe: &UniverseData,
) -> Result<HashMap<String, (Vec<i32>, Vec<f64>)>> {
    let n = universe
        .data
        .iter()
        .map(|(_, df)| df.height())
        .min()
        .unwrap_or(0);
    let mut raw_scores = HashMap::<String, Vec<f64>>::new();
    for (symbol, df) in &universe.data {
        let close = df.column("close")?.f64()?;
        let volume = df.column("volume")?.f64()?;
        let dollar_volume = rolling_dollar_volume(close, volume, 63);
        let mut scores = vec![0.0; n];
        for i in 63..n {
            let dv = dollar_volume[i].max(1.0);
            scores[i] = -dv.ln();
        }
        raw_scores.insert(symbol.clone(), scores);
    }

    let symbols = universe
        .data
        .iter()
        .map(|(s, _)| s.clone())
        .collect::<Vec<_>>();
    let mut out = HashMap::<String, (Vec<i32>, Vec<f64>)>::new();
    for symbol in &symbols {
        out.insert(symbol.clone(), (vec![0i32; n], vec![0.0; n]));
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
            let signal = if z > 0.35 {
                1
            } else if z < -0.35 {
                -1
            } else {
                0
            };
            let strength = z.abs();
            let slot = out.get_mut(symbol).unwrap();
            slot.0[i] = signal;
            slot.1[i] = strength;
        }
    }
    Ok(out)
}

fn build_symbol_plans(
    universe: &UniverseData,
    strategy: StrategyKind,
    small_signal_map: &HashMap<String, (Vec<i32>, Vec<f64>)>,
) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(symbol, df)| build_symbol_plan(df, symbol, strategy, small_signal_map))
        .collect()
}

fn build_symbol_plan(
    df: &DataFrame,
    symbol: &str,
    strategy: StrategyKind,
    small_signal_map: &HashMap<String, (Vec<i32>, Vec<f64>)>,
) -> Result<SymbolPlan> {
    let (signals, strengths) = match strategy {
        StrategyKind::AdMomentum => (
            generate_ad_momentum_signals(df, AD_PERIOD)?,
            generate_ad_strengths(df, AD_PERIOD)?,
        ),
        StrategyKind::MacdRegime => (
            generate_macd_regime_signals(df)?,
            generate_macd_strengths(df)?,
        ),
        StrategyKind::SmallByDollarVol => small_signal_map
            .get(symbol)
            .cloned()
            .context("missing small sleeve signal map")?,
    };

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

fn build_observable_state(
    universe: &UniverseData,
    bench_close: &[f64],
    sleeves: &[FamilySleeve],
) -> Result<Vec<ObservableState>> {
    let steps = universe.steps;
    let connectedness = build_connectedness_metric(universe)?;
    let btc_vol = rolling_realized_vol(bench_close, VOL_LOOKBACK, steps);
    let breadth = sleeve_breadth_metric(sleeves, steps);

    let conn_z = zscore_series(&connectedness, CONN_LOOKBACK);
    let vol_z = zscore_series(&btc_vol, VOL_LOOKBACK);
    let breadth_z = zscore_series(&breadth, BREADTH_LOOKBACK);

    let mut score = vec![0.0; steps + 1];
    for day in 0..=steps {
        score[day] = 0.45 * conn_z[day] + 0.35 * vol_z[day] - 0.45 * breadth_z[day];
    }

    let valid = score.iter().copied().skip(WARMUP_BARS).collect::<Vec<_>>();
    let mean = valid.iter().sum::<f64>() / valid.len().max(1) as f64;
    let var = valid.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / valid.len().max(1) as f64;
    let std = var.max(1e-12).sqrt();
    let stressed_cut = mean + 0.40 * std;
    let healthy_cut = mean - 0.40 * std;

    let mut states = vec![ObservableState::Neutral; steps + 1];
    for day in 0..=steps {
        states[day] = if day < WARMUP_BARS {
            ObservableState::Neutral
        } else if score[day] >= stressed_cut {
            ObservableState::Stressed
        } else if score[day] <= healthy_cut {
            ObservableState::Healthy
        } else {
            ObservableState::Neutral
        };
    }
    Ok(states)
}

fn build_connectedness_metric(universe: &UniverseData) -> Result<Vec<f64>> {
    let closes = universe
        .data
        .iter()
        .map(|(_, df)| Ok(series_to_vec(df.column("close")?.f64()?)))
        .collect::<Result<Vec<_>>>()?;
    let steps = universe.steps;
    let mut mean_corr = vec![0.0; steps + 1];

    for day in CONN_LOOKBACK..=steps {
        let start = day.saturating_sub(CONN_LOOKBACK);
        let end = day;
        let mut sum = 0.0;
        let mut pairs = 0usize;
        for i in 0..closes.len() {
            let ra = returns_slice(&closes[i], start, end);
            for j in (i + 1)..closes.len() {
                let rb = returns_slice(&closes[j], start, end);
                let corr = correlation(&ra, &rb);
                if corr.is_finite() {
                    sum += corr;
                    pairs += 1;
                }
            }
        }
        mean_corr[day] = if pairs > 0 { sum / pairs as f64 } else { 0.0 };
    }
    Ok(mean_corr)
}

fn sleeve_breadth_metric(sleeves: &[FamilySleeve], steps: usize) -> Vec<f64> {
    let mut out = vec![0.0; steps + 1];
    for day in 0..=steps {
        let mut active = 0usize;
        let mut total = 0usize;
        for sleeve in sleeves {
            total += 1;
            let (_, cnt) = sleeve_day_return(&sleeve.plans, day.min(steps.saturating_sub(1)));
            if cnt > 0 {
                active += 1;
            }
        }
        out[day] = if total > 0 {
            active as f64 / total as f64
        } else {
            0.0
        };
    }
    out
}

fn rolling_realized_vol(close: &[f64], lookback: usize, steps: usize) -> Vec<f64> {
    let rets = returns_slice(close, 0, close.len().saturating_sub(1));
    let mut out = vec![0.0; steps + 1];
    for day in lookback..=steps.min(rets.len()) {
        let start = day.saturating_sub(lookback);
        let slice = &rets[start..day];
        let mean = slice.iter().sum::<f64>() / slice.len().max(1) as f64;
        let var = slice.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / slice.len().max(1) as f64;
        out[day] = var.sqrt() * (252.0_f64).sqrt();
    }
    out
}

fn zscore_series(values: &[f64], lookback: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in 0..values.len() {
        if i < lookback {
            continue;
        }
        let slice = &values[i - lookback..i];
        let mean = slice.iter().sum::<f64>() / slice.len().max(1) as f64;
        let var = slice.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / slice.len().max(1) as f64;
        let std = var.max(1e-12).sqrt();
        out[i] = (values[i] - mean) / std;
    }
    out
}

fn simulate_family_portfolio(
    sleeves: &[FamilySleeve],
    steps: usize,
    state: &[ObservableState],
    portfolio: PortfolioKind,
) -> PortfolioStats {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut exposure_sum = 0.0;
    let mut stressed_days = 0usize;

    let mut sleeve_equity = HashMap::<StrategyKind, f64>::new();
    let mut sleeve_peak = HashMap::<StrategyKind, f64>::new();
    let mut trade_count = 0usize;
    let mut wins = 0usize;
    for sleeve in sleeves {
        sleeve_equity.insert(sleeve.strategy, 1.0);
        sleeve_peak.insert(sleeve.strategy, 1.0);
        for plan in &sleeve.plans {
            for trade in &plan.trades {
                trade_count += 1;
                if trade.net_return > 0.0 {
                    wins += 1;
                }
            }
        }
    }

    for day in 0..steps {
        let current_equity = equity_curve[day];
        let mut sleeve_daily = Vec::<(StrategyKind, f64, usize, f64)>::new();
        let mut total_active = 0usize;
        let obs_state = state.get(day).copied().unwrap_or(ObservableState::Neutral);
        if obs_state == ObservableState::Stressed {
            stressed_days += 1;
        }

        for sleeve in sleeves {
            let (base_ret, active_count) = sleeve_day_return(&sleeve.plans, day);
            total_active += active_count;
            let eq = *sleeve_equity.get(&sleeve.strategy).unwrap_or(&1.0);
            let pk = *sleeve_peak.get(&sleeve.strategy).unwrap_or(&1.0);
            let dd = if pk > 0.0 {
                (1.0 - eq / pk) * 100.0
            } else {
                0.0
            };
            let rec = if pk > 0.0 { eq / pk } else { 1.0 };
            let weight = sleeve_weight(portfolio, sleeve.strategy, dd, rec, obs_state);
            sleeve_daily.push((sleeve.strategy, base_ret, active_count, weight));
        }

        active_counts[day] = total_active;
        let weight_sum = sleeve_daily.iter().map(|(_, _, _, w)| *w).sum::<f64>();
        exposure_sum += weight_sum;
        if sleeve_daily.is_empty() || weight_sum <= 0.0 {
            daily_returns[day] = 0.0;
            equity_curve[day + 1] = current_equity;
            continue;
        }

        let mut portfolio_ret = 0.0;
        for (strategy, base_ret, _, weight) in sleeve_daily {
            portfolio_ret += base_ret * weight;
            let prev_eq = *sleeve_equity.get(&strategy).unwrap_or(&1.0);
            let new_eq = prev_eq * (1.0 + base_ret * weight);
            let pk = sleeve_peak.entry(strategy).or_insert(1.0);
            *pk = pk.max(new_eq);
            sleeve_equity.insert(strategy, new_eq);
        }

        daily_returns[day] = portfolio_ret;
        equity_curve[day + 1] = current_equity * (1.0 + portfolio_ret);
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
        stressed_days_pct: stressed_days as f64 / steps.max(1) as f64 * 100.0,
    }
}

fn sleeve_day_return(plans: &[SymbolPlan], day: usize) -> (f64, usize) {
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
    let active_count = active.len();
    if active.is_empty() {
        return (0.0, 0);
    }
    active.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let selected = &active[..POSITION_CAP.min(active.len())];
    let avg_ret = selected.iter().map(|(_, r)| *r).sum::<f64>() / selected.len() as f64;
    (avg_ret, active_count)
}

fn sleeve_weight(
    portfolio: PortfolioKind,
    strategy: StrategyKind,
    dd_pct: f64,
    recovery_ratio: f64,
    obs_state: ObservableState,
) -> f64 {
    let base = (1.0 / 3.0) * ddhard_exposure(dd_pct, recovery_ratio);
    match portfolio {
        PortfolioKind::DdBudgetBaseline => base,
        PortfolioKind::ObservableTilt => match (strategy, obs_state) {
            (StrategyKind::AdMomentum, ObservableState::Stressed) => {
                0.42 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            (StrategyKind::MacdRegime, ObservableState::Stressed) => {
                0.20 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            (StrategyKind::SmallByDollarVol, ObservableState::Stressed) => {
                0.28 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            (StrategyKind::AdMomentum, ObservableState::Healthy) => {
                0.28 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            (StrategyKind::MacdRegime, ObservableState::Healthy) => {
                0.38 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            (StrategyKind::SmallByDollarVol, ObservableState::Healthy) => {
                0.34 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            _ => base,
        },
        PortfolioKind::ObservableRiskOff => match (strategy, obs_state) {
            (StrategyKind::AdMomentum, ObservableState::Stressed) => {
                0.30 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            (StrategyKind::MacdRegime, ObservableState::Stressed) => {
                0.12 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            (StrategyKind::SmallByDollarVol, ObservableState::Stressed) => {
                0.18 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            (StrategyKind::AdMomentum, ObservableState::Healthy) => {
                0.30 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            (StrategyKind::MacdRegime, ObservableState::Healthy) => {
                0.40 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            (StrategyKind::SmallByDollarVol, ObservableState::Healthy) => {
                0.30 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            _ => base,
        },
    }
}

fn ddhard_exposure(dd_pct: f64, recovery_ratio: f64) -> f64 {
    if dd_pct >= 20.0 && recovery_ratio < 0.95 {
        0.30
    } else if dd_pct >= 10.0 && recovery_ratio < 0.98 {
        0.60
    } else {
        1.0
    }
}

fn generate_ad_momentum_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let ad = compute_ad_line(df)?;
    let n = ad.len();
    let mut signals = vec![0; n];
    for i in period..n {
        let current = ad[i];
        let past = ad[i - period];
        if !current.is_finite() || !past.is_finite() || past.abs() < 1e-9 {
            continue;
        }
        let momentum = current / past - 1.0;
        if momentum > 0.05 {
            signals[i] = 1;
        } else if momentum < -0.05 {
            signals[i] = -1;
        }
    }
    Ok(signals)
}

fn generate_ad_strengths(df: &DataFrame, period: usize) -> Result<Vec<f64>> {
    let ad = compute_ad_line(df)?;
    let n = ad.len();
    let mut strengths = vec![0.0; n];
    for i in period..n {
        let current = ad[i];
        let past = ad[i - period];
        if !current.is_finite() || !past.is_finite() || past.abs() < 1e-9 {
            continue;
        }
        strengths[i] = (current / past - 1.0).abs();
    }
    Ok(strengths)
}

fn compute_ad_line(df: &DataFrame) -> Result<Vec<f64>> {
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let close = df.column("close")?.f64()?;
    let volume = df.column("volume")?.f64()?;
    let n = df.height();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let c = close.get(i).unwrap_or(0.0);
        let v = volume.get(i).unwrap_or(0.0);
        let range = (h - l).abs();
        let mfm = if range > 1e-12 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        let mfv = mfm * v;
        ad[i] = if i == 0 { mfv } else { ad[i - 1] + mfv };
    }
    Ok(ad)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd_line = series_to_vec(df.column("macd")?.f64()?);
    let signal_line = series_to_vec(df.column("macd_signal")?.f64()?);
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let n = df.height();
    let mut signals = vec![0; n];

    for i in 200..n {
        let bull = close.get(i).unwrap_or(0.0) > sma_200[i];
        let bear = close.get(i).unwrap_or(0.0) < sma_200[i];
        if macd_line[i] > signal_line[i] && bull {
            signals[i] = 1;
        } else if macd_line[i] < signal_line[i] && bear {
            signals[i] = -1;
        }
    }
    Ok(signals)
}

fn generate_macd_strengths(df: &DataFrame) -> Result<Vec<f64>> {
    let macd_line = series_to_vec(df.column("macd")?.f64()?);
    let signal_line = series_to_vec(df.column("macd_signal")?.f64()?);
    let hist = df
        .column("macd_histogram")
        .ok()
        .and_then(|s| s.f64().ok())
        .map(series_to_vec);
    let n = df.height();
    let mut out = vec![0.0; n];
    for i in 0..n {
        let spread = (macd_line[i] - signal_line[i]).abs();
        let h = hist.as_ref().map(|v| v[i].abs()).unwrap_or(0.0);
        out[i] = spread + 0.5 * h;
    }
    Ok(out)
}

fn rolling_dollar_volume(
    close: &Float64Chunked,
    volume: &Float64Chunked,
    lookback: usize,
) -> Vec<f64> {
    let close_v = series_to_vec(close);
    let volume_v = series_to_vec(volume);
    let mut out = vec![0.0; close_v.len()];
    let mut sum = 0.0;
    for i in 0..close_v.len() {
        let dv = close_v[i].max(0.0) * volume_v[i].max(0.0);
        sum += dv;
        if i >= lookback {
            let old = close_v[i - lookback].max(0.0) * volume_v[i - lookback].max(0.0);
            sum -= old;
        }
        if i + 1 >= lookback {
            out[i] = sum / lookback as f64;
        }
    }
    out
}

fn calculate_sma(series: &Float64Chunked, period: usize) -> Vec<f64> {
    let values = series_to_vec(series);
    let mut out = vec![f64::NAN; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i >= period {
            sum -= values[i - period];
        }
        if i + 1 >= period {
            out[i] = sum / period as f64;
        }
    }
    out
}

fn series_to_vec(series: &Float64Chunked) -> Vec<f64> {
    (0..series.len())
        .map(|i| series.get(i).unwrap_or(f64::NAN))
        .collect()
}

fn returns_slice(close: &[f64], start: usize, end: usize) -> Vec<f64> {
    let mut out = Vec::new();
    let capped_end = end.min(close.len().saturating_sub(1));
    for i in (start + 1)..=capped_end {
        let prev = close[i - 1];
        let curr = close[i];
        if prev.is_finite() && curr.is_finite() && prev > 0.0 {
            out.push(curr / prev - 1.0);
        }
    }
    out
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n < 3 {
        return 0.0;
    }
    let mean_a = a.iter().take(n).sum::<f64>() / n as f64;
    let mean_b = b.iter().take(n).sum::<f64>() / n as f64;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for i in 0..n {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    if var_a <= 1e-12 || var_b <= 1e-12 {
        0.0
    } else {
        cov / (var_a.sqrt() * var_b.sqrt())
    }
}

fn calc_sharpe_from_returns(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    if var <= 1e-12 {
        0.0
    } else {
        mean / var.sqrt() * (252.0_f64).sqrt()
    }
}

fn calc_max_drawdown_pct(equity: &[f64]) -> f64 {
    let mut peak = equity.first().copied().unwrap_or(1.0_f64);
    let mut max_dd = 0.0_f64;
    for &v in equity {
        peak = peak.max(v);
        if peak > 0.0 {
            max_dd = max_dd.max((1.0 - v / peak) * 100.0);
        }
    }
    max_dd
}

fn write_snapshot(summaries: &[UniverseSummary]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let ts = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let archive_md = format!("{}/observable_state_score_overlay_{}.md", SNAPSHOT_DIR, ts);
    let archive_csv = format!("{}/observable_state_score_overlay_{}.csv", SNAPSHOT_DIR, ts);

    let mut md = String::new();
    md.push_str("# Observable state score overlay\n\n");
    md.push_str("Compare the frozen DDBudget(A/D,MACD,Small) book against two observable-state allocator tilts.\n\n");
    for summary in summaries {
        let total = summary.state_counts.iter().sum::<usize>().max(1) as f64;
        md.push_str(&format!("## {}\n\n", summary.label));
        md.push_str(&format!(
            "State mix: Healthy {:.1}% | Neutral {:.1}% | Stressed {:.1}%\n\n",
            summary.state_counts[0] as f64 / total * 100.0,
            summary.state_counts[1] as f64 / total * 100.0,
            summary.state_counts[2] as f64 / total * 100.0,
        ));
        md.push_str("| Portfolio | Return % | Sharpe | MaxDD % | Trades | Avg Exposure | Stressed Days % |\n");
        md.push_str("|---|---:|---:|---:|---:|---:|---:|\n");
        for row in &summary.rows {
            md.push_str(&format!(
                "| {} | {:.1} | {:.2} | {:.1} | {} | {:.2} | {:.1} |\n",
                row.portfolio.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.avg_exposure,
                row.stats.stressed_days_pct,
            ));
        }
        md.push('\n');
    }

    let mut csv = String::from("universe,portfolio,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_active_positions,idle_days_pct,avg_exposure,stressed_days_pct\n");
    for summary in summaries {
        for row in &summary.rows {
            csv.push_str(&format!(
                "{},{},{:.4},{:.4},{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                summary.label,
                row.portfolio.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
                row.stats.avg_exposure,
                row.stats.stressed_days_pct,
            ));
        }
    }

    fs::write(SNAPSHOT_LATEST_MD, &md)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, md)?;
    fs::write(&archive_csv, csv)?;
    Ok((archive_md, archive_csv))
}

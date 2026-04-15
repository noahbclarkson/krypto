//! Separate-sleeve audit for FactorSmallByDollarVol against the current A/D + MACD baseline.
//!
//! Purpose:
//! - follow up the factor-sleeve benchmark with the honest next object: a separate sleeve,
//!   not another merged-factor blend
//! - test whether the inverse dollar-volume proxy adds anything as its own family under the
//!   current realistic portfolio lens
//! - keep the comparison pre-declared and small to avoid another local tuning loop
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
//! - "small" here means inverse rolling dollar volume, not true historical market cap
//! - this is a sleeve-construction audit, not a promotion result

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
const HOLD_BARS: usize = 49;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const WARMUP_BARS: usize = 200;
const CS_LOOKBACK: usize = 15; // Updated via hyperopt 2026-04-07
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47) // hyperopt 2026-04-06: sweep shows Sharpe 2.43->3.06 on Base5, tied with 42-bar
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/factor_small_sleeve_overlay_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/factor_small_sleeve_overlay_latest.csv";

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
    AdOnly,
    MacdOnly,
    SmallOnly,
    EqualAdMacd,
    EqualAdSmall,
    EqualMacdSmall,
    DdBudgetAdMacd,
    DdBudgetAdSmall,
    DdBudgetMacdSmall,
    DdBudgetAllThree,
}

impl PortfolioKind {
    fn all() -> &'static [PortfolioKind] {
        &[
            Self::AdOnly,
            Self::MacdOnly,
            Self::SmallOnly,
            Self::EqualAdMacd,
            Self::EqualAdSmall,
            Self::EqualMacdSmall,
            Self::DdBudgetAdMacd,
            Self::DdBudgetAdSmall,
            Self::DdBudgetMacdSmall,
            Self::DdBudgetAllThree,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::AdOnly => "A/D only",
            Self::MacdOnly => "MACD+Regime only",
            Self::SmallOnly => "SmallByDollarVol only",
            Self::EqualAdMacd => "Equal(A/D,MACD)",
            Self::EqualAdSmall => "Equal(A/D,Small)",
            Self::EqualMacdSmall => "Equal(MACD,Small)",
            Self::DdBudgetAdMacd => "DDBudget(A/D,MACD)",
            Self::DdBudgetAdSmall => "DDBudget(A/D,Small)",
            Self::DdBudgetMacdSmall => "DDBudget(MACD,Small)",
            Self::DdBudgetAllThree => "DDBudget(A/D,MACD,Small)",
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
    dd20_days_pct: f64,
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
    println!("=== FACTOR SMALL SLEEVE OVERLAY ===\n");
    println!("Question: does FactorSmallByDollarVol add anything as a separate sleeve vs the frozen A/D + MACD baseline?");
    println!("Allocators: standalone sleeves, equal-weight sleeve pairs, and DDHard-style family budgets.");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Portfolio lens: top-{} capped book inside each sleeve, then sleeve-level capital budgeting", POSITION_CAP);
    println!(
        "Honesty note: SmallByDollarVol = inverse dollar-volume proxy, not true market cap.\n"
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

        let mut rows = Vec::new();
        for &portfolio in PortfolioKind::all() {
            let stats = simulate_family_portfolio(&sleeves, universe.steps, portfolio);
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

        println!(
            "{:<24} {:>10} {:>8} {:>8} {:>7} {:>7} {:>7}",
            "Portfolio", "Ret%", "Sharpe", "MaxDD", "Trades", "Exp", ">20DD"
        );
        println!("{}", "-".repeat(82));
        for row in &rows {
            println!(
                "{:<24} {:>9.1} {:>8.2} {:>7.1} {:>7} {:>6.2} {:>6.1}%",
                row.portfolio.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.avg_exposure,
                row.stats.dd20_days_pct,
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

fn simulate_family_portfolio(
    sleeves: &[FamilySleeve],
    steps: usize,
    portfolio: PortfolioKind,
) -> PortfolioStats {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut exposure_sum = 0.0;
    let mut dd20_days = 0usize;

    let included = included_strategies(portfolio);
    let mut sleeve_equity = HashMap::<StrategyKind, f64>::new();
    let mut sleeve_peak = HashMap::<StrategyKind, f64>::new();
    let mut trade_count = 0usize;
    let mut wins = 0usize;
    for sleeve in sleeves {
        if included.contains(&sleeve.strategy) {
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
    }

    let mut global_peak = 1.0_f64;
    for day in 0..steps {
        let current_equity = equity_curve[day];
        global_peak = global_peak.max(current_equity);
        let global_dd = (1.0 - current_equity / global_peak) * 100.0;
        let global_recovery = current_equity / global_peak;
        if global_dd >= 20.0 {
            dd20_days += 1;
        }

        let mut sleeve_daily = Vec::<(StrategyKind, f64, usize, f64)>::new();
        let mut total_active = 0usize;
        for sleeve in sleeves {
            if !included.contains(&sleeve.strategy) {
                continue;
            }
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
            let weight = sleeve_weight(
                portfolio,
                sleeve.strategy,
                dd,
                rec,
                global_dd,
                global_recovery,
            );
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
        dd20_days_pct: dd20_days as f64 / steps.max(1) as f64 * 100.0,
    }
}

fn included_strategies(portfolio: PortfolioKind) -> &'static [StrategyKind] {
    match portfolio {
        PortfolioKind::AdOnly => &[StrategyKind::AdMomentum],
        PortfolioKind::MacdOnly => &[StrategyKind::MacdRegime],
        PortfolioKind::SmallOnly => &[StrategyKind::SmallByDollarVol],
        PortfolioKind::EqualAdMacd | PortfolioKind::DdBudgetAdMacd => {
            &[StrategyKind::AdMomentum, StrategyKind::MacdRegime]
        }
        PortfolioKind::EqualAdSmall | PortfolioKind::DdBudgetAdSmall => {
            &[StrategyKind::AdMomentum, StrategyKind::SmallByDollarVol]
        }
        PortfolioKind::EqualMacdSmall | PortfolioKind::DdBudgetMacdSmall => {
            &[StrategyKind::MacdRegime, StrategyKind::SmallByDollarVol]
        }
        PortfolioKind::DdBudgetAllThree => &[
            StrategyKind::AdMomentum,
            StrategyKind::MacdRegime,
            StrategyKind::SmallByDollarVol,
        ],
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
    _global_dd: f64,
    _global_recovery: f64,
) -> f64 {
    match portfolio {
        PortfolioKind::AdOnly if strategy == StrategyKind::AdMomentum => {
            ddhard_exposure(dd_pct, recovery_ratio)
        }
        PortfolioKind::MacdOnly if strategy == StrategyKind::MacdRegime => {
            ddhard_exposure(dd_pct, recovery_ratio)
        }
        PortfolioKind::SmallOnly if strategy == StrategyKind::SmallByDollarVol => {
            ddhard_exposure(dd_pct, recovery_ratio)
        }
        PortfolioKind::EqualAdMacd
            if matches!(
                strategy,
                StrategyKind::AdMomentum | StrategyKind::MacdRegime
            ) =>
        {
            0.5
        }
        PortfolioKind::EqualAdSmall
            if matches!(
                strategy,
                StrategyKind::AdMomentum | StrategyKind::SmallByDollarVol
            ) =>
        {
            0.5
        }
        PortfolioKind::EqualMacdSmall
            if matches!(
                strategy,
                StrategyKind::MacdRegime | StrategyKind::SmallByDollarVol
            ) =>
        {
            0.5
        }
        PortfolioKind::DdBudgetAdMacd => match strategy {
            StrategyKind::AdMomentum | StrategyKind::MacdRegime => {
                0.50 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            _ => 0.0,
        },
        PortfolioKind::DdBudgetAdSmall => match strategy {
            StrategyKind::AdMomentum | StrategyKind::SmallByDollarVol => {
                0.50 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            _ => 0.0,
        },
        PortfolioKind::DdBudgetMacdSmall => match strategy {
            StrategyKind::MacdRegime | StrategyKind::SmallByDollarVol => {
                0.50 * ddhard_exposure(dd_pct, recovery_ratio)
            }
            _ => 0.0,
        },
        PortfolioKind::DdBudgetAllThree => match strategy {
            StrategyKind::AdMomentum
            | StrategyKind::MacdRegime
            | StrategyKind::SmallByDollarVol => {
                (1.0 / 3.0) * ddhard_exposure(dd_pct, recovery_ratio)
            }
        },
        _ => 0.0,
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

fn generate_ad_strengths(df: &DataFrame, period: usize) -> Result<Vec<f64>> {
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

    let mut out = vec![0.0; df.height()];
    for i in period..df.height() {
        out[i] = (ad_line[i] - ad_line[i - period]).abs();
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

fn generate_macd_strengths(df: &DataFrame) -> Result<Vec<f64>> {
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    Ok((0..macd.len())
        .map(|i| (macd.get(i).unwrap_or(0.0) - signal.get(i).unwrap_or(0.0)).abs())
        .collect())
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
    let n = returns.len();
    if n < 2 {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / n as f64;
    let var = returns
        .iter()
        .map(|r| {
            let d = *r - mean;
            d * d
        })
        .sum::<f64>()
        / (n as f64 - 1.0);
    if var <= 1e-12 {
        return 0.0;
    }
    mean / var.sqrt() * (252.0_f64).sqrt()
}

fn calc_max_drawdown_pct(equity: &[f64]) -> f64 {
    let mut peak = equity.first().copied().unwrap_or(1.0);
    let mut max_dd = 0.0;
    for &v in equity {
        if v > peak {
            peak = v;
        }
        if peak > 0.0 {
            let dd = 1.0 - v / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd * 100.0
}

fn write_snapshot(universe_summaries: &[UniverseSummary]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let ts = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!("{}/factor_small_sleeve_overlay_{}.md", SNAPSHOT_DIR, ts);
    let archive_csv = format!("{}/factor_small_sleeve_overlay_{}.csv", SNAPSHOT_DIR, ts);

    let mut md = String::from("# Factor Small Sleeve Overlay\n\n");
    let mut csv = String::from("universe,portfolio,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_active_positions,idle_days_pct,avg_exposure,dd20_days_pct\n");

    for universe in universe_summaries {
        md.push_str(&format!("## {}\n\n", universe.label));
        md.push_str("| Portfolio | Return % | Sharpe | MaxDD % | Trades | Win Rate % | Avg Active | Idle % | Avg Exp | >20DD % |\n");
        md.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
        for row in &universe.rows {
            md.push_str(&format!(
                "| {} | {:.1} | {:.2} | {:.1} | {} | {:.1} | {:.2} | {:.1} | {:.2} | {:.1} |\n",
                row.portfolio.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
                row.stats.avg_exposure,
                row.stats.dd20_days_pct,
            ));
            csv.push_str(&format!(
                "{},{},{:.4},{:.4},{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                universe.label,
                row.portfolio.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
                row.stats.avg_exposure,
                row.stats.dd20_days_pct,
            ));
        }
        md.push('\n');
    }

    fs::write(SNAPSHOT_LATEST_MD, &md)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, md)?;
    fs::write(&archive_csv, csv)?;
    Ok((archive_md, archive_csv))
}

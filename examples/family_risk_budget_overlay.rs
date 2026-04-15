//! Portfolio-level family risk-budget overlay benchmark.
//!
//! Purpose:
//! - answer the higher-value follow-up after the failed simple blend test:
//!   if A/D is the least-overlapping survivor, does it help more as a separate sleeve
//!   inside a simple portfolio allocator than as a per-symbol conflict rule?
//! - keep the signal families separate and compare only a few pre-declared allocators
//! - avoid another local tuning loop; this is a portfolio-trust test, not a promotion table
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry next open
//! - exit after fixed 21-bar hold at open
//! - 0.1% taker each side
//! - top-3 strength-capped book within each sleeve
//! - DDHard-style family-level exposure budgeting only

use anyhow::Result;
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
const TURTLE_PERIOD: usize = 20;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47) // hyperopt: was 20, sweep shows Sharpe 2.43 -> 3.06 on Base5, tied with 42-bar
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/family_risk_budget_overlay_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/family_risk_budget_overlay_latest.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    AdMomentum,
    CTRend,
    MacdRegime,
    FactorSmall,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::AdMomentum,
            Self::CTRend,
            Self::MacdRegime,
            Self::FactorSmall,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::AdMomentum => "A/D Momentum",
            Self::CTRend => "CTREND",
            Self::MacdRegime => "MACD+Regime",
            Self::FactorSmall => "SmallByDollarVol",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PortfolioKind {
    AdOnly,
    CTRendOnly,
    MacdOnly,
    EqualAdCTRend,
    EqualAdMacd,
    DdBudgetAdCTRend,
    DdBudgetAdMacd,
    // Three-sleeve variants
    EqualAdMacdSmall,
    TailParityAdMacdSmall,
    TailParityAdMacdSmallCapped,
}

impl PortfolioKind {
    fn all() -> &'static [PortfolioKind] {
        &[
            Self::AdOnly,
            Self::CTRendOnly,
            Self::MacdOnly,
            Self::EqualAdCTRend,
            Self::EqualAdMacd,
            Self::DdBudgetAdCTRend,
            Self::DdBudgetAdMacd,
            Self::EqualAdMacdSmall,
            Self::TailParityAdMacdSmall,
            Self::TailParityAdMacdSmallCapped,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::AdOnly => "A/D only",
            Self::CTRendOnly => "CTREND only",
            Self::MacdOnly => "MACD+Regime only",
            Self::EqualAdCTRend => "Equal(A/D,CTREND)",
            Self::EqualAdMacd => "Equal(A/D,MACD+Regime)",
            Self::DdBudgetAdCTRend => "DDBudget(A/D,CTREND)",
            Self::DdBudgetAdMacd => "DDBudget(A/D,MACD+Regime)",
            Self::EqualAdMacdSmall => "Equal(A/D,MACD,Small)",
            Self::TailParityAdMacdSmall => "TailParity(A/D,MACD,Small)",
            Self::TailParityAdMacdSmallCapped => "TailParityCap(A/D,MACD,Small)",
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
    println!("=== FAMILY RISK-BUDGET OVERLAY ===\n");
    println!("Question: does A/D help more as a separate sleeve allocator than as a per-symbol blend rule?");
    println!("Allocators: standalone sleeves, equal-weight sleeves, and simple DDHard-style family budgets.");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Portfolio lens: top-{} capped book inside each sleeve, then sleeve-level capital budgeting\n", POSITION_CAP);

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
        let sleeves = StrategyKind::all()
            .iter()
            .map(|&strategy| {
                Ok(FamilySleeve {
                    strategy,
                    plans: build_symbol_plans(&universe, strategy)?,
                })
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

    println!("\nInterpretation:");
    println!("- If A/D really diversifies the trend basin, a separate sleeve allocator should look cleaner than the earlier blend rules.");
    println!(
        "- Equal-weight helps only if diversification is real enough to survive capital sharing."
    );
    println!("- DD-budget sleeve overlays help only if family-level throttling improves risk shape without becoming another blunt cash switch.");
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

fn build_symbol_plans(universe: &UniverseData, strategy: StrategyKind) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(symbol, df)| build_symbol_plan(df, symbol, strategy))
        .collect()
}

fn build_symbol_plan(df: &DataFrame, _symbol: &str, strategy: StrategyKind) -> Result<SymbolPlan> {
    let (signals, strengths) = signal_and_strength_for_strategy(df, strategy)?;
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

    // Tail-risk parity: rolling sleeve daily returns for CVaR computation
    const TAIL_WINDOW: usize = 30;
    const TAIL_FRAC: f64 = 0.10;
    let mut sleeve_rolling_rets: HashMap<StrategyKind, Vec<f64>> = HashMap::new();
    for &s in included {
        sleeve_rolling_rets.insert(s, Vec::with_capacity(steps));
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

            // Track rolling sleeve returns for tail-risk parity
            if let Some(rets_vec) = sleeve_rolling_rets.get_mut(&sleeve.strategy) {
                rets_vec.push(base_ret);
                if rets_vec.len() > TAIL_WINDOW {
                    rets_vec.remove(0);
                }
            }

            // Compute tail-parity weights if needed
            let weight = if matches!(
                portfolio,
                PortfolioKind::TailParityAdMacdSmall | PortfolioKind::TailParityAdMacdSmallCapped
            ) {
                tail_parity_weight(
                    portfolio,
                    sleeve.strategy,
                    &sleeve_rolling_rets,
                    dd,
                    rec,
                    global_dd,
                    global_recovery,
                )
            } else {
                sleeve_weight(
                    portfolio,
                    sleeve.strategy,
                    dd,
                    rec,
                    global_dd,
                    global_recovery,
                )
            };
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
        PortfolioKind::CTRendOnly => &[StrategyKind::CTRend],
        PortfolioKind::MacdOnly => &[StrategyKind::MacdRegime],
        PortfolioKind::EqualAdCTRend | PortfolioKind::DdBudgetAdCTRend => {
            &[StrategyKind::AdMomentum, StrategyKind::CTRend]
        }
        PortfolioKind::EqualAdMacd | PortfolioKind::DdBudgetAdMacd => {
            &[StrategyKind::AdMomentum, StrategyKind::MacdRegime]
        }
        PortfolioKind::EqualAdMacdSmall
        | PortfolioKind::TailParityAdMacdSmall
        | PortfolioKind::TailParityAdMacdSmallCapped => &[
            StrategyKind::AdMomentum,
            StrategyKind::MacdRegime,
            StrategyKind::FactorSmall,
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
        PortfolioKind::CTRendOnly if strategy == StrategyKind::CTRend => {
            ddhard_exposure(dd_pct, recovery_ratio)
        }
        PortfolioKind::MacdOnly if strategy == StrategyKind::MacdRegime => {
            ddhard_exposure(dd_pct, recovery_ratio)
        }
        PortfolioKind::EqualAdCTRend
            if matches!(strategy, StrategyKind::AdMomentum | StrategyKind::CTRend) =>
        {
            0.5
        }
        PortfolioKind::EqualAdMacd
            if matches!(
                strategy,
                StrategyKind::AdMomentum | StrategyKind::MacdRegime
            ) =>
        {
            0.5
        }
        PortfolioKind::DdBudgetAdCTRend => match strategy {
            StrategyKind::AdMomentum => 0.50 * ddhard_exposure(dd_pct, recovery_ratio),
            StrategyKind::CTRend => 0.50 * ddhard_exposure(dd_pct, recovery_ratio),
            _ => 0.0,
        },
        PortfolioKind::DdBudgetAdMacd => match strategy {
            StrategyKind::AdMomentum => 0.50 * ddhard_exposure(dd_pct, recovery_ratio),
            StrategyKind::MacdRegime => 0.50 * ddhard_exposure(dd_pct, recovery_ratio),
            _ => 0.0,
        },
        PortfolioKind::EqualAdMacdSmall => match strategy {
            StrategyKind::AdMomentum | StrategyKind::MacdRegime | StrategyKind::FactorSmall => {
                1.0 / 3.0
            }
            _ => 0.0,
        },
        _ => 0.0,
    }
}

/// Tail-risk parity weight computation using inverse CVaR weighting.
fn tail_parity_weight(
    portfolio: PortfolioKind,
    strategy: StrategyKind,
    sleeve_rolling_rets: &HashMap<StrategyKind, Vec<f64>>,
    _dd_pct: f64,
    _recovery_ratio: f64,
    _global_dd: f64,
    _global_recovery: f64,
) -> f64 {
    if !matches!(
        portfolio,
        PortfolioKind::TailParityAdMacdSmall | PortfolioKind::TailParityAdMacdSmallCapped
    ) {
        return 0.0;
    }
    if !matches!(
        strategy,
        StrategyKind::AdMomentum | StrategyKind::MacdRegime | StrategyKind::FactorSmall
    ) {
        return 0.0;
    }

    // Compute CVaR per sleeve from rolling returns
    const TAIL_FRAC: f64 = 0.10;

    let mut cvar_vec: Vec<(StrategyKind, f64)> = Vec::new();
    for (s, rets) in sleeve_rolling_rets {
        if rets.len() < 15 {
            return 1.0 / 3.0; // Not enough history yet
        }
        let tail_n = ((rets.len() as f64) * TAIL_FRAC).ceil() as usize;
        let tail_n = tail_n.max(1).min(rets.len().saturating_sub(1));
        let mut sorted = rets.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let tail_mean: f64 = sorted[..tail_n].iter().sum::<f64>() / tail_n as f64;
        let es = tail_mean.abs().max(1e-8);
        cvar_vec.push((*s, es));
    }

    // Inverse-CVaR weights
    let inv_sum: f64 = cvar_vec.iter().map(|(_, es)| 1.0 / es).sum::<f64>();
    let mut raw_weights: Vec<(StrategyKind, f64)> = cvar_vec
        .iter()
        .map(|(s, es)| (*s, (1.0 / es) / inv_sum))
        .collect();

    // Apply max-cap if specified (default 50% per sleeve)
    if matches!(portfolio, PortfolioKind::TailParityAdMacdSmallCapped) {
        const MAX_W: f64 = 0.50;
        let any_over = raw_weights.iter().any(|(_, w)| *w > MAX_W);
        if any_over {
            // Cap over-weight slots and re-normalize under-weight slots
            let sum_over: f64 = raw_weights
                .iter()
                .filter(|(_, w)| *w > MAX_W)
                .map(|(_, w)| *w)
                .sum();
            let sum_under: f64 = raw_weights
                .iter()
                .filter(|(_, w)| *w <= MAX_W)
                .map(|(_, w)| *w)
                .sum();

            let residual = 1.0 - sum_over;
            let scale = if sum_under > 1e-9 {
                residual / sum_under
            } else {
                1.0
            };

            for (s, w) in &mut raw_weights {
                if *w > MAX_W {
                    *w = MAX_W;
                } else {
                    *w *= scale;
                }
            }
        }
    }

    raw_weights
        .iter()
        .find(|(s, _)| *s == strategy)
        .map(|(_, w)| *w)
        .unwrap_or(1.0 / 3.0)
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

fn signal_and_strength_for_strategy(
    df: &DataFrame,
    strategy: StrategyKind,
) -> Result<(Vec<i32>, Vec<f64>)> {
    match strategy {
        StrategyKind::AdMomentum => Ok((
            generate_ad_momentum_signals(df, AD_PERIOD)?,
            generate_ad_strengths(df, AD_PERIOD)?,
        )),
        StrategyKind::CTRend => Ok(generate_ctrend_signal_strength(df)?),
        StrategyKind::MacdRegime => Ok((
            generate_macd_regime_signals(df)?,
            generate_macd_strengths(df)?,
        )),
        StrategyKind::FactorSmall => Ok(generate_small_signal_strength(df)?),
    }
}

fn generate_ctrend_signal_strength(df: &DataFrame) -> Result<(Vec<i32>, Vec<f64>)> {
    let close = df.column("close")?.f64()?;
    let volume = df.column("volume")?.f64()?;
    let n = df.height();

    let vol_sma_20 = calculate_sma(&volume, 20);
    let vol_sma_63 = calculate_sma(&volume, 63);
    let ret_5 = rolling_return(&close, 5);
    let ret_21 = rolling_return(&close, 21);
    let ret_63 = rolling_return(&close, 63);
    let ret_126 = rolling_return(&close, 126);
    let rv_21 = rolling_realized_vol(&close, 21);
    let rv_63 = rolling_realized_vol(&close, 63);

    let mut out_sig = vec![0i32; n];
    let mut out_str = vec![0.0; n];
    for i in 126..n {
        let short_vol = rv_21[i].max(1e-6);
        let med_vol = rv_63[i].max(1e-6);
        let price_score = 0.15 * (ret_5[i] / short_vol)
            + 0.35 * (ret_21[i] / short_vol)
            + 0.30 * (ret_63[i] / med_vol)
            + 0.20 * (ret_126[i] / med_vol);

        let vol_ratio_fast = if vol_sma_20[i] > 1e-9 {
            volume.get(i).unwrap_or(0.0) / vol_sma_20[i]
        } else {
            1.0
        };
        let vol_ratio_slow = if vol_sma_63[i] > 1e-9 {
            vol_sma_20[i] / vol_sma_63[i]
        } else {
            1.0
        };
        let price_dir = if ret_21[i] > 0.0 {
            1.0
        } else if ret_21[i] < 0.0 {
            -1.0
        } else {
            0.0
        };
        let short_dir = if ret_5[i] > 0.0 {
            1.0
        } else if ret_5[i] < 0.0 {
            -1.0
        } else {
            0.0
        };
        let volume_score = 0.20 * (vol_ratio_fast.ln()).clamp(-1.5, 1.5) * short_dir
            + 0.20 * (vol_ratio_slow.ln()).clamp(-1.5, 1.5) * price_dir;

        let score = price_score + volume_score;
        out_str[i] = score.abs();
        if score > 0.35 {
            out_sig[i] = 1;
        } else if score < -0.35 {
            out_sig[i] = -1;
        }
    }
    Ok((out_sig, out_str))
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

fn generate_small_signal_strength(df: &DataFrame) -> Result<(Vec<i32>, Vec<f64>)> {
    // Small = inverse dollar-volume proxy (per-symbol time-series version).
    // Lower dollar volume now vs its own history → "small" signal.
    // Cross-sectional ranking is applied at the universe level in run();
    // here we use the symbol's own rolling volume rank as a proxy.
    let close = df.column("close")?.f64()?;
    let volume = df.column("volume")?.f64()?;
    let n = df.height();

    let mut dollar_vol = vec![0.0; n];
    for i in 0..n {
        let c = close.get(i).unwrap_or(0.0);
        let v = volume.get(i).unwrap_or(0.0);
        dollar_vol[i] = c * v;
    }

    // Rolling median dollar volume over 63 bars
    let mut dv_rank = vec![0.0; n];
    for i in 126..n {
        let mut window: Vec<f64> = (i.saturating_sub(63)..=i).map(|j| dollar_vol[j]).collect();
        window.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = window[window.len() / 2];
        dv_rank[i] = if med > 1e-9 { dollar_vol[i] / med } else { 1.0 };
    }

    let mut signals = vec![0i32; n];
    let mut strengths = vec![0.0; n];
    for i in 126..n {
        let rank = dv_rank[i];
        strengths[i] = (1.0 / rank.max(1e-6)).min(10.0);
        if rank < 0.80 {
            signals[i] = 1; // small → long signal
        } else if rank > 1.25 {
            signals[i] = -1; // large → short signal
        }
    }
    Ok((signals, strengths))
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

fn rolling_return(values: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        let now = values.get(i).unwrap_or(0.0);
        let prev = values.get(i - lookback).unwrap_or(0.0);
        if now > 0.0 && prev > 0.0 {
            out[i] = (now / prev).ln();
        }
    }
    out
}

fn rolling_realized_vol(values: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut rets = vec![0.0; values.len()];
    for i in 1..values.len() {
        let now = values.get(i).unwrap_or(0.0);
        let prev = values.get(i - 1).unwrap_or(0.0);
        if now > 0.0 && prev > 0.0 {
            rets[i] = (now / prev).ln();
        }
    }
    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        let window = &rets[i - lookback + 1..=i];
        let n = window.len() as f64;
        let mean = window.iter().sum::<f64>() / n;
        let var = window.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / n;
        out[i] = var.max(0.0).sqrt();
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

fn write_snapshot(universe_summaries: &[UniverseSummary]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let ts = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!("{}/family_risk_budget_overlay_{}.md", SNAPSHOT_DIR, ts);
    let archive_csv = format!("{}/family_risk_budget_overlay_{}.csv", SNAPSHOT_DIR, ts);

    let mut md = String::from("# Family Risk-Budget Overlay\n\n");
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

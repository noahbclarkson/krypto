//! Audit crypto-vs-macro relative-strength context on the frozen three-sleeve book.
//!
//! Purpose:
//! - follow the plan's next under-served lane instead of reopening pressure/state polish
//! - test whether lagged crypto-vs-SPX / DXY leadership is useful as sleeve-weight context
//!   for the frozen `DDBudget(A/D,MACD,Small)` book
//! - keep the comparison small and pre-declared: baseline vs two context overlays
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry at next open
//! - exit after fixed 21-bar hold at open
//! - 0.1% taker fee on entry and exit
//! - top-3 strength-capped book within each sleeve
//! - DDHard-style family budgets remain the base allocator
//! - macro inputs (SPX / DXY) are lagged to the previous available macro print
//! - crypto-relative-strength state uses prior crypto close vs prior macro print only

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
const ETH_SYMBOL: &str = "ETHUSDT";
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
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/crypto_macro_relative_strength_overlay_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/crypto_macro_relative_strength_overlay_latest.csv";
const SP500_CSV: &str = "data/exogenous/sp500.csv";
const DXY_CSV: &str = "data/exogenous/dxy.csv";
const RS_SMA: usize = 20;
const RS_SLOPE: usize = 5;

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
enum OverlayKind {
    Baseline,
    LeaderTilt,
    LaggardCut,
}

impl OverlayKind {
    fn all() -> &'static [OverlayKind] {
        &[Self::Baseline, Self::LeaderTilt, Self::LaggardCut]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Baseline => "Baseline DDHard",
            Self::LeaderTilt => "LeaderTilt(1.10 leader / 1.00 neutral / 0.55 laggard)",
            Self::LaggardCut => "LaggardCut(1.00 leader-neutral / 0.35 laggard)",
        }
    }

    fn exposure_multiplier(&self, bucket: RelativeStrengthBucket) -> f64 {
        match self {
            Self::Baseline => 1.0,
            Self::LeaderTilt => match bucket {
                RelativeStrengthBucket::Leader => 1.10,
                RelativeStrengthBucket::Neutral => 1.00,
                RelativeStrengthBucket::Laggard => 0.55,
            },
            Self::LaggardCut => match bucket {
                RelativeStrengthBucket::Leader | RelativeStrengthBucket::Neutral => 1.00,
                RelativeStrengthBucket::Laggard => 0.35,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum RelativeStrengthBucket {
    Leader,
    Neutral,
    Laggard,
}

impl RelativeStrengthBucket {
    fn name(&self) -> &'static str {
        match self {
            Self::Leader => "Leader",
            Self::Neutral => "Neutral",
            Self::Laggard => "Laggard",
        }
    }
}

#[derive(Clone, Debug)]
struct RelativeStrengthState {
    bucket: Vec<RelativeStrengthBucket>,
    score: Vec<i32>,
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

#[derive(Clone, Debug)]
struct FamilySleeve {
    strategy: StrategyKind,
    plans: Vec<SymbolPlan>,
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
    leader_days_pct: f64,
    laggard_days_pct: f64,
    state_return_pct: BTreeMap<RelativeStrengthBucket, f64>,
}

#[derive(Clone, Debug)]
struct ResultRow {
    overlay: OverlayKind,
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
    println!("=== CRYPTO-MACRO RELATIVE STRENGTH OVERLAY ===\n");
    println!("Question: does lagged crypto-vs-SPX / DXY leadership improve the frozen three-sleeve DDHard book?");
    println!(
        "Portfolio: DDBudget(A/D,MACD,Small) only, with pre-declared relative-strength overlays."
    );
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Context inputs: prior BTC/SPX, ETH/SPX, BTC/DXY relative-strength vs {}d SMA + {}d slope.\n", RS_SMA, RS_SLOPE);

    let macro_cache = fetch_macro_series()?;
    let loader = DataLoader::new(None, None);

    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;

    let raw_eth = loader.fetch_with_cache(ETH_SYMBOL, "1d", CANDLES).await?;
    let eth_df = FeatureEngine::add_technicals(&raw_eth, Some(&bench_df))?;

    let rs_state = build_relative_strength_state(&bench_df, &eth_df, &macro_cache)?;

    let mut data_cache = HashMap::<String, DataFrame>::new();
    data_cache.insert(BENCHMARK.to_string(), bench_df.clone());
    data_cache.insert(ETH_SYMBOL.to_string(), eth_df.clone());

    for &symbol in LOAD_SYMBOLS
        .iter()
        .filter(|&&s| s != BENCHMARK && s != ETH_SYMBOL)
    {
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
        for &overlay in OverlayKind::all() {
            let stats = simulate_family_portfolio(&sleeves, universe.steps, &rs_state, overlay);
            rows.push(ResultRow { overlay, stats });
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
            "{:<52} {:>11} {:>8} {:>8} {:>7} {:>7} {:>8} {:>8}",
            "Overlay", "Ret%", "Sharpe", "MaxDD", "Trades", "Exp", "Lead%", "Lag%"
        );
        println!("{}", "-".repeat(122));
        for row in &rows {
            println!(
                "{:<52} {:>10.1} {:>8.2} {:>7.1} {:>7} {:>6.2} {:>7.1} {:>7.1}",
                row.overlay.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.avg_exposure,
                row.stats.leader_days_pct,
                row.stats.laggard_days_pct,
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
    println!("- If leader/laggard occupancy has real spread, this is already a better state object than the earlier ~99.7% neutral macro score.");
    println!("- If laggard cuts reduce DD without merely erasing the whole book, relative-strength context is more promising than blunt macro stress gates.");
    println!("- This is still a context audit, not strategy promotion.");

    Ok(())
}

fn aligned_universe(cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<UniverseData> {
    let mut min_steps = usize::MAX;
    let mut data = Vec::new();
    for &symbol in symbols {
        let df = cache
            .get(symbol)
            .with_context(|| format!("missing {} in cache", symbol))?
            .clone();
        min_steps = min_steps.min(df.height());
        data.push((symbol.to_string(), df));
    }

    let steps = min_steps.saturating_sub(HOLD_BARS + 1);
    Ok(UniverseData { data, steps })
}

fn generate_small_by_dollar_volume_signal_strengths(
    universe: &UniverseData,
) -> Result<HashMap<String, Vec<(i32, f64)>>> {
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
    let mut out = HashMap::<String, Vec<(i32, f64)>>::new();
    for symbol in &symbols {
        out.insert(symbol.clone(), vec![(0, 0.0); n]);
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
            out.get_mut(symbol).unwrap()[i] = (signal, strength);
        }
    }
    Ok(out)
}

fn build_symbol_plans(
    universe: &UniverseData,
    strategy: StrategyKind,
    small_signal_map: &HashMap<String, Vec<(i32, f64)>>,
) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(symbol, df)| {
            let signals = match strategy {
                StrategyKind::AdMomentum => generate_ad_momentum_signals(df, AD_PERIOD)?,
                StrategyKind::MacdRegime => generate_macd_regime_signals(df)?,
                StrategyKind::SmallByDollarVol => small_signal_map
                    .get(symbol)
                    .context("missing small signal map")?
                    .iter()
                    .map(|(signal, _)| *signal)
                    .collect(),
            };
            let strengths = match strategy {
                StrategyKind::AdMomentum => generate_ad_strengths(df, AD_PERIOD)?,
                StrategyKind::MacdRegime => generate_macd_strengths(df)?,
                StrategyKind::SmallByDollarVol => small_signal_map
                    .get(symbol)
                    .context("missing small signal map")?
                    .iter()
                    .map(|(_, strength)| *strength)
                    .collect(),
            };
            build_plan_from_signals(df, &signals, &strengths)
        })
        .collect()
}

fn build_plan_from_signals(
    df: &DataFrame,
    signals: &[i32],
    strengths: &[f64],
) -> Result<SymbolPlan> {
    let open = df.column("open")?.f64()?;
    let mut trades = Vec::new();
    let mut i = WARMUP_BARS;
    let limit = df.height().saturating_sub(HOLD_BARS + 1);
    while i < limit {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }
        let entry_idx = i + 1;
        let exit_idx = entry_idx + HOLD_BARS;
        if exit_idx >= df.height() {
            break;
        }
        let entry = open.get(entry_idx).unwrap_or(0.0);
        let exit = open.get(exit_idx).unwrap_or(0.0);
        if entry <= 0.0 || exit <= 0.0 {
            i += 1;
            continue;
        }
        let gross_return = if signal > 0 {
            exit / entry - 1.0
        } else {
            entry / exit - 1.0
        };
        let net_return = gross_return - 2.0 * TAKER_FEE;
        trades.push(TradeWindow {
            entry_idx,
            exit_idx,
            strength: strengths.get(i).copied().unwrap_or(0.0),
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
    rs_state: &RelativeStrengthState,
    overlay: OverlayKind,
) -> PortfolioStats {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut exposure_sum = 0.0;
    let mut sleeve_equity = HashMap::<StrategyKind, f64>::new();
    let mut sleeve_peak = HashMap::<StrategyKind, f64>::new();
    let mut trade_count = 0usize;
    let mut wins = 0usize;
    let mut leader_days = 0usize;
    let mut laggard_days = 0usize;
    let mut state_return_pct = BTreeMap::<RelativeStrengthBucket, f64>::new();

    for bucket in [
        RelativeStrengthBucket::Leader,
        RelativeStrengthBucket::Neutral,
        RelativeStrengthBucket::Laggard,
    ] {
        state_return_pct.insert(bucket, 0.0);
    }

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

    let mut global_peak = 1.0_f64;
    for day in 0..steps {
        let bucket = rs_state
            .bucket
            .get(day.saturating_sub(1))
            .copied()
            .unwrap_or(RelativeStrengthBucket::Neutral);
        match bucket {
            RelativeStrengthBucket::Leader => leader_days += 1,
            RelativeStrengthBucket::Laggard => laggard_days += 1,
            RelativeStrengthBucket::Neutral => {}
        }

        let current_equity = equity_curve[day];
        global_peak = global_peak.max(current_equity);
        let global_dd = (1.0 - current_equity / global_peak) * 100.0;
        let global_recovery = current_equity / global_peak;

        let mut sleeve_daily = Vec::<(StrategyKind, f64, usize, f64)>::new();
        let mut total_active = 0usize;
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
            let base_weight = (1.0 / 3.0) * ddhard_exposure(dd, rec);
            let weight = base_weight * overlay.exposure_multiplier(bucket);
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
        *state_return_pct.get_mut(&bucket).unwrap() += portfolio_ret * 100.0;

        let _ = (global_dd, global_recovery);
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
        leader_days_pct: leader_days as f64 / steps.max(1) as f64 * 100.0,
        laggard_days_pct: laggard_days as f64 / steps.max(1) as f64 * 100.0,
        state_return_pct,
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

fn ddhard_exposure(dd_pct: f64, recovery_ratio: f64) -> f64 {
    if dd_pct >= 20.0 && recovery_ratio < 0.95 {
        0.30
    } else if dd_pct >= 10.0 && recovery_ratio < 0.98 {
        0.60
    } else {
        1.0
    }
}

fn fetch_macro_series() -> Result<HashMap<&'static str, BTreeMap<NaiveDate, f64>>> {
    let specs = [("SP500", SP500_CSV), ("DTWEXBGS", DXY_CSV)];
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

fn build_relative_strength_state(
    btc_df: &DataFrame,
    eth_df: &DataFrame,
    macro_cache: &HashMap<&'static str, BTreeMap<NaiveDate, f64>>,
) -> Result<RelativeStrengthState> {
    let dates = crypto_dates(btc_df)?;
    let btc_close = series_to_vec(btc_df.column("close")?.f64()?);
    let eth_close = series_to_vec(eth_df.column("close")?.f64()?);
    let spx_prev = align_previous_macro(&dates, macro_cache.get("SP500").context("missing SP500")?);
    let dxy_prev = align_previous_macro(
        &dates,
        macro_cache.get("DTWEXBGS").context("missing DTWEXBGS")?,
    );

    let btc_spx = ratio_series(&btc_close, &spx_prev);
    let eth_spx = ratio_series(&eth_close, &spx_prev);
    let btc_dxy = ratio_series(&btc_close, &dxy_prev);

    let btc_spx_sma = rolling_sma_option(&btc_spx, RS_SMA);
    let eth_spx_sma = rolling_sma_option(&eth_spx, RS_SMA);
    let btc_dxy_sma = rolling_sma_option(&btc_dxy, RS_SMA);

    let btc_spx_slope = trailing_slope_sign(&btc_spx, RS_SLOPE);
    let eth_spx_slope = trailing_slope_sign(&eth_spx, RS_SLOPE);
    let btc_dxy_slope = trailing_slope_sign(&btc_dxy, RS_SLOPE);

    let mut bucket = vec![RelativeStrengthBucket::Neutral; btc_df.height()];
    let mut score = vec![0i32; btc_df.height()];
    for i in 0..btc_df.height() {
        let components = [
            ratio_component_score(btc_spx[i], btc_spx_sma[i], btc_spx_slope[i]),
            ratio_component_score(eth_spx[i], eth_spx_sma[i], eth_spx_slope[i]),
            ratio_component_score(btc_dxy[i], btc_dxy_sma[i], btc_dxy_slope[i]),
        ];
        let sum = components.iter().sum::<i32>();
        score[i] = sum;
        bucket[i] = if sum >= 2 {
            RelativeStrengthBucket::Leader
        } else if sum <= -2 {
            RelativeStrengthBucket::Laggard
        } else {
            RelativeStrengthBucket::Neutral
        };
    }

    Ok(RelativeStrengthState { bucket, score })
}

fn ratio_component_score(value: Option<f64>, sma: Option<f64>, slope: i32) -> i32 {
    match (value, sma) {
        (Some(v), Some(avg)) if v > avg && slope > 0 => 1,
        (Some(v), Some(avg)) if v < avg && slope < 0 => -1,
        _ => 0,
    }
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

fn series_to_vec(values: &Float64Chunked) -> Vec<f64> {
    (0..values.len())
        .map(|i| values.get(i).unwrap_or(0.0))
        .collect()
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

fn ratio_series(numer: &[f64], denom: &[Option<f64>]) -> Vec<Option<f64>> {
    numer
        .iter()
        .zip(denom.iter())
        .map(|(n, d)| match d {
            Some(v) if *n > 0.0 && *v > 0.0 => Some(*n / *v),
            _ => None,
        })
        .collect()
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

fn trailing_slope_sign(series: &[Option<f64>], lookback: usize) -> Vec<i32> {
    let mut out = vec![0; series.len()];
    for i in lookback..series.len() {
        out[i] = match (series[i], series[i - lookback]) {
            (Some(now), Some(prev)) if now > prev => 1,
            (Some(now), Some(prev)) if now < prev => -1,
            _ => 0,
        };
    }
    out
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
        let prev = ad_line[i - period];
        let curr = ad_line[i];
        if curr > prev {
            out[i] = 1;
        } else if curr < prev {
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
    let mut out = vec![0.0; macd.len()];
    for i in 0..macd.len() {
        out[i] = (macd.get(i).unwrap_or(0.0) - signal.get(i).unwrap_or(0.0)).abs();
    }
    Ok(out)
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
        "{}/crypto_macro_relative_strength_overlay_{}.md",
        SNAPSHOT_DIR, timestamp
    );
    let archive_csv = format!(
        "{}/crypto_macro_relative_strength_overlay_{}.csv",
        SNAPSHOT_DIR, timestamp
    );

    let mut markdown = String::new();
    markdown.push_str("# Crypto-Macro Relative Strength Overlay Audit\n\n");
    markdown.push_str(&format!("- Timestamp (UTC): {}\n", Utc::now().to_rfc3339()));
    markdown.push_str("- Base portfolio: DDBudget(A/D,MACD,Small)\n");
    markdown.push_str(&format!(
        "- Top-{} strength-capped book within each sleeve\n",
        POSITION_CAP
    ));
    markdown.push_str(&format!("- Hold bars: {}\n", HOLD_BARS));
    markdown.push_str(&format!("- Fee each side: {:.3}%\n\n", TAKER_FEE * 100.0));

    for universe in universes {
        markdown.push_str(&format!("## {}\n\n", universe.label));
        markdown.push_str("| Rank | Overlay | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Exposure | Leader % | Laggard % | Leader Ret % | Neutral Ret % | Laggard Ret % |\n");
        markdown.push_str("|------|---------|---------:|-------:|--------:|-------:|------:|-------------:|---------:|----------:|-------------:|--------------:|--------------:|\n");
        for (idx, row) in universe.rows.iter().enumerate() {
            markdown.push_str(&format!(
                "| {} | {} | {:.1} | {:.2} | {:.1} | {} | {:.1} | {:.2} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |\n",
                idx + 1,
                row.overlay.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_exposure,
                row.stats.leader_days_pct,
                row.stats.laggard_days_pct,
                row.stats.state_return_pct.get(&RelativeStrengthBucket::Leader).copied().unwrap_or(0.0),
                row.stats.state_return_pct.get(&RelativeStrengthBucket::Neutral).copied().unwrap_or(0.0),
                row.stats.state_return_pct.get(&RelativeStrengthBucket::Laggard).copied().unwrap_or(0.0),
            ));
        }
        markdown.push('\n');
    }

    let mut csv = String::from("universe,rank,overlay,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_active_positions,idle_days_pct,avg_exposure,leader_days_pct,laggard_days_pct,leader_return_pct,neutral_return_pct,laggard_return_pct\n");
    for universe in universes {
        for (idx, row) in universe.rows.iter().enumerate() {
            csv.push_str(&format!(
                "{},{},{},{:.4},{:.4},{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                universe.label,
                idx + 1,
                row.overlay.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
                row.stats.avg_exposure,
                row.stats.leader_days_pct,
                row.stats.laggard_days_pct,
                row.stats.state_return_pct.get(&RelativeStrengthBucket::Leader).copied().unwrap_or(0.0),
                row.stats.state_return_pct.get(&RelativeStrengthBucket::Neutral).copied().unwrap_or(0.0),
                row.stats.state_return_pct.get(&RelativeStrengthBucket::Laggard).copied().unwrap_or(0.0),
            ));
        }
    }

    fs::write(SNAPSHOT_LATEST_MD, &markdown)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, markdown)?;
    fs::write(&archive_csv, csv)?;
    Ok((archive_md, archive_csv))
}

//! SmallByDollarVol lookback parameter sweep.

use anyhow::{Context, Result};
use chrono::Utc;
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use rayon::prelude::*;
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
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/small_lookback_sweep_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/small_lookback_sweep_latest.csv";
const SNAPSHOT_LATEST_EQUITY: &str = "snapshots/small_lookback_sweep_equity.csv";

const MIN_LOOKBACK: usize = 5;
const MAX_LOOKBACK: usize = 150;
const LOOKBACK_STEP: usize = 5;

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
    DdBudgetFull,
}

impl PortfolioKind {
    fn all() -> &'static [PortfolioKind] {
        &[Self::DdBudgetFull]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::DdBudgetFull => "DDBudget(A/D,MACD,Small)",
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

#[derive(Clone, Debug)]
struct FamilySleeve {
    strategy: StrategyKind,
    plans: Vec<SymbolPlan>,
}

#[derive(Clone, Debug)]
struct UniverseData {
    data: HashMap<String, DataFrame>,
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

#[derive(Clone)]
struct SweepResult {
    lookback: usize,
    portfolio: PortfolioKind,
    stats: PortfolioStats,
    equity_curve: Vec<f64>,
}

struct EquityExport {
    lookback: usize,
    portfolio_name: String,
    equity: Vec<f64>,
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
    equity_curve: Vec<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let btc_feat = FeatureEngine::add_technicals(&raw_bench, None)?;

    let mut loaded_data = HashMap::new();
    for symbol in LOAD_SYMBOLS {
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        if raw.height() < WARMUP_BARS {
            continue;
        }
        let df = FeatureEngine::add_technicals(&raw, Some(&btc_feat))?;
        loaded_data.insert(symbol.to_string(), df);
    }

    let mut min_height = usize::MAX;
    for df in loaded_data.values() {
        if df.height() < min_height {
            min_height = df.height();
        }
    }
    let steps = min_height - WARMUP_BARS;

    let mut all_results = HashMap::<String, Vec<SweepResult>>::new();
    let lookbacks: Vec<usize> = (MIN_LOOKBACK..=MAX_LOOKBACK)
        .step_by(LOOKBACK_STEP)
        .collect();

    for &(label, universe_symbols) in UNIVERSES {
        let mut u_data = HashMap::new();
        for &s in universe_symbols {
            if let Some(df) = loaded_data.get(s) {
                u_data.insert(s.to_string(), df.clone());
            }
        }
        if u_data.is_empty() {
            continue;
        }
        let universe = UniverseData { data: u_data };

        println!(
            "\n--- Universe: {} ({}) ---",
            label,
            universe_symbols.join(", ")
        );

        let ad_signal_map = generate_ad_momentum_signal_strengths(&universe)?;
        let macd_signal_map = generate_macd_regime_signal_strengths(&universe)?;

        let ad_sleeve = FamilySleeve {
            strategy: StrategyKind::AdMomentum,
            plans: generate_sleeve_plans(&universe, StrategyKind::AdMomentum, &ad_signal_map)?,
        };
        let macd_sleeve = FamilySleeve {
            strategy: StrategyKind::MacdRegime,
            plans: generate_sleeve_plans(&universe, StrategyKind::MacdRegime, &macd_signal_map)?,
        };

        let mut u_results: Vec<SweepResult> = lookbacks
            .par_iter()
            .map(|&lookback| {
                let small_signal_map =
                    generate_small_by_dollar_volume_signal_strengths(&universe, lookback).unwrap();
                let small_sleeve = FamilySleeve {
                    strategy: StrategyKind::SmallByDollarVol,
                    plans: generate_sleeve_plans(
                        &universe,
                        StrategyKind::SmallByDollarVol,
                        &small_signal_map,
                    )
                    .unwrap(),
                };

                let sleeves = vec![ad_sleeve.clone(), macd_sleeve.clone(), small_sleeve];
                let portfolio = PortfolioKind::DdBudgetFull;
                let stats = simulate_family_portfolio(&sleeves, steps, portfolio);

                SweepResult {
                    lookback,
                    portfolio,
                    equity_curve: stats.equity_curve.clone(),
                    stats,
                }
            })
            .collect();

        u_results.sort_by(|a, b| a.lookback.cmp(&b.lookback));

        println!(
            "{:<10} | {:<10} | {:<8} | {:<10} | {:<8} | {:<10}",
            "Lookback", "Return %", "Sharpe", "MaxDD %", "Trades", "Win Rate %"
        );
        for res in &u_results {
            println!(
                "{:<10} | {:<10.1} | {:<8.2} | {:<10.1} | {:<8} | {:<10.1}",
                res.lookback,
                res.stats.aligned_return_pct,
                res.stats.sharpe,
                res.stats.max_dd_pct,
                res.stats.trades,
                res.stats.win_rate_pct
            );
        }

        all_results.insert(label.to_string(), u_results);
    }

    write_snapshot(&all_results, &lookbacks)?;

    Ok(())
}

fn generate_sleeve_plans(
    universe: &UniverseData,
    strategy: StrategyKind,
    signal_map: &HashMap<String, (Vec<i32>, Vec<f64>)>,
) -> Result<Vec<SymbolPlan>> {
    let n = universe
        .data
        .values()
        .map(|df| df.height())
        .min()
        .unwrap_or(0);
    if n == 0 {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    let symbols: Vec<String> = universe.data.keys().cloned().collect();

    for symbol in &symbols {
        let df = &universe.data[symbol];
        let (signals, strengths) = &signal_map[symbol];
        let open = df.column("open")?.f64()?;
        let mut trades = Vec::new();
        let mut i = WARMUP_BARS;

        while i < n - 1 {
            if signals[i] == 0 {
                i += 1;
                continue;
            }

            let mut active_strengths = Vec::new();
            for s in &symbols {
                if let Some((sig, str)) = signal_map.get(s) {
                    if sig[i] != 0 {
                        active_strengths.push((s, str[i]));
                    }
                }
            }
            active_strengths.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            let rank = active_strengths
                .iter()
                .position(|(s, _)| *s == symbol)
                .unwrap_or(POSITION_CAP);

            if rank >= POSITION_CAP {
                i += 1;
                continue;
            }

            let signal = signals[i];
            let entry_idx = i + 1;
            let exit_idx = (entry_idx + HOLD_BARS).min(n - 1);
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
                strength: strengths[i].abs(),
                gross_return,
                net_return,
            });
            i = exit_idx;
        }
        plans.push(SymbolPlan { trades });
    }

    Ok(plans)
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
        equity_curve,
    }
}

fn included_strategies(portfolio: PortfolioKind) -> &'static [StrategyKind] {
    match portfolio {
        PortfolioKind::DdBudgetFull => StrategyKind::all(),
    }
}

fn sleeve_day_return(plans: &[SymbolPlan], day: usize) -> (f64, usize) {
    let mut sum_ret = 0.0;
    let mut active = 0usize;
    let target_idx = WARMUP_BARS + day;
    for plan in plans {
        for trade in &plan.trades {
            if target_idx >= trade.entry_idx && target_idx < trade.exit_idx {
                active += 1;
                sum_ret += trade.net_return / HOLD_BARS as f64;
            }
        }
    }
    if active > 0 {
        (sum_ret / active as f64, active)
    } else {
        (0.0, 0)
    }
}

fn sleeve_weight(
    portfolio: PortfolioKind,
    _strategy: StrategyKind,
    _sleeve_dd: f64,
    _sleeve_rec: f64,
    global_dd: f64,
    _global_rec: f64,
) -> f64 {
    match portfolio {
        PortfolioKind::DdBudgetFull => {
            if global_dd < 10.0 {
                1.0 / 3.0
            } else if global_dd < 20.0 {
                (1.0 / 3.0) * 0.60
            } else {
                (1.0 / 3.0) * 0.30
            }
        }
    }
}

fn generate_ad_momentum_signal_strengths(
    universe: &UniverseData,
) -> Result<HashMap<String, (Vec<i32>, Vec<f64>)>> {
    let mut res = HashMap::new();
    for (symbol, df) in &universe.data {
        let signals = generate_ad_momentum_signals(df, AD_PERIOD)?;
        let strengths = generate_ad_strengths(df, AD_PERIOD)?;
        res.insert(symbol.clone(), (signals, strengths));
    }
    Ok(res)
}

fn generate_macd_regime_signal_strengths(
    universe: &UniverseData,
) -> Result<HashMap<String, (Vec<i32>, Vec<f64>)>> {
    let mut res = HashMap::new();
    for (symbol, df) in &universe.data {
        let signals = generate_macd_regime_signals(df)?;
        let strengths = generate_macd_strengths(df)?;
        res.insert(symbol.clone(), (signals, strengths));
    }
    Ok(res)
}

fn generate_small_by_dollar_volume_signal_strengths(
    universe: &UniverseData,
    cs_lookback: usize,
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
        let dollar_volume = rolling_dollar_volume(close, volume, cs_lookback);
        let mut scores = vec![0.0; n];
        for i in cs_lookback..n {
            let dv = dollar_volume[i].max(1.0);
            scores[i] = -dv.ln();
        }
        raw_scores.insert(symbol.clone(), scores);
    }

    let mut res = HashMap::new();
    for s in universe.data.keys() {
        res.insert(s.clone(), (vec![1; n], raw_scores[s].clone()));
    }
    Ok(res)
}

fn generate_ad_momentum_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let mut out = vec![0; df.height()];
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let volume = df.column("volume")?.f64()?;

    let mut ad_line = vec![0.0; df.height()];
    let mut cum_ad = 0.0;
    for i in 0..df.height() {
        let c = close.get(i).unwrap_or(0.0);
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let v = volume.get(i).unwrap_or(0.0);

        if h > l {
            let mult = ((c - l) - (h - c)) / (h - l);
            cum_ad += mult * v;
        }
        ad_line[i] = cum_ad;
    }

    let sma200 = calculate_sma(close, 200);

    for i in period..df.height() {
        let mom = ad_line[i] - ad_line[i - period];
        let c = close.get(i).unwrap_or(0.0);
        let is_bull = c > sma200[i];

        if mom > 0.0 && is_bull {
            out[i] = 1;
        }
    }
    Ok(out)
}

fn generate_ad_strengths(df: &DataFrame, period: usize) -> Result<Vec<f64>> {
    let mut out = vec![0.0; df.height()];
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let volume = df.column("volume")?.f64()?;

    let mut ad_line = vec![0.0; df.height()];
    let mut cum_ad = 0.0;
    for i in 0..df.height() {
        let c = close.get(i).unwrap_or(0.0);
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let v = volume.get(i).unwrap_or(0.0);

        if h > l {
            let mult = ((c - l) - (h - c)) / (h - l);
            cum_ad += mult * v;
        }
        ad_line[i] = cum_ad;
    }

    for i in period..df.height() {
        out[i] = (ad_line[i] - ad_line[i - period]).abs();
    }
    Ok(out)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let mut out = vec![0; df.height()];
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    let close = df.column("close")?.f64()?;
    let sma200 = calculate_sma(close, 200);

    for i in 0..df.height() {
        let m = macd.get(i).unwrap_or(0.0);
        let s = signal.get(i).unwrap_or(0.0);
        let c = close.get(i).unwrap_or(0.0);
        let s200 = sma200[i];

        if m > s && c > s200 {
            out[i] = 1;
        } else if m < s && c < s200 {
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

fn write_snapshot(
    all_results: &HashMap<String, Vec<SweepResult>>,
    lookbacks: &[usize],
) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;

    let mut md = String::from("# Factor Small Sleeve Lookback Sweep\n\n");
    let mut csv = String::from("universe,lookback,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_active_positions,idle_days_pct,avg_exposure,dd20_days_pct\n");
    let mut equity_csv = String::from("universe,lookback,step,equity\n");

    for (universe, results) in all_results {
        md.push_str(&format!("## {}\n\n", universe));
        md.push_str("| Lookback | Return % | Sharpe | MaxDD % | Trades | Win Rate % | Avg Active | Idle % | Avg Exp | >20DD % |\n");
        md.push_str("|----------|----------|--------|---------|--------|------------|------------|--------|---------|---------|\n");

        for r in results {
            md.push_str(&format!(
                "| {} | {:.1} | {:.2} | {:.1} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |\n",
                r.lookback,
                r.stats.aligned_return_pct,
                r.stats.sharpe,
                r.stats.max_dd_pct,
                r.stats.trades,
                r.stats.win_rate_pct,
                r.stats.avg_active_positions,
                r.stats.idle_days_pct,
                r.stats.avg_exposure,
                r.stats.dd20_days_pct
            ));

            csv.push_str(&format!(
                "{},{},{:.2},{:.4},{:.2},{},{:.2},{:.2},{:.2},{:.2},{:.2}\n",
                universe,
                r.lookback,
                r.stats.aligned_return_pct,
                r.stats.sharpe,
                r.stats.max_dd_pct,
                r.stats.trades,
                r.stats.win_rate_pct,
                r.stats.avg_active_positions,
                r.stats.idle_days_pct,
                r.stats.avg_exposure,
                r.stats.dd20_days_pct
            ));

            for (step, &eq) in r.equity_curve.iter().enumerate() {
                equity_csv.push_str(&format!("{},{},{},{:.4}\n", universe, r.lookback, step, eq));
            }
        }
        md.push_str("\n");
    }

    fs::write(SNAPSHOT_LATEST_MD, &md)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(SNAPSHOT_LATEST_EQUITY, &equity_csv)?;
    println!(
        "Saved artifacts to:\n  {}\n  {}\n  {}",
        SNAPSHOT_LATEST_MD, SNAPSHOT_LATEST_CSV, SNAPSHOT_LATEST_EQUITY
    );

    Ok((
        SNAPSHOT_LATEST_MD.to_string(),
        SNAPSHOT_LATEST_CSV.to_string(),
    ))
}

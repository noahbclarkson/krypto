//! Attribute realistic capped-book portfolio returns of the current yardsticks by lagged macro state.
//!
//! Purpose:
//! - rebalance away from the recent relational-rescue loop
//! - stress the current benchmark yardsticks under the same realistic top-3 capped-book lens
//! - answer where the realistic portfolio returns actually come from: MacroRiskOn, MacroStress, or MacroNeutral

use anyhow::{Context, Result};
use chrono::{NaiveDate, TimeZone, Utc};
use krypto::{
    algo::{strategies::CrossSectionalMomentum, SignalGenerator},
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::fs;

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const WARMUP_BARS: usize = 200;
const TURTLE_PERIOD: usize = 20;
const POSITION_CAP: usize = 3;

const BASE5: &[&str] = &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];
const LEGACY5_BNB: &[&str] = &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT"];
const OLD_GUARD_NO_BNB: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
];

const SP500_CSV: &str = "data/exogenous/sp500.csv";
const VIX_CSV: &str = "data/exogenous/vixcls.csv";
const DXY_CSV: &str = "data/exogenous/dxy.csv";

const SNAPSHOT_LATEST_MD: &str = "snapshots/cross_family_macro_state_attribution_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/cross_family_macro_state_attribution_latest.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    MacdRegime,
    TurtleRegimeMacd,
    EnsembleMajority3,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
            Self::EnsembleMajority3 => "Ensemble(Majority 2/3)",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::MacdRegime,
            Self::TurtleRegimeMacd,
            Self::EnsembleMajority3,
        ]
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

    fn all() -> &'static [MacroStateBucket] {
        &[Self::RiskOn, Self::Stress, Self::Neutral]
    }
}

#[derive(Clone, Debug)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    gross_return: f64,
}

#[derive(Clone, Debug)]
struct SymbolPlan {
    trades: Vec<TradeWindow>,
}

#[derive(Clone, Debug)]
struct DailyContribution {
    strength: f64,
    ret: f64,
}

#[derive(Clone, Debug)]
struct PortfolioTrace {
    daily_returns: Vec<f64>,
}

#[derive(Clone, Debug)]
struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

#[derive(Clone, Debug)]
struct MarketState {
    macro_risk_on: Vec<bool>,
    macro_stress: Vec<bool>,
}

#[derive(Clone, Debug, Default)]
struct StateStats {
    days: usize,
    total_return_pct: f64,
    avg_daily_return_pct: f64,
    annualized_sharpe: f64,
}

#[derive(Clone, Debug)]
struct SummaryRow {
    universe: String,
    strategy: String,
    state: String,
    days: usize,
    total_return_pct: f64,
    avg_daily_return_pct: f64,
    annualized_sharpe: f64,
}

fn main() -> Result<()> {
    println!("=== CROSS-FAMILY MACRO-STATE ATTRIBUTION ===\n");
    println!(
        "Goal: attribute current realistic yardstick returns by lagged macro state under the same top-{} capped-book lens",
        POSITION_CAP
    );
    println!(
        "Lens: next-open entry, {}-bar hold, {:.1}% taker/side, capped book\n",
        HOLD_BARS,
        TAKER_FEE * 100.0
    );

    fs::create_dir_all("snapshots")?;

    let macro_cache = fetch_macro_series()?;
    let loader = DataLoader::new(None, None);
    let mut raw_cache = HashMap::<String, DataFrame>::new();

    print!("Loading BTCUSDT... ");
    let benchmark_raw = tokio::runtime::Runtime::new()?
        .block_on(loader.fetch_with_cache("BTCUSDT", "1d", CANDLES))?;
    let benchmark_df = FeatureEngine::add_technicals(&benchmark_raw, None)?;
    println!("{} bars", benchmark_df.height());
    raw_cache.insert("BTCUSDT".to_string(), benchmark_df.clone());

    let others = [
        "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
        "BCHUSDT",
    ];
    let mut cs_map = HashMap::<String, DataFrame>::new();
    for symbol in others {
        print!("Loading {}... ", symbol);
        let raw = tokio::runtime::Runtime::new()?
            .block_on(loader.fetch_with_cache(symbol, "1d", CANDLES))?;
        let df = FeatureEngine::add_technicals(&raw, Some(&benchmark_df))?;
        println!("{} bars", df.height());
        cs_map.insert(symbol.to_string(), df);
    }
    compute_cross_sectional_features(&mut cs_map, 63)?;
    raw_cache.extend(cs_map);

    let benchmark = raw_cache
        .get("BTCUSDT")
        .context("missing BTCUSDT benchmark data")?;
    let market_state = build_macro_state(benchmark, &macro_cache)?;

    let cases = vec![
        ("Base5", BASE5),
        ("Legacy5BNB", LEGACY5_BNB),
        ("OldGuardNoBNB", OLD_GUARD_NO_BNB),
    ];
    let mut summaries = Vec::new();

    for (label, symbols) in cases {
        println!("\n=== {} ===", label);
        let universe = aligned_universe(&raw_cache, symbols)?;
        let state_view = trim_market_state(&market_state, universe.steps);

        for &strategy in StrategyKind::all() {
            let plans = build_symbol_plans(&universe, strategy)?;
            let trace = simulate_portfolio_trace(&plans, universe.steps, POSITION_CAP);
            let stats = attribute_by_macro_state(&trace, &state_view);

            println!("- {}", strategy.name());
            for bucket in MacroStateBucket::all() {
                let row = stats.get(bucket).unwrap();
                println!(
                    "    {:<12} days {:>4} | return {:+9.1}% | avg/day {:+6.3}% | Sharpe {:>5.2}",
                    bucket.name(),
                    row.days,
                    row.total_return_pct,
                    row.avg_daily_return_pct,
                    row.annualized_sharpe,
                );
                summaries.push(SummaryRow {
                    universe: label.to_string(),
                    strategy: strategy.name().to_string(),
                    state: bucket.name().to_string(),
                    days: row.days,
                    total_return_pct: row.total_return_pct,
                    avg_daily_return_pct: row.avg_daily_return_pct,
                    annualized_sharpe: row.annualized_sharpe,
                });
            }
        }
    }

    write_markdown(&summaries, SNAPSHOT_LATEST_MD)?;
    write_csv(&summaries, SNAPSHOT_LATEST_CSV)?;

    println!("\nWrote:");
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);

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
        .map(|(_, df)| build_symbol_plan(df, strategy))
        .collect()
}

fn build_symbol_plan(df: &DataFrame, strategy: StrategyKind) -> Result<SymbolPlan> {
    let signals = generate_signals(df, strategy)?;
    let strengths = generate_strengths(df, strategy)?;
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
        trades.push(TradeWindow {
            entry_idx,
            exit_idx,
            strength: strengths.get(i).copied().unwrap_or(0.0).abs(),
            gross_return,
        });
        i = exit_idx;
    }

    Ok(SymbolPlan { trades })
}

fn simulate_portfolio_trace(plans: &[SymbolPlan], steps: usize, cap: usize) -> PortfolioTrace {
    let mut daily_returns = vec![0.0; steps];

    for day in 0..steps {
        let mut active = Vec::<DailyContribution>::new();
        for plan in plans {
            for trade in &plan.trades {
                if day >= trade.entry_idx && day < trade.exit_idx {
                    let days_in_trade = (trade.exit_idx - trade.entry_idx).max(1) as f64;
                    let daily_ret = (1.0 + trade.gross_return).powf(1.0 / days_in_trade) - 1.0;
                    let daily_ret = daily_ret - (2.0 * TAKER_FEE / days_in_trade);
                    active.push(DailyContribution {
                        strength: trade.strength,
                        ret: daily_ret,
                    });
                    break;
                }
            }
        }

        if active.is_empty() {
            continue;
        }

        active.sort_by(|a, b| {
            b.strength
                .partial_cmp(&a.strength)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let picked = &active[..active.len().min(cap)];
        let mean_ret = picked.iter().map(|x| x.ret).sum::<f64>() / picked.len() as f64;
        daily_returns[day] = mean_ret;
    }

    PortfolioTrace { daily_returns }
}

fn attribute_by_macro_state(
    trace: &PortfolioTrace,
    state: &MarketState,
) -> BTreeMap<MacroStateBucket, StateStats> {
    let mut buckets = BTreeMap::<MacroStateBucket, Vec<f64>>::new();
    for bucket in MacroStateBucket::all() {
        buckets.insert(*bucket, Vec::new());
    }

    for day in 0..trace.daily_returns.len() {
        let bucket = if state.macro_risk_on.get(day).copied().unwrap_or(false) {
            MacroStateBucket::RiskOn
        } else if state.macro_stress.get(day).copied().unwrap_or(false) {
            MacroStateBucket::Stress
        } else {
            MacroStateBucket::Neutral
        };
        buckets
            .get_mut(&bucket)
            .unwrap()
            .push(trace.daily_returns[day]);
    }

    let mut out = BTreeMap::new();
    for bucket in MacroStateBucket::all() {
        let returns = buckets.remove(bucket).unwrap_or_default();
        out.insert(*bucket, summarize_returns(&returns));
    }
    out
}

fn summarize_returns(returns: &[f64]) -> StateStats {
    if returns.is_empty() {
        return StateStats::default();
    }

    let days = returns.len();
    let total = returns.iter().fold(1.0, |acc, r| acc * (1.0 + r)) - 1.0;
    let mean = returns.iter().sum::<f64>() / days as f64;
    let variance = if days > 1 {
        returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (days as f64 - 1.0)
    } else {
        0.0
    };
    let stdev = variance.sqrt();
    let sharpe = if stdev > 0.0 {
        mean / stdev * 252.0_f64.sqrt()
    } else {
        0.0
    };

    StateStats {
        days,
        total_return_pct: total * 100.0,
        avg_daily_return_pct: mean * 100.0,
        annualized_sharpe: sharpe,
    }
}

fn generate_signals(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::MacdRegime => generate_macd_regime_signals(df),
        StrategyKind::TurtleRegimeMacd => generate_turtle_regime_macd_signals(df, TURTLE_PERIOD),
        StrategyKind::EnsembleMajority3 => {
            let macd = generate_macd_regime_signals(df)?;
            let turtle = generate_turtle_regime_macd_signals(df, TURTLE_PERIOD)?;
            let csm = CrossSectionalMomentum::new()
                .predict(df)?
                .f64()?
                .into_iter()
                .map(|v| match v.unwrap_or(0.0).partial_cmp(&0.0) {
                    Some(std::cmp::Ordering::Greater) => 1,
                    Some(std::cmp::Ordering::Less) => -1,
                    _ => 0,
                })
                .collect::<Vec<_>>();
            Ok(consensus_signal(&[&macd, &turtle, &csm], 2))
        }
    }
}

fn generate_strengths(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<f64>> {
    let close = collect_f64(df, "close")?;
    let macd_line = collect_f64(df, "macd")?;
    let macd_signal = collect_f64(df, "macd_signal")?;
    let sma200 = calculate_sma_vec(&close, 200);
    let highs = collect_f64(df, "high")?;
    let lows = collect_f64(df, "low")?;
    let csm = CrossSectionalMomentum::new().predict(df)?.f64()?.clone();

    let n = df.height();
    let mut out = vec![0.0; n];
    for i in 0..n {
        let c = close[i];
        let macd_gap = macd_line[i] - macd_signal[i];
        let regime_gap = if c > 0.0 { (c - sma200[i]) / c } else { 0.0 };
        let turtle_gap = if i >= TURTLE_PERIOD {
            let start = i + 1 - TURTLE_PERIOD;
            let mut hh = f64::NEG_INFINITY;
            let mut ll = f64::INFINITY;
            for j in start..=i {
                hh = hh.max(highs[j]);
                ll = ll.min(lows[j]);
            }
            if c > 0.0 {
                ((c - hh) / c).abs().max(((c - ll) / c).abs())
            } else {
                0.0
            }
        } else {
            0.0
        };
        let cs = csm.get(i).unwrap_or(0.0).abs();

        out[i] = match strategy {
            StrategyKind::MacdRegime => macd_gap.abs() + regime_gap.abs(),
            StrategyKind::TurtleRegimeMacd => turtle_gap + macd_gap.abs() + regime_gap.abs(),
            StrategyKind::EnsembleMajority3 => macd_gap.abs() + turtle_gap + regime_gap.abs() + cs,
        };
    }
    Ok(out)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let close = collect_f64(df, "close")?;
    let sma200 = calculate_sma_vec(&close, 200);
    let macd = collect_f64(df, "macd")?;
    let signal = collect_f64(df, "macd_signal")?;
    let mut out = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        let c = close[i];
        let s = sma200[i];
        let m = macd[i];
        let ms = signal[i];
        out.push(if c > s && m > ms {
            1
        } else if c < s && m < ms {
            -1
        } else {
            0
        });
    }
    Ok(out)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = collect_f64(df, "close")?;
    let high = collect_f64(df, "high")?;
    let low = collect_f64(df, "low")?;
    let sma200 = calculate_sma_vec(&close, 200);
    let macd = collect_f64(df, "macd")?;
    let signal = collect_f64(df, "macd_signal")?;

    let n = df.height();
    let mut out = vec![0; n];
    for i in period..n {
        let c = close[i];
        let s = sma200[i];
        let m = macd[i];
        let ms = signal[i];
        let mut hh = f64::NEG_INFINITY;
        let mut ll = f64::INFINITY;
        for j in (i - period)..i {
            hh = hh.max(high[j]);
            ll = ll.min(low[j]);
        }
        out[i] = if c > hh && c > s && m > ms {
            1
        } else if c < ll && c < s && m < ms {
            -1
        } else {
            0
        };
    }
    Ok(out)
}

fn consensus_signal(signals: &[&[i32]], threshold: usize) -> Vec<i32> {
    let n = signals.iter().map(|s| s.len()).min().unwrap_or(0);
    let mut out = vec![0; n];
    for i in 0..n {
        let long_votes = signals.iter().filter(|s| s[i] > 0).count();
        let short_votes = signals.iter().filter(|s| s[i] < 0).count();
        out[i] = if long_votes >= threshold {
            1
        } else if short_votes >= threshold {
            -1
        } else {
            0
        };
    }
    out
}

fn fetch_macro_series() -> Result<HashMap<String, BTreeMap<NaiveDate, f64>>> {
    let mut out = HashMap::new();
    out.insert("sp500".to_string(), read_macro_csv(SP500_CSV)?);
    out.insert("vix".to_string(), read_macro_csv(VIX_CSV)?);
    out.insert("dxy".to_string(), read_macro_csv(DXY_CSV)?);
    Ok(out)
}

fn read_macro_csv(path: &str) -> Result<BTreeMap<NaiveDate, f64>> {
    let content = fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let mut map = BTreeMap::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split(',');
        let date = parts.next().context("missing date")?.trim();
        let value = parts.next().context("missing value")?.trim();
        if value == "." || value.is_empty() {
            continue;
        }
        let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
        let value: f64 = value.parse()?;
        map.insert(date, value);
    }
    Ok(map)
}

fn build_macro_state(
    benchmark_df: &DataFrame,
    macro_cache: &HashMap<String, BTreeMap<NaiveDate, f64>>,
) -> Result<MarketState> {
    let times = benchmark_df.column("time")?.datetime()?;
    let mut dates = Vec::with_capacity(benchmark_df.height());
    for i in 0..times.len() {
        let ts = times.get(i).context("missing time")?;
        let dt = Utc
            .timestamp_millis_opt(ts)
            .single()
            .context("bad timestamp")?;
        dates.push(dt.date_naive());
    }

    let spx = aligned_lagged_series(&dates, macro_cache.get("sp500").context("missing sp500")?)?;
    let vix = aligned_lagged_series(&dates, macro_cache.get("vix").context("missing vix")?)?;
    let dxy = aligned_lagged_series(&dates, macro_cache.get("dxy").context("missing dxy")?)?;

    let spx_sma50 = rolling_mean(&spx, 50);
    let vix_sma20 = rolling_mean(&vix, 20);
    let dxy_sma50 = rolling_mean(&dxy, 50);

    let mut macro_risk_on = vec![false; dates.len()];
    let mut macro_stress = vec![false; dates.len()];
    for i in 0..dates.len() {
        let spx_ok = matches!((spx[i], spx_sma50[i]), (Some(a), Some(b)) if a > b);
        let vix_calm = matches!((vix[i], vix_sma20[i]), (Some(a), Some(b)) if a < b);
        let dxy_weak = matches!((dxy[i], dxy_sma50[i]), (Some(a), Some(b)) if a < b);
        let vix_stress = matches!((vix[i], vix_sma20[i]), (Some(a), Some(b)) if a > b);
        let dxy_stress = matches!((dxy[i], dxy_sma50[i]), (Some(a), Some(b)) if a > b);
        macro_risk_on[i] = spx_ok && vix_calm && dxy_weak;
        macro_stress[i] = vix_stress || dxy_stress;
    }

    Ok(MarketState {
        macro_risk_on,
        macro_stress,
    })
}

fn trim_market_state(state: &MarketState, steps: usize) -> MarketState {
    MarketState {
        macro_risk_on: state.macro_risk_on.iter().copied().take(steps).collect(),
        macro_stress: state.macro_stress.iter().copied().take(steps).collect(),
    }
}

fn aligned_lagged_series(
    dates: &[NaiveDate],
    raw: &BTreeMap<NaiveDate, f64>,
) -> Result<Vec<Option<f64>>> {
    let mut out = Vec::with_capacity(dates.len());
    for date in dates {
        let prior = raw.range(..*date).next_back().map(|(_, v)| *v);
        out.push(prior);
    }
    Ok(out)
}

fn rolling_mean(series: &[Option<f64>], window: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; series.len()];
    let mut vals = Vec::<f64>::new();
    for i in 0..series.len() {
        if let Some(v) = series[i] {
            vals.push(v);
        }
        if i >= window {
            if let Some(v) = series[i - window] {
                if let Some(pos) = vals.iter().position(|x| (*x - v).abs() < 1e-12) {
                    vals.remove(pos);
                }
            }
        }
        if i + 1 >= window && !vals.is_empty() {
            out[i] = Some(vals.iter().sum::<f64>() / vals.len() as f64);
        }
    }
    out
}

fn collect_f64(df: &DataFrame, column: &str) -> Result<Vec<f64>> {
    Ok(df
        .column(column)?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect())
}

fn calculate_sma_vec(values: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i >= period {
            sum -= values[i - period];
        }
        let denom = if i + 1 < period { i + 1 } else { period };
        out[i] = sum / denom as f64;
    }
    out
}

fn write_markdown(rows: &[SummaryRow], path: &str) -> Result<()> {
    let mut out = String::new();
    out.push_str("# Cross-Family Macro-State Attribution\n\n");
    out.push_str("Realistic top-3 capped-book portfolio attribution of current yardsticks by lagged macro state.\n\n");
    let mut grouped = BTreeMap::<String, Vec<&SummaryRow>>::new();
    for row in rows {
        grouped.entry(row.universe.clone()).or_default().push(row);
    }

    for (universe, items) in grouped {
        out.push_str(&format!("## {}\n\n", universe));
        out.push_str("| Strategy | State | Days | Return % | Avg day % | Sharpe |\n");
        out.push_str("|---|---:|---:|---:|---:|---:|\n");
        for row in items {
            out.push_str(&format!(
                "| {} | {} | {} | {:+.1} | {:+.3} | {:.2} |\n",
                row.strategy,
                row.state,
                row.days,
                row.total_return_pct,
                row.avg_daily_return_pct,
                row.annualized_sharpe
            ));
        }
        out.push('\n');
    }
    fs::write(path, out)?;
    Ok(())
}

fn write_csv(rows: &[SummaryRow], path: &str) -> Result<()> {
    let mut out = String::from(
        "universe,strategy,state,days,total_return_pct,avg_daily_return_pct,annualized_sharpe\n",
    );
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{},{:.6},{:.6},{:.6}\n",
            row.universe,
            row.strategy,
            row.state,
            row.days,
            row.total_return_pct,
            row.avg_daily_return_pct,
            row.annualized_sharpe
        ));
    }
    fs::write(path, out)?;
    Ok(())
}

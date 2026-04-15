//! Decompose Sharpe ratios and max drawdowns of the realistic yardsticks by lagged macro
//! state under the top-3 capped-book lens.
//!
//! Purpose:
//!   Follow up the macro state attribution: does stress-dependency mean high-Sharpe or
//!   just high-level noise?  The attribution showed raw returns; this asks whether
//!   those returns are high-Sharpe (genuinely good risk-budget) or just high-return
//!   in high-volatility windows (bad risk-budget even if nominal return is high).
//!
//! Scope:
//!   same fair daily harness: signal at close, next-open entry, fixed 21-bar hold,
//!   0.1% taker each side, top-3 strength-capped book
//!   universes: Base5, Legacy5BNB, OldGuardNoBNB
//!   strategies: MACD+Regime, Turtle+Regime+MACD, Ensemble(Majority 2/3)
//!   macro buckets: MacroRiskOn, MacroStress, MacroNeutral (conservative alignment)

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

const SNAPSHOT_LATEST_MD: &str = "snapshots/cross_family_state_sharpe_decomposition_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/cross_family_state_sharpe_decomposition_latest.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    fn all() -> Vec<StrategyKind> {
        vec![
            StrategyKind::MacdRegime,
            StrategyKind::TurtleRegimeMacd,
            StrategyKind::EnsembleMajority3,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum MacroStateBucket {
    RiskOn,
    Stress,
    Neutral,
    All,
}

impl MacroStateBucket {
    fn all() -> Vec<Self> {
        vec![Self::RiskOn, Self::Stress, Self::Neutral, Self::All]
    }
    fn label(&self) -> &'static str {
        match self {
            Self::RiskOn => "RiskOn",
            Self::Stress => "Stress",
            Self::Neutral => "Neutral",
            Self::All => "All",
        }
    }
}

#[derive(Debug, Default, Clone)]
struct StateStats {
    days: usize,
    total_return_pct: f64,
    avg_daily_return_pct: f64,
    annualized_sharpe: f64,
    max_drawdown_pct: f64,
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

struct SymbolPlan {
    trades: Vec<TradeWindow>,
}

#[derive(Clone)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    gross_return: f64,
}

struct DailyContribution {
    strength: f64,
    ret: f64,
}

struct PortfolioTrace {
    daily_returns: Vec<f64>,
}

struct MarketState {
    macro_risk_on: Vec<bool>,
    macro_stress: Vec<bool>,
}

#[derive(Debug, Default)]
struct ResultRow {
    universe: String,
    strategy: String,
    overall: StateStats,
    by_state: BTreeMap<MacroStateBucket, StateStats>,
}

fn main() -> Result<()> {
    let snapshot_ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");

    fs::create_dir_all("snapshots")?;

    println!(
        "State Sharpe Decomposition: next-open entry, {}-bar hold, {:.1}% taker/side, top-3 capped book",
        HOLD_BARS,
        TAKER_FEE * 100.0
    );

    // Load macro data
    let macro_cache = fetch_macro_series()?;
    let loader = DataLoader::new(None, None);

    // Load benchmark (BTCUSDT)
    print!("Loading BTCUSDT (benchmark)... ");
    let benchmark_raw = tokio::runtime::Runtime::new()?
        .block_on(loader.fetch_with_cache("BTCUSDT", "1d", CANDLES))?;
    let benchmark_df = FeatureEngine::add_technicals(&benchmark_raw, None)?;
    println!("{} bars", benchmark_df.height());

    // Load all universe symbols
    let others = [
        "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
        "BCHUSDT",
    ];
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    raw_cache.insert("BTCUSDT".to_string(), benchmark_df.clone());

    let mut cs_map: HashMap<String, DataFrame> = HashMap::new();
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
        .context("missing BTCUSDT benchmark")?;
    let market_state = build_macro_state(benchmark, &macro_cache)?;

    let universes = [
        ("Base5", BASE5),
        ("Legacy5BNB", LEGACY5_BNB),
        ("OldGuardNoBNB", OLD_GUARD_NO_BNB),
    ];

    let mut all_rows: Vec<ResultRow> = Vec::new();

    for (universe_name, symbols) in universes {
        println!("\n=== {} ===", universe_name);
        let universe = aligned_universe(&raw_cache, symbols)?;
        let trimmed_state = trim_market_state(&market_state, universe.steps);

        for strategy in StrategyKind::all() {
            let plans = build_symbol_plans(&universe, strategy)?;
            let trace = simulate_portfolio_trace(&plans, universe.steps, POSITION_CAP);
            let by_state = attribute_by_macro_state(&trace, &trimmed_state);
            let overall = summarize_returns_with_dd(&trace.daily_returns);

            // Print console summary
            println!("- {}", strategy.name());
            for bucket in MacroStateBucket::all() {
                if let Some(st) = by_state.get(&bucket) {
                    println!(
                        "    {:<12} days {:>4} | return {:+9.1}% | Sharpe {:>6.2} | MaxDD {:>6.1}%",
                        bucket.label(),
                        st.days,
                        st.total_return_pct,
                        st.annualized_sharpe,
                        st.max_drawdown_pct,
                    );
                }
            }

            all_rows.push(ResultRow {
                universe: universe_name.to_string(),
                strategy: strategy.name().to_string(),
                overall,
                by_state,
            });
        }
    }

    // Write snapshots
    let md = build_markdown(&all_rows);
    fs::write(SNAPSHOT_LATEST_MD, &md)?;
    println!("\nWrote {}", SNAPSHOT_LATEST_MD);

    let csv = build_csv(&all_rows);
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    println!("Wrote {}", SNAPSHOT_LATEST_CSV);

    let ts_md = format!(
        "snapshots/cross_family_state_sharpe_decomposition_{}.md",
        snapshot_ts
    );
    let ts_csv = format!(
        "snapshots/cross_family_state_sharpe_decomposition_{}.csv",
        snapshot_ts
    );
    fs::write(&ts_md, &md)?;
    fs::write(&ts_csv, &csv)?;
    println!("Wrote {}", ts_md);

    Ok(())
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

fn aligned_universe(
    raw_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<UniverseData> {
    let min_len = symbols
        .iter()
        .filter_map(|s| raw_cache.get(*s).map(|df| df.height()))
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
    let mut plans = Vec::new();
    for (sym, df) in &universe.data {
        match build_symbol_plan(df, strategy) {
            Ok(p) => plans.push(p),
            Err(e) => anyhow::bail!("build_symbol_plan failed for {}: {:?}", sym, e),
        }
    }
    Ok(plans)
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
    let mut buckets: BTreeMap<MacroStateBucket, Vec<f64>> = BTreeMap::new();
    for bucket in MacroStateBucket::all() {
        buckets.insert(bucket, Vec::new());
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
        buckets
            .get_mut(&MacroStateBucket::All)
            .unwrap()
            .push(trace.daily_returns[day]);
    }

    let mut out = BTreeMap::new();
    for bucket in MacroStateBucket::all() {
        let returns = buckets.remove(&bucket).unwrap_or_default();
        out.insert(bucket, summarize_returns_with_dd(&returns));
    }
    out
}

fn summarize_returns_with_dd(returns: &[f64]) -> StateStats {
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
        mean / stdev * (252.0_f64.sqrt())
    } else {
        0.0
    };

    // Max drawdown
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut equity = 1.0_f64;
    for r in returns {
        equity *= 1.0 + r;
        peak = peak.max(equity);
        let dd = (equity - peak) / peak;
        if dd < max_dd {
            max_dd = dd;
        }
    }

    StateStats {
        days,
        total_return_pct: total * 100.0,
        avg_daily_return_pct: mean * 100.0,
        annualized_sharpe: sharpe,
        max_drawdown_pct: max_dd * 100.0,
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
    let macd_col = df.column("macd")?.f64()?;
    let macd_signal_col = df.column("macd_signal")?.f64()?;
    let macd_line: Vec<f64> = macd_col.into_iter().map(|v| v.unwrap_or(0.0)).collect();
    let macd_signal: Vec<f64> = macd_signal_col
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
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
    let macd_col = df.column("macd")?.f64()?;
    let macd_signal_col = df.column("macd_signal")?.f64()?;
    let macd_line: Vec<f64> = macd_col.into_iter().map(|v| v.unwrap_or(0.0)).collect();
    let macd_signal: Vec<f64> = macd_signal_col
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let sma200 = calculate_sma_vec(&close, 200);
    let n = df.height();
    let mut signals = vec![0; n];

    for i in WARMUP_BARS..n {
        let regime = close[i] > sma200[i];
        let macd_buy = macd_line[i] > macd_signal[i];
        let macd_sell = macd_line[i] < macd_signal[i];

        if regime && macd_buy {
            signals[i] = 1;
        } else if !regime && macd_sell {
            signals[i] = -1;
        }
    }
    Ok(signals)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = collect_f64(df, "close")?;
    let macd_col = df.column("macd")?.f64()?;
    let macd_signal_col = df.column("macd_signal")?.f64()?;
    let macd_line: Vec<f64> = macd_col.into_iter().map(|v| v.unwrap_or(0.0)).collect();
    let macd_signal: Vec<f64> = macd_signal_col
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let sma200 = calculate_sma_vec(&close, 200);
    let highs = collect_f64(df, "high")?;
    let lows = collect_f64(df, "low")?;
    let n = df.height();
    let mut signals = vec![0; n];

    for i in (WARMUP_BARS + period)..n {
        let regime = close[i] > sma200[i];
        if !regime {
            continue;
        }
        let start = i + 1 - period;
        let mut hh = f64::NEG_INFINITY;
        let mut ll = f64::INFINITY;
        for j in start..=i {
            hh = hh.max(highs[j]);
            ll = ll.min(lows[j]);
        }
        let macd_buy = macd_line[i] > macd_signal[i];
        let macd_sell = macd_line[i] < macd_signal[i];

        if regime && macd_buy && close[i] >= hh {
            signals[i] = 1;
        } else if regime && macd_sell && close[i] <= ll {
            signals[i] = -1;
        }
    }
    Ok(signals)
}

fn consensus_signal(signals: &[&[i32]], min_agree: usize) -> Vec<i32> {
    let n = signals.first().map(|s| s.len()).unwrap_or(0);
    let mut out = vec![0; n];
    for i in 0..n {
        let pos = signals.iter().filter(|s| s[i] > 0).count();
        let neg = signals.iter().filter(|s| s[i] < 0).count();
        if pos >= min_agree {
            out[i] = 1;
        } else if neg >= min_agree {
            out[i] = -1;
        }
    }
    out
}

fn collect_f64(df: &DataFrame, col: &str) -> Result<Vec<f64>> {
    df.column(col)
        .map_err(|e| anyhow::anyhow!("{:?}", e))?
        .f64()
        .map_err(|e| anyhow::anyhow!("{:?}", e))?
        .into_iter()
        .map(|v| v.ok_or_else(|| anyhow::anyhow!("null")))
        .collect()
}

fn calculate_sma_vec(values: &[f64], window: usize) -> Vec<f64> {
    let n = values.len();
    let mut sma = vec![0.0; n];
    for i in window..n {
        let sum: f64 = values[(i + 1 - window)..=i].iter().sum();
        sma[i] = sum / window as f64;
    }
    sma
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

fn build_markdown(rows: &[ResultRow]) -> String {
    let mut s = String::new();
    s.push_str("# Cross-Family State Sharpe Decomposition\n\n");
    s.push_str("*Top-3 capped-book lens. Does stress-state dominance mean high-Sharpe or high-level noise?*\n\n");

    let universe_names = ["Base5", "Legacy5BNB", "OldGuardNoBNB"];

    for &universe_name in &universe_names {
        let group: Vec<&ResultRow> = rows
            .iter()
            .filter(|r| r.universe == universe_name)
            .collect();
        if group.is_empty() {
            continue;
        }

        s.push_str(&format!("## {universe_name}\n\n"));

        // Headline table
        s.push_str("| Strategy | Overall Sharpe | Overall Return | Stress Sharpe | Stress Days (frac) | RiskOn Sharpe | RiskOn Days (frac) |\n");
        s.push_str("|----------|---------------|---------------|---------------|-------------------|---------------|--------------------|\n");

        for r in &group {
            let overall = &r.overall;
            let stress = r
                .by_state
                .get(&MacroStateBucket::Stress)
                .cloned()
                .unwrap_or_default();
            let risk_on = r
                .by_state
                .get(&MacroStateBucket::RiskOn)
                .cloned()
                .unwrap_or_default();
            let stress_frac = stress.days as f64 / overall.days.max(1) as f64 * 100.0;
            let riskon_frac = risk_on.days as f64 / overall.days.max(1) as f64 * 100.0;

            s.push_str(&format!(
                "| {} | **{:.2}** | {:.1}% | {:.2} | {} ({:.0}%) | {:.2} | {} ({:.0}%) |\n",
                r.strategy,
                overall.annualized_sharpe,
                overall.total_return_pct,
                stress.annualized_sharpe,
                stress.days,
                stress_frac,
                risk_on.annualized_sharpe,
                risk_on.days,
                riskon_frac,
            ));
        }
        s.push_str("\n");

        // State decomposition
        s.push_str("| Strategy | State | Days | Return | Avg Daily | Sharpe | MaxDD |\n");
        s.push_str("|---------|-------|------|--------|----------|--------|-------|\n");
        for r in &group {
            for bucket in MacroStateBucket::all() {
                let st = r.by_state.get(&bucket).cloned().unwrap_or_default();
                if st.days == 0 {
                    continue;
                }
                s.push_str(&format!(
                    "| {} | {} | {} | {:.1}% | {:.4}% | {:.2} | {:.1}% |\n",
                    r.strategy,
                    bucket.label(),
                    st.days,
                    st.total_return_pct,
                    st.avg_daily_return_pct,
                    st.annualized_sharpe,
                    st.max_drawdown_pct,
                ));
            }
        }
        s.push_str("\n");

        // Trust read
        s.push_str("**Trust read**\n\n");
        for r in &group {
            let overall = &r.overall;
            let stress = r
                .by_state
                .get(&MacroStateBucket::Stress)
                .cloned()
                .unwrap_or_default();
            let risk_on = r
                .by_state
                .get(&MacroStateBucket::RiskOn)
                .cloned()
                .unwrap_or_default();
            let neutral = r
                .by_state
                .get(&MacroStateBucket::Neutral)
                .cloned()
                .unwrap_or_default();

            let stress_frac = stress.days as f64 / overall.days.max(1) as f64;
            let riskon_frac = risk_on.days as f64 / overall.days.max(1) as f64;
            let neutral_frac = neutral.days as f64 / overall.days.max(1) as f64;

            let quality = if stress_frac > 0.40 && stress.annualized_sharpe > 2.0 {
                "HIGH-CONCENTRATION STRESS-HARVEST"
            } else if stress_frac > 0.30 && stress.annualized_sharpe > risk_on.annualized_sharpe {
                "STRESS-HARVEST (Sharpe OK, state concentration risk)"
            } else if riskon_frac > 0.30 && risk_on.annualized_sharpe > stress.annualized_sharpe {
                "RISK-ON DEFENSIVE"
            } else if stress_frac < 0.15 && riskon_frac < 0.15 {
                "LOW-STATE-RELIANCE"
            } else {
                "BALANCED STATE MIX"
            };

            s.push_str(&format!(
                "- **{}**: overall {:.2} Sharpe / {:.1}% return / {:.1}% DD | \
                 Stress {:.2} Sharpe ({:.0}% of days) | RiskOn {:.2} Sharpe ({:.0}%) | \
                 Neutral {:.2} ({:.0}%) | *{}*\n",
                r.strategy,
                overall.annualized_sharpe,
                overall.total_return_pct,
                overall.max_drawdown_pct,
                stress.annualized_sharpe,
                stress_frac * 100.0,
                risk_on.annualized_sharpe,
                riskon_frac * 100.0,
                neutral.annualized_sharpe,
                neutral_frac * 100.0,
                quality,
            ));
        }
        s.push_str("\n---\n\n");
    }

    s
}

fn build_csv(rows: &[ResultRow]) -> String {
    let mut lines = Vec::new();
    lines.push("universe,strategy,state,days,total_return_pct,avg_daily_return_pct,annualized_sharpe,max_drawdown_pct".to_string());

    for r in rows {
        for bucket in MacroStateBucket::all() {
            let st = r.by_state.get(&bucket).cloned().unwrap_or_default();
            if st.days == 0 {
                continue;
            }
            lines.push(format!(
                "{},{},{},{},{:.4},{:.6},{:.4},{:.4}",
                r.universe,
                r.strategy,
                bucket.label(),
                st.days,
                st.total_return_pct,
                st.avg_daily_return_pct,
                st.annualized_sharpe,
                st.max_drawdown_pct,
            ));
        }
    }
    lines.join("\n")
}

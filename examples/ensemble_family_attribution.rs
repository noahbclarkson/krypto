//! Attribution / orthogonality audit for the majority-vote ensemble.
//!
//! Goal:
//! - explain whether Ensemble(Majority 2/3) is actually adding orthogonal signal value
//!   or mostly repackaging the same trend exposure
//! - decompose results by symbol, universe, and coalition source
//! - quantify when CrossSectionalMomentum changes the ensemble decision versus when
//!   the two trend legs already agree on their own

use anyhow::{Context, Result};
use krypto::{
    algo::{strategies::CrossSectionalMomentum, SignalGenerator},
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;

const BENCHMARK: &str = "BTCUSDT";
const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
    "EOSUSDT", "BCHUSDT",
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const CS_LOOKBACK: usize = 63;
const TURTLE_PERIOD: usize = 20;

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

#[derive(Default, Clone, Debug)]
struct StrategyResult {
    total_return_pct: f64,
    trades: usize,
    wins: usize,
}

impl StrategyResult {
    fn add_trade(&mut self, net_return_pct: f64) {
        self.total_return_pct += net_return_pct;
        self.trades += 1;
        if net_return_pct > 0.0 {
            self.wins += 1;
        }
    }

    fn add_assign(&mut self, other: &StrategyResult) {
        self.total_return_pct += other.total_return_pct;
        self.trades += other.trades;
        self.wins += other.wins;
    }

    fn win_rate(&self) -> f64 {
        if self.trades == 0 {
            0.0
        } else {
            self.wins as f64 / self.trades as f64
        }
    }
}

#[derive(Default, Clone, Debug)]
struct CoalitionStats {
    result: StrategyResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Coalition {
    TrendConsensus,
    MacdPlusCross,
    TurtlePlusCross,
    Unanimous,
}

impl Coalition {
    fn all() -> &'static [Coalition] {
        &[
            Self::TrendConsensus,
            Self::MacdPlusCross,
            Self::TurtlePlusCross,
            Self::Unanimous,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::TrendConsensus => "MACD+Regime + Turtle+Regime+MACD",
            Self::MacdPlusCross => "MACD+Regime + CrossSectionalMomentum",
            Self::TurtlePlusCross => "Turtle+Regime+MACD + CrossSectionalMomentum",
            Self::Unanimous => "All three agree",
        }
    }
}

#[derive(Default, Clone, Debug)]
struct EnsembleTradeAttribution {
    majority: StrategyResult,
    macd: StrategyResult,
    turtle: StrategyResult,
    cross: StrategyResult,
    coalition_stats: HashMap<Coalition, CoalitionStats>,
    csm_changed_decision_trades: usize,
    csm_changed_decision_return_pct: f64,
    trend_pair_only_trades: usize,
    trend_pair_only_return_pct: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== ENSEMBLE FAMILY ATTRIBUTION ===\n");
    println!("Benchmark leader for relative features: {}", BENCHMARK);
    println!("Cross-sectional lookback: {} bars", CS_LOOKBACK);
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side\n", TAKER_FEE * 100.0);

    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;

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

    let mut grand_coalitions = HashMap::<Coalition, CoalitionStats>::new();
    let mut grand_majority = StrategyResult::default();
    let mut grand_macd = StrategyResult::default();
    let mut grand_turtle = StrategyResult::default();
    let mut grand_cross = StrategyResult::default();
    let mut grand_csm_changed_trades = 0usize;
    let mut grand_csm_changed_return = 0.0;
    let mut grand_trend_pair_only_trades = 0usize;
    let mut grand_trend_pair_only_return = 0.0;

    for &(universe_name, universe_symbols) in UNIVERSES {
        println!(
            "\n--- Universe: {} ({}) ---",
            universe_name,
            universe_symbols.join(", ")
        );

        let mut universe = EnsembleTradeAttribution::default();
        for &symbol in universe_symbols {
            let df = data_cache
                .get(symbol)
                .with_context(|| format!("missing df for {}", symbol))?;
            let attr = attribute_symbol(df)?;

            println!(
                "{:<12} majority {:>8.1}% ({:>3} trades, {:>5.1}% win) | MACD+Regime {:>8.1}% | Turtle+Regime+MACD {:>8.1}% | CSM {:>8.1}%",
                symbol,
                attr.majority.total_return_pct,
                attr.majority.trades,
                attr.majority.win_rate() * 100.0,
                attr.macd.total_return_pct,
                attr.turtle.total_return_pct,
                attr.cross.total_return_pct,
            );

            universe.majority.add_assign(&attr.majority);
            universe.macd.add_assign(&attr.macd);
            universe.turtle.add_assign(&attr.turtle);
            universe.cross.add_assign(&attr.cross);
            universe.csm_changed_decision_trades += attr.csm_changed_decision_trades;
            universe.csm_changed_decision_return_pct += attr.csm_changed_decision_return_pct;
            universe.trend_pair_only_trades += attr.trend_pair_only_trades;
            universe.trend_pair_only_return_pct += attr.trend_pair_only_return_pct;
            for coalition in Coalition::all() {
                let entry = attr
                    .coalition_stats
                    .get(coalition)
                    .cloned()
                    .unwrap_or_default();
                universe
                    .coalition_stats
                    .entry(*coalition)
                    .or_default()
                    .result
                    .add_assign(&entry.result);
            }
        }

        print_universe_summary(universe_name, &universe);

        grand_majority.add_assign(&universe.majority);
        grand_macd.add_assign(&universe.macd);
        grand_turtle.add_assign(&universe.turtle);
        grand_cross.add_assign(&universe.cross);
        grand_csm_changed_trades += universe.csm_changed_decision_trades;
        grand_csm_changed_return += universe.csm_changed_decision_return_pct;
        grand_trend_pair_only_trades += universe.trend_pair_only_trades;
        grand_trend_pair_only_return += universe.trend_pair_only_return_pct;
        for coalition in Coalition::all() {
            let entry = universe
                .coalition_stats
                .get(coalition)
                .cloned()
                .unwrap_or_default();
            grand_coalitions
                .entry(*coalition)
                .or_default()
                .result
                .add_assign(&entry.result);
        }
    }

    println!("\n=== GRAND SUMMARY ACROSS ALL UNIVERSES ===");
    println!(
        "Majority 2/3     {:>10.1}% {:>5} trades {:>5.1}% win",
        grand_majority.total_return_pct,
        grand_majority.trades,
        grand_majority.win_rate() * 100.0
    );
    println!(
        "MACD+Regime      {:>10.1}% {:>5} trades {:>5.1}% win",
        grand_macd.total_return_pct,
        grand_macd.trades,
        grand_macd.win_rate() * 100.0
    );
    println!(
        "Turtle+Reg+MACD  {:>10.1}% {:>5} trades {:>5.1}% win",
        grand_turtle.total_return_pct,
        grand_turtle.trades,
        grand_turtle.win_rate() * 100.0
    );
    println!(
        "CSM              {:>10.1}% {:>5} trades {:>5.1}% win",
        grand_cross.total_return_pct,
        grand_cross.trades,
        grand_cross.win_rate() * 100.0
    );

    println!("\nCoalition attribution (majority trades only):");
    for coalition in Coalition::all() {
        let stats = grand_coalitions.get(coalition).cloned().unwrap_or_default();
        println!(
            "{:<36} {:>8.1}% {:>5} trades {:>5.1}% win",
            coalition.name(),
            stats.result.total_return_pct,
            stats.result.trades,
            stats.result.win_rate() * 100.0
        );
    }

    println!("\nOrthogonality read:");
    println!(
        "CSM changed the majority decision on {:>4} trades for {:>8.1}% total return.",
        grand_csm_changed_trades, grand_csm_changed_return
    );
    println!(
        "Pure trend-pair consensus accounted for {:>4} trades and {:>8.1}% total return.",
        grand_trend_pair_only_trades, grand_trend_pair_only_return
    );
    println!(
        "If most value comes from trend-pair consensus, the ensemble is mainly smoothing trend exposure. If CSM-changed trades contribute meaningfully, the ensemble is adding some genuine breadth."
    );

    Ok(())
}

fn print_universe_summary(universe_name: &str, universe: &EnsembleTradeAttribution) {
    println!("\n{} summary:", universe_name);
    println!(
        "  Majority 2/3    {:>8.1}% {:>4} trades {:>5.1}% win",
        universe.majority.total_return_pct,
        universe.majority.trades,
        universe.majority.win_rate() * 100.0
    );
    println!(
        "  MACD+Regime     {:>8.1}% {:>4} trades {:>5.1}% win",
        universe.macd.total_return_pct,
        universe.macd.trades,
        universe.macd.win_rate() * 100.0
    );
    println!(
        "  Turtle+Reg+MACD {:>8.1}% {:>4} trades {:>5.1}% win",
        universe.turtle.total_return_pct,
        universe.turtle.trades,
        universe.turtle.win_rate() * 100.0
    );
    println!(
        "  CSM             {:>8.1}% {:>4} trades {:>5.1}% win",
        universe.cross.total_return_pct,
        universe.cross.trades,
        universe.cross.win_rate() * 100.0
    );

    println!("  Coalition split:");
    for coalition in Coalition::all() {
        let stats = universe
            .coalition_stats
            .get(coalition)
            .cloned()
            .unwrap_or_default();
        println!(
            "    {:<34} {:>8.1}% {:>4} trades {:>5.1}% win",
            coalition.name(),
            stats.result.total_return_pct,
            stats.result.trades,
            stats.result.win_rate() * 100.0
        );
    }
    println!(
        "  CSM changed decision: {:>4} trades, {:>8.1}% return",
        universe.csm_changed_decision_trades, universe.csm_changed_decision_return_pct
    );
    println!(
        "  Trend pair only:      {:>4} trades, {:>8.1}% return",
        universe.trend_pair_only_trades, universe.trend_pair_only_return_pct
    );
}

fn attribute_symbol(df: &DataFrame) -> Result<EnsembleTradeAttribution> {
    let open = df.column("open")?.f64()?;
    let macd = generate_macd_regime_signals(df)?;
    let turtle = generate_turtle_regime_macd_signals(df, TURTLE_PERIOD)?;
    let cross = generate_cross_sectional_signals(df)?;
    let majority = consensus_signal(&[&macd, &turtle, &cross], 2);

    let mut out = EnsembleTradeAttribution::default();
    let end = df.height();

    let mut i = 1usize;
    while i + HOLD_BARS < end {
        let entry_price = open.get(i).unwrap_or(0.0);
        let exit_price = open.get(i + HOLD_BARS).unwrap_or(0.0);
        if entry_price <= 0.0 || exit_price <= 0.0 {
            i += 1;
            continue;
        }

        let macd_sig = macd[i - 1];
        let turtle_sig = turtle[i - 1];
        let cross_sig = cross[i - 1];
        let majority_sig = majority[i - 1];

        add_strategy_trade(&mut out.macd, macd_sig, entry_price, exit_price);
        add_strategy_trade(&mut out.turtle, turtle_sig, entry_price, exit_price);
        add_strategy_trade(&mut out.cross, cross_sig, entry_price, exit_price);

        if majority_sig != 0 {
            let trade_return = trade_return_pct(majority_sig, entry_price, exit_price);
            out.majority.add_trade(trade_return);

            let coalition = classify_coalition(macd_sig, turtle_sig, cross_sig, majority_sig)
                .context("failed to classify majority coalition")?;
            out.coalition_stats
                .entry(coalition)
                .or_default()
                .result
                .add_trade(trade_return);

            let trend_pair_agree = macd_sig == majority_sig && turtle_sig == majority_sig;
            let csm_agrees = cross_sig == majority_sig;
            if trend_pair_agree && !csm_agrees {
                out.trend_pair_only_trades += 1;
                out.trend_pair_only_return_pct += trade_return;
            }
            if !trend_pair_agree && csm_agrees {
                out.csm_changed_decision_trades += 1;
                out.csm_changed_decision_return_pct += trade_return;
            }
        }

        i += HOLD_BARS;
    }

    Ok(out)
}

fn add_strategy_trade(result: &mut StrategyResult, signal: i32, entry_price: f64, exit_price: f64) {
    if signal != 0 {
        result.add_trade(trade_return_pct(signal, entry_price, exit_price));
    }
}

fn classify_coalition(macd: i32, turtle: i32, cross: i32, majority: i32) -> Result<Coalition> {
    let macd_ok = macd == majority;
    let turtle_ok = turtle == majority;
    let cross_ok = cross == majority;
    match (macd_ok, turtle_ok, cross_ok) {
        (true, true, true) => Ok(Coalition::Unanimous),
        (true, true, false) => Ok(Coalition::TrendConsensus),
        (true, false, true) => Ok(Coalition::MacdPlusCross),
        (false, true, true) => Ok(Coalition::TurtlePlusCross),
        _ => anyhow::bail!("majority trade without 2-of-3 agreement"),
    }
}

fn trade_return_pct(signal: i32, entry: f64, exit: f64) -> f64 {
    let gross = if signal > 0 {
        (exit / entry) - 1.0
    } else {
        (entry / exit) - 1.0
    };
    (gross - (2.0 * TAKER_FEE)) * 100.0
}

fn generate_cross_sectional_signals(df: &DataFrame) -> Result<Vec<i32>> {
    Ok(CrossSectionalMomentum::new()
        .predict(df)?
        .f64()?
        .into_iter()
        .map(|v| match v.unwrap_or(0.0).partial_cmp(&0.0) {
            Some(std::cmp::Ordering::Greater) => 1,
            Some(std::cmp::Ordering::Less) => -1,
            _ => 0,
        })
        .collect::<Vec<_>>())
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

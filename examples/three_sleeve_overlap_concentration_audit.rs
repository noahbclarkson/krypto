//! Structural overlap / concentration audit for the frozen three-sleeve book.
//!
//! Purpose:
//! - trust-test the current `A/D + MACD + Small` reference book in a more structural way
//! - measure how much symbol overlap and crowding actually exists across sleeves
//! - avoid another local overlay or factor-weight loop
//!
//! Lens:
//! - signal at close
//! - trade opens next bar and holds for 21 bars
//! - inspect active top-3 strength-capped holdings per sleeve each day
//! - this is a portfolio-realism audit, not a new strategy search

use anyhow::{Context, Result};
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};

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
const POSITION_CAP: usize = 3;
const WARMUP_BARS: usize = 200;
const CS_LOOKBACK: usize = 63;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
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
            Self::AdMomentum => "A/D",
            Self::MacdRegime => "MACD",
            Self::SmallByDollarVol => "Small",
        }
    }
}

#[derive(Clone, Debug)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    signal: i32,
}

#[derive(Clone, Debug)]
struct SymbolPlan {
    symbol: String,
    trades: Vec<TradeWindow>,
}

#[derive(Clone, Debug)]
struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

#[derive(Default, Clone, Debug)]
struct PairMetrics {
    days_union_active: usize,
    days_both_active: usize,
    days_symbol_overlap: usize,
    overlap_ratio_sum: f64,
    same_sign_symbol_sum: usize,
    shared_symbol_sum: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== THREE-SLEEVE OVERLAP / CONCENTRATION AUDIT ===\n");
    println!("Question: is the frozen `DDBudget(A/D,MACD,Small)` book actually diversified at the symbol-occupancy level, or are sleeves crowding into the same names?");
    println!("Lens: next-open 21-bar holds, active top-{} positions per sleeve, structural overlap only.\n", POSITION_CAP);

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

    let mut aggregate_pairs = BTreeMap::<(StrategyKind, StrategyKind), PairMetrics>::new();

    for &(label, symbols) in UNIVERSES {
        println!("--- Universe: {} ({}) ---", label, symbols.join(", "));
        let universe = aligned_universe(&data_cache, symbols)?;
        let small_signal_map = generate_small_by_dollar_volume_signal_strengths(&universe)?;

        let mut sleeves = BTreeMap::<StrategyKind, Vec<SymbolPlan>>::new();
        for &strategy in StrategyKind::all() {
            let plans = universe
                .data
                .iter()
                .map(|(symbol, df)| build_symbol_plan(df, symbol, strategy, &small_signal_map))
                .collect::<Result<Vec<_>>>()?;
            sleeves.insert(strategy, plans);
        }

        let mut avg_unique_symbols = 0.0;
        let mut avg_total_slots = 0.0;
        let mut avg_duplication = 0.0;
        let mut avg_top_symbol_weight = 0.0;
        let mut active_days = 0usize;
        let mut sleeve_active_counts = BTreeMap::<StrategyKind, usize>::new();
        let mut sleeve_avg_names = BTreeMap::<StrategyKind, f64>::new();
        let mut symbol_weight_days = BTreeMap::<String, f64>::new();

        for day in 0..universe.steps {
            let mut by_sleeve = BTreeMap::<StrategyKind, Vec<(String, i32, f64)>>::new();
            for (&strategy, plans) in &sleeves {
                let picks = active_top_positions(plans, day);
                if !picks.is_empty() {
                    *sleeve_active_counts.entry(strategy).or_default() += 1;
                    *sleeve_avg_names.entry(strategy).or_default() += picks.len() as f64;
                }
                by_sleeve.insert(strategy, picks);
            }

            let mut symbol_weight = BTreeMap::<String, f64>::new();
            let mut total_slots = 0usize;
            for picks in by_sleeve.values() {
                if picks.is_empty() {
                    continue;
                }
                let per_slot_weight = 1.0 / StrategyKind::all().len() as f64 / picks.len() as f64;
                for (symbol, _signal, _strength) in picks {
                    total_slots += 1;
                    *symbol_weight.entry(symbol.clone()).or_default() += per_slot_weight;
                }
            }

            if total_slots > 0 {
                active_days += 1;
                let unique_symbols = symbol_weight.len();
                let duplication = 1.0 - unique_symbols as f64 / total_slots as f64;
                let top_symbol_weight = symbol_weight.values().copied().fold(0.0, f64::max);
                avg_unique_symbols += unique_symbols as f64;
                avg_total_slots += total_slots as f64;
                avg_duplication += duplication;
                avg_top_symbol_weight += top_symbol_weight;
                for (symbol, weight) in symbol_weight {
                    *symbol_weight_days.entry(symbol).or_default() += weight;
                }
            }

            let all = StrategyKind::all();
            for i in 0..all.len() {
                for j in i + 1..all.len() {
                    let a = all[i];
                    let b = all[j];
                    let pa = by_sleeve.get(&a).unwrap();
                    let pb = by_sleeve.get(&b).unwrap();
                    let ma = aggregate_pairs.entry((a, b)).or_default();
                    update_pair_metrics(ma, pa, pb);
                }
            }
        }

        let denom = active_days.max(1) as f64;
        println!(
            "book avg_unique_symbols {:>4.2} | avg_total_slots {:>4.2} | duplication {:>5.1}% | top_symbol_weight {:>5.1}%",
            avg_unique_symbols / denom,
            avg_total_slots / denom,
            avg_duplication / denom * 100.0,
            avg_top_symbol_weight / denom * 100.0,
        );

        for &strategy in StrategyKind::all() {
            let active = *sleeve_active_counts.get(&strategy).unwrap_or(&0);
            let avg_names =
                sleeve_avg_names.get(&strategy).copied().unwrap_or(0.0) / active.max(1) as f64;
            println!(
                "{:<5} active_days {:>5.1}% | avg_names_when_active {:>4.2}",
                strategy.name(),
                active as f64 / universe.steps.max(1) as f64 * 100.0,
                avg_names,
            );
        }

        let mut symbol_rank: Vec<_> = symbol_weight_days.into_iter().collect();
        symbol_rank.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let total_weight_days = symbol_rank.iter().map(|(_, w)| *w).sum::<f64>().max(1e-9);
        let leaders = symbol_rank
            .iter()
            .take(3)
            .map(|(s, w)| {
                format!(
                    "{} {:.1}%",
                    s.trim_end_matches("USDT"),
                    w / total_weight_days * 100.0
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("book crowding leaders: {}", leaders);
        println!();
    }

    println!("=== AGGREGATE PAIR OVERLAP SUMMARY ===");
    for ((a, b), m) in aggregate_pairs {
        let denom_union = m.days_union_active.max(1) as f64;
        let denom_both = m.days_both_active.max(1) as f64;
        let denom_shared = m.shared_symbol_sum.max(1) as f64;
        println!(
            "{:<5} vs {:<5} union-active {:>5.1}% | both-active {:>5.1}% | any-symbol-overlap {:>5.1}% | overlap_ratio {:>5.1}% | same-sign-on-shared {:>5.1}%",
            a.name(),
            b.name(),
            m.days_union_active as f64 / (UNIVERSES.len() * universe_len_placeholder()) as f64 * 100.0,
            m.days_both_active as f64 / (UNIVERSES.len() * universe_len_placeholder()) as f64 * 100.0,
            m.days_symbol_overlap as f64 / denom_union * 100.0,
            m.overlap_ratio_sum / denom_both * 100.0,
            m.same_sign_symbol_sum as f64 / denom_shared * 100.0,
        );
    }

    println!("\nInterpretation:");
    println!("- Lower duplication and lower pairwise symbol-overlap mean the sleeves are earning diversification structurally, not just cosmetically.");
    println!("- High top-symbol weight means the frozen book still hides name concentration even if sleeve-level Sharpe looks good.");
    println!(
        "- This is a trust audit; it should decide whether the lab is over-reading a crowded book."
    );
    Ok(())
}

fn update_pair_metrics(
    metrics: &mut PairMetrics,
    a: &[(String, i32, f64)],
    b: &[(String, i32, f64)],
) {
    let set_a: HashSet<_> = a.iter().map(|(s, _, _)| s.as_str()).collect();
    let set_b: HashSet<_> = b.iter().map(|(s, _, _)| s.as_str()).collect();
    if !a.is_empty() || !b.is_empty() {
        metrics.days_union_active += 1;
    }
    if !a.is_empty() && !b.is_empty() {
        metrics.days_both_active += 1;
        let overlap = set_a.intersection(&set_b).count();
        if overlap > 0 {
            metrics.days_symbol_overlap += 1;
        }
        let union = set_a.union(&set_b).count().max(1);
        metrics.overlap_ratio_sum += overlap as f64 / union as f64;

        let sign_b = b
            .iter()
            .map(|(s, sig, _)| (s.as_str(), *sig))
            .collect::<HashMap<_, _>>();
        for (symbol, sig_a, _) in a {
            if let Some(sig_b) = sign_b.get(symbol.as_str()) {
                metrics.shared_symbol_sum += 1;
                if *sig_b == *sig_a {
                    metrics.same_sign_symbol_sum += 1;
                }
            }
        }
    }
}

fn active_top_positions(plans: &[SymbolPlan], day: usize) -> Vec<(String, i32, f64)> {
    let mut out = Vec::<(String, i32, f64)>::new();
    for plan in plans {
        for trade in &plan.trades {
            if day >= trade.entry_idx && day <= trade.exit_idx {
                out.push((plan.symbol.clone(), trade.signal, trade.strength));
            }
        }
    }
    out.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    out.truncate(POSITION_CAP.min(out.len()));
    out
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
            let slot = out.get_mut(symbol).unwrap();
            slot.0[i] = signal;
            slot.1[i] = z.abs();
        }
    }
    Ok(out)
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
        if open.get(entry_idx).unwrap_or(0.0) <= 0.0 || open.get(exit_idx).unwrap_or(0.0) <= 0.0 {
            i += 1;
            continue;
        }
        trades.push(TradeWindow {
            entry_idx,
            exit_idx,
            strength: strengths.get(i).copied().unwrap_or(0.0).abs(),
            signal,
        });
        i = exit_idx;
    }
    Ok(SymbolPlan {
        symbol: symbol.to_string(),
        trades,
    })
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
        let sma_now = sma_200[i];
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

fn universe_len_placeholder() -> usize {
    2800
}

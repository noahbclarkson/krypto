//! Factor-decomposition audit for the current daily yardsticks.
//!
//! Purpose:
//! - test whether `A/D`, `CTREND`, `MACD+Regime`, and `Turtle+MACD`
//!   are meaningfully distinct or mostly the same latent trend engine
//! - use the same cached harsh-universe inputs already trusted elsewhere
//! - answer a structural question, not optimize parameters

use anyhow::{Context, Result};
use krypto::{
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
const CS_LOOKBACK: usize = 63;
const TURTLE_PERIOD: usize = 20;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    AdMomentum,
    CTRend,
    MacdRegime,
    TurtleMACD,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::AdMomentum,
            Self::CTRend,
            Self::MacdRegime,
            Self::TurtleMACD,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::AdMomentum => "A/D",
            Self::CTRend => "CTREND",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleMACD => "Turtle+MACD",
        }
    }
}

#[derive(Default, Clone, Debug)]
struct PairAgg {
    days_both_active: usize,
    days_same_sign: usize,
    days_opposite_sign: usize,
    days_either_active: usize,
    corr_sum: f64,
    corr_count: usize,
    avg_abs_diff_sum: f64,
    universes: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== FACTOR DECOMPOSITION AUDIT ===\n");
    println!(
        "Question: are A/D, CTREND, MACD+Regime, and Turtle+MACD distinct enough to diversify?"
    );
    println!("Lens: harsh universes, daily signal overlap, and daily PnL correlation under shared 21-bar holding logic.\n");

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

    let mut pair_aggs: HashMap<(StrategyKind, StrategyKind), PairAgg> = HashMap::new();

    for &(universe_name, symbols) in UNIVERSES {
        let len = min_symbol_len(&data_cache, symbols)?;
        println!("--- Universe: {} ---", universe_name);

        let mut strat_series: HashMap<StrategyKind, Vec<f64>> = HashMap::new();
        let mut strat_active_share: HashMap<StrategyKind, f64> = HashMap::new();

        for &kind in StrategyKind::all() {
            let mut per_symbol_positions = Vec::new();
            let mut per_symbol_pnl = Vec::new();
            for &symbol in symbols {
                let df = data_cache.get(symbol).context("missing symbol data")?;
                let signal = signal_for_strategy(df, kind)?;
                let position = expand_to_position_series(&signal, len.min(df.height()));
                let pnl = daily_pnl_from_positions(df, &position)?;
                per_symbol_positions.push(position);
                per_symbol_pnl.push(pnl);
            }

            let portfolio_position = average_series(&per_symbol_positions, len);
            let portfolio_pnl = average_series(&per_symbol_pnl, len);
            let active_share =
                portfolio_position.iter().filter(|x| x.abs() > 1e-9).count() as f64 / len as f64;

            strat_active_share.insert(kind, active_share);
            strat_series.insert(kind, portfolio_pnl);
        }

        for &kind in StrategyKind::all() {
            println!(
                "{:<14} active_share {:>5.1}%",
                kind.name(),
                strat_active_share[&kind] * 100.0
            );
        }

        for i in 0..StrategyKind::all().len() {
            for j in i + 1..StrategyKind::all().len() {
                let a = StrategyKind::all()[i];
                let b = StrategyKind::all()[j];
                let series_a = &strat_series[&a];
                let series_b = &strat_series[&b];

                let mut both_active = 0usize;
                let mut same_sign = 0usize;
                let mut opposite_sign = 0usize;
                let mut either_active = 0usize;
                let mut abs_diff_sum = 0.0;

                for k in 0..series_a.len().min(series_b.len()) {
                    let sa = sign(series_a[k]);
                    let sb = sign(series_b[k]);
                    if sa != 0 || sb != 0 {
                        either_active += 1;
                    }
                    if sa != 0 && sb != 0 {
                        both_active += 1;
                        if sa == sb {
                            same_sign += 1;
                        } else {
                            opposite_sign += 1;
                        }
                    }
                    abs_diff_sum += (series_a[k] - series_b[k]).abs();
                }

                let corr = pearson_corr(series_a, series_b).unwrap_or(0.0);
                println!(
                    "{:<14} vs {:<14} corr {:>5.2} | same {:>5.1}% | opp {:>5.1}% | union-active {:>5.1}%",
                    a.name(),
                    b.name(),
                    corr,
                    pct(same_sign, both_active),
                    pct(opposite_sign, both_active),
                    pct(either_active, series_a.len().min(series_b.len())),
                );

                let agg = pair_aggs.entry((a, b)).or_default();
                agg.days_both_active += both_active;
                agg.days_same_sign += same_sign;
                agg.days_opposite_sign += opposite_sign;
                agg.days_either_active += either_active;
                agg.corr_sum += corr;
                agg.corr_count += 1;
                agg.avg_abs_diff_sum += abs_diff_sum / series_a.len().max(1) as f64;
                agg.universes += 1;
            }
        }
        println!();
    }

    println!("=== AGGREGATE PAIR SUMMARY ===");
    let mut pairs: Vec<_> = pair_aggs.into_iter().collect();
    pairs.sort_by(|a, b| {
        let ac = a.1.corr_sum / a.1.corr_count.max(1) as f64;
        let bc = b.1.corr_sum / b.1.corr_count.max(1) as f64;
        bc.partial_cmp(&ac).unwrap()
    });

    for ((a, b), agg) in pairs {
        println!(
            "{:<14} vs {:<14} avg_corr {:>5.2} | same {:>5.1}% | opp {:>5.1}% | union-active {:>5.1}% | avg_abs_diff {:>7.4}",
            a.name(),
            b.name(),
            agg.corr_sum / agg.corr_count.max(1) as f64,
            pct(agg.days_same_sign, agg.days_both_active),
            pct(agg.days_opposite_sign, agg.days_both_active),
            pct(agg.days_either_active, agg.universes * min_len_placeholder()),
            agg.avg_abs_diff_sum / agg.universes.max(1) as f64,
        );
    }

    println!("\nInterpretation:");
    println!("- High same-sign + high correlation => mostly the same latent engine.");
    println!(
        "- Lower correlation with persistent activity => more credible diversification value."
    );
    println!("- This is a structure audit, not a winner table.");
    Ok(())
}

fn signal_for_strategy(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::AdMomentum => generate_ad_momentum_signals(df, AD_PERIOD),
        StrategyKind::CTRend => generate_ctrend_signals(df),
        StrategyKind::MacdRegime => generate_macd_regime_signals(df),
        StrategyKind::TurtleMACD => generate_turtle_macd_signals(df, TURTLE_PERIOD),
    }
}

fn generate_ctrend_signals(df: &DataFrame) -> Result<Vec<i32>> {
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

    let mut out = vec![0i32; n];
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
        if score > 0.35 {
            out[i] = 1;
        } else if score < -0.35 {
            out[i] = -1;
        }
    }
    Ok(out)
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

fn generate_turtle_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let macd = df.column("macd")?.f64()?;
    let macd_signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0i32; df.height()];
    for i in period..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let macd_now = macd.get(i).unwrap_or(0.0);
        let macd_sig_now = macd_signal.get(i).unwrap_or(0.0);
        let macd_dir = if macd_now > macd_sig_now {
            1
        } else if macd_now < macd_sig_now {
            -1
        } else {
            0
        };
        let highest = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let turtle = if price > highest {
            1
        } else if price < lowest {
            -1
        } else {
            0
        };
        if turtle != 0 && turtle == macd_dir {
            out[i] = turtle;
        }
    }
    Ok(out)
}

fn expand_to_position_series(signals: &[i32], len: usize) -> Vec<f64> {
    let mut pos = vec![0.0; len];
    let mut t = 1usize;
    while t + HOLD_BARS < len && t < signals.len() {
        let signal = signals[t - 1] as f64;
        if signal.abs() < 1e-9 {
            t += 1;
            continue;
        }
        for k in t..(t + HOLD_BARS).min(len) {
            pos[k] += signal;
        }
        t += HOLD_BARS;
    }
    pos
}

fn daily_pnl_from_positions(df: &DataFrame, positions: &[f64]) -> Result<Vec<f64>> {
    let open = df.column("open")?.f64()?;
    let len = positions.len().min(df.height());
    let mut pnl = vec![0.0; len];
    for i in 1..len {
        let prev = open.get(i - 1).unwrap_or(0.0);
        let now = open.get(i).unwrap_or(0.0);
        if prev > 0.0 && now > 0.0 {
            let ret = (now / prev) - 1.0;
            pnl[i] = positions[i - 1] * ret;
        }
    }
    Ok(pnl)
}

fn average_series(series_list: &[Vec<f64>], len: usize) -> Vec<f64> {
    let mut out = vec![0.0; len];
    if series_list.is_empty() {
        return out;
    }
    for series in series_list {
        for i in 0..len.min(series.len()) {
            out[i] += series[i];
        }
    }
    for v in &mut out {
        *v /= series_list.len() as f64;
    }
    out
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

fn min_symbol_len(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    symbols
        .iter()
        .map(|symbol| {
            data_cache
                .get(*symbol)
                .map(|df| df.height())
                .with_context(|| format!("missing df for {}", symbol))
        })
        .collect::<Result<Vec<_>>>()
        .map(|lens| lens.into_iter().min().unwrap_or(0))
}

fn pearson_corr(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len().min(b.len());
    if n < 2 {
        return None;
    }
    let mean_a = a[..n].iter().sum::<f64>() / n as f64;
    let mean_b = b[..n].iter().sum::<f64>() / n as f64;
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
        None
    } else {
        Some(cov / (var_a.sqrt() * var_b.sqrt()))
    }
}

fn sign(x: f64) -> i32 {
    if x > 1e-12 {
        1
    } else if x < -1e-12 {
        -1
    } else {
        0
    }
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        100.0 * n as f64 / d as f64
    }
}

fn min_len_placeholder() -> usize {
    3000
}

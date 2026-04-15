//! Attribution / structure audit for the first cross-impact breadth result.
//!
//! Goal:
//! - explain whether the `CrossImpact slow` result is driven by a few durable peer->target links
//!   or just whole-basket averaging noise
//! - focus on the harsher old-guard basket where the first benchmark showed the family could win
//! - keep the same fair daily assumptions: signal at close, next-open entry, fixed 21-bar hold,
//!   0.1% taker each side

use anyhow::{Context, Result};
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::{HashMap, HashSet};

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const CS_LOOKBACK: usize = 63;
const WARMUP_BARS: usize = 200;

const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
    "EOSUSDT", "BCHUSDT",
];

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
    ),
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
        &["ETHUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT", "LTCUSDT"],
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

#[derive(Clone)]
struct CrossImpactInputs {
    target_open: Vec<f64>,
    target_close: Vec<f64>,
    peer_closes: HashMap<String, Vec<f64>>,
}

#[derive(Default, Clone, Debug)]
struct PairStats {
    result: StrategyResult,
    long_trades: usize,
    short_trades: usize,
}

#[derive(Default, Clone, Debug)]
struct SymbolAttribution {
    result: StrategyResult,
    peer_stats: HashMap<String, PairStats>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== CROSS-IMPACT ATTRIBUTION ===\n");
    println!("Focus strategy: CrossImpact slow");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side\n", TAKER_FEE * 100.0);

    let loader = DataLoader::new(None, None);
    let mut base_cache = HashMap::<String, DataFrame>::new();
    for &symbol in LOAD_SYMBOLS {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", enriched.height());
        base_cache.insert(symbol.to_string(), enriched);
    }

    let mut cs_map = base_cache.clone();
    compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;

    for &(universe_name, symbols) in UNIVERSES {
        println!(
            "\n=== Universe: {} ({}) ===",
            universe_name,
            symbols.join(", ")
        );
        let inputs = build_cross_impact_inputs(&cs_map, symbols)?;
        let mut universe_total = StrategyResult::default();
        let mut pair_totals = HashMap::<(String, String), PairStats>::new();

        for &target in symbols {
            let df = cs_map
                .get(target)
                .with_context(|| format!("missing df for {}", target))?;
            let target_inputs = inputs
                .get(target)
                .with_context(|| format!("missing inputs for {}", target))?;
            let attr = attribute_symbol(df, target_inputs)?;
            universe_total.add_assign(&attr.result);

            println!(
                "{:<10} {:>8.1}% {:>4} trades {:>5.1}% win",
                target,
                attr.result.total_return_pct,
                attr.result.trades,
                attr.result.win_rate() * 100.0,
            );

            let mut top_peers: Vec<_> = attr.peer_stats.iter().collect();
            top_peers.sort_by(|a, b| {
                b.1.result
                    .total_return_pct
                    .partial_cmp(&a.1.result.total_return_pct)
                    .unwrap()
                    .then_with(|| b.1.result.trades.cmp(&a.1.result.trades))
            });
            for (peer, stats) in top_peers.into_iter().take(3) {
                println!(
                    "    -> {:<8} {:>8.1}% {:>4} trades {:>5.1}% win (L{} / S{})",
                    peer,
                    stats.result.total_return_pct,
                    stats.result.trades,
                    stats.result.win_rate() * 100.0,
                    stats.long_trades,
                    stats.short_trades,
                );
                pair_totals
                    .entry((peer.clone(), target.to_string()))
                    .or_default()
                    .result
                    .add_assign(&stats.result);
                let entry = pair_totals
                    .get_mut(&(peer.clone(), target.to_string()))
                    .unwrap();
                entry.long_trades += stats.long_trades;
                entry.short_trades += stats.short_trades;
            }
        }

        println!(
            "\nUniverse total: {:>8.1}% {:>4} trades {:>5.1}% win",
            universe_total.total_return_pct,
            universe_total.trades,
            universe_total.win_rate() * 100.0,
        );

        let mut pair_rows: Vec<_> = pair_totals.into_iter().collect();
        pair_rows.sort_by(|a, b| {
            b.1.result
                .total_return_pct
                .partial_cmp(&a.1.result.total_return_pct)
                .unwrap()
                .then_with(|| b.1.result.trades.cmp(&a.1.result.trades))
        });

        println!("Top peer -> target links:");
        for ((peer, target), stats) in pair_rows.into_iter().take(8) {
            println!(
                "- {:<8} -> {:<8} {:>8.1}% {:>4} trades {:>5.1}% win (L{} / S{})",
                peer,
                target,
                stats.result.total_return_pct,
                stats.result.trades,
                stats.result.win_rate() * 100.0,
                stats.long_trades,
                stats.short_trades,
            );
        }
    }

    Ok(())
}

fn build_cross_impact_inputs(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<HashMap<String, CrossImpactInputs>> {
    let include: HashSet<&str> = symbols.iter().copied().collect();
    let mut closes = HashMap::<String, Vec<f64>>::new();
    let mut opens = HashMap::<String, Vec<f64>>::new();
    let n = min_rows(data_cache, symbols)?;

    for &symbol in symbols {
        let df = data_cache.get(symbol).unwrap();
        closes.insert(
            symbol.to_string(),
            df.column("close")?
                .f64()?
                .into_iter()
                .take(n)
                .map(|v| v.unwrap_or(0.0))
                .collect(),
        );
        opens.insert(
            symbol.to_string(),
            df.column("open")?
                .f64()?
                .into_iter()
                .take(n)
                .map(|v| v.unwrap_or(0.0))
                .collect(),
        );
    }

    let mut out = HashMap::new();
    for &symbol in symbols {
        let mut peer_closes = HashMap::new();
        for &peer in symbols {
            if peer != symbol && include.contains(peer) {
                peer_closes.insert(peer.to_string(), closes.get(peer).unwrap().clone());
            }
        }
        out.insert(
            symbol.to_string(),
            CrossImpactInputs {
                target_open: opens.get(symbol).unwrap().clone(),
                target_close: closes.get(symbol).unwrap().clone(),
                peer_closes,
            },
        );
    }
    Ok(out)
}

fn min_rows(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    symbols
        .iter()
        .map(|symbol| data_cache.get(*symbol).map(|df| df.height()).unwrap_or(0))
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty symbol set"))
}

fn attribute_symbol(df: &DataFrame, inputs: &CrossImpactInputs) -> Result<SymbolAttribution> {
    let signals = generate_cross_impact_signals(inputs, 3, 0.040, 0.015);
    let n = df
        .height()
        .min(inputs.target_open.len())
        .min(inputs.target_close.len());
    let mut out = SymbolAttribution::default();

    let mut i = WARMUP_BARS;
    while i + HOLD_BARS + 1 < n {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }

        let entry_idx = i + 1;
        let exit_idx = i + 1 + HOLD_BARS;
        let entry = inputs.target_open.get(entry_idx).copied().unwrap_or(0.0);
        let exit = inputs.target_open.get(exit_idx).copied().unwrap_or(0.0);
        if entry <= 0.0 || exit <= 0.0 {
            i += 1;
            continue;
        }

        let gross = if signal > 0 {
            (exit / entry - 1.0) * 100.0
        } else {
            (entry / exit - 1.0) * 100.0
        };
        let net = gross - 2.0 * TAKER_FEE * 100.0;
        out.result.add_trade(net);

        for (peer, closes) in &inputs.peer_closes {
            if let Some(peer_ret) = pct_return(closes, i, 3) {
                if (signal > 0 && peer_ret > 0.0) || (signal < 0 && peer_ret < 0.0) {
                    let stats = out.peer_stats.entry(peer.clone()).or_default();
                    stats.result.add_trade(net);
                    if signal > 0 {
                        stats.long_trades += 1;
                    } else {
                        stats.short_trades += 1;
                    }
                }
            }
        }

        i = exit_idx;
    }

    Ok(out)
}

fn generate_cross_impact_signals(
    inputs: &CrossImpactInputs,
    peer_lookback: usize,
    peer_threshold: f64,
    self_cap: f64,
) -> Vec<i32> {
    let n = inputs.target_close.len();
    let mut out = vec![0; n];

    for i in peer_lookback..n {
        let self_ret = pct_return(&inputs.target_close, i, peer_lookback).unwrap_or(0.0);
        if self_ret.abs() > self_cap {
            continue;
        }

        let mut peer_sum = 0.0;
        let mut peer_count = 0usize;
        let mut same_sign = 0i32;
        for closes in inputs.peer_closes.values() {
            if let Some(ret) = pct_return(closes, i, peer_lookback) {
                peer_sum += ret;
                peer_count += 1;
                if ret > 0.0 {
                    same_sign += 1;
                } else if ret < 0.0 {
                    same_sign -= 1;
                }
            }
        }
        if peer_count < 2 {
            continue;
        }

        let peer_avg = peer_sum / peer_count as f64;
        let raw_signal = if peer_avg >= peer_threshold && same_sign >= 2 {
            1
        } else if peer_avg <= -peer_threshold && same_sign <= -2 {
            -1
        } else {
            0
        };
        out[i] = raw_signal;
    }

    out
}

fn pct_return(values: &[f64], i: usize, lookback: usize) -> Option<f64> {
    if i < lookback {
        return None;
    }
    let prev = values.get(i - lookback).copied().unwrap_or(0.0);
    let now = values.get(i).copied().unwrap_or(0.0);
    if prev <= 0.0 || now <= 0.0 {
        None
    } else {
        Some(now / prev - 1.0)
    }
}

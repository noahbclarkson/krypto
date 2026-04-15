//! Test a richer beta-neutral funding carry spread that includes realized funding cashflows.
//!
//! This is a stricter follow-up to `funding_spread_carry.rs`, which only measured price
//! convergence after entering the spread. Here we explicitly add the funding payments that
//! a perp long/short pair would earn or pay during the hold.
//!
//! Idea:
//! - at bar close, rank assets by funding-rate z-score
//! - long the most negatively funded perp and short the most positively funded perp
//! - enter both legs at next open, hold a fixed number of 4h bars
//! - include BOTH price PnL and realized funding cashflows between entry and exit
//!
//! If the combined result is still weak, then the simple carry-dispersion family probably
//! needs a more sophisticated construction than “buy the most negative / short the most positive”.

use anyhow::{anyhow, Result};
use krypto::data::funding_rate::FundingRateLoader;
use krypto::data::{align_to_ohlcv, DataLoader};
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::{BTreeSet, HashMap};

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT"];
const INTERVAL: &str = "4h";
const CANDLES: u32 = 3000;
const Z_WINDOW: usize = 90;
const FEE_PER_SIDE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 8;
const WARMUP_BARS: usize = 120;
const RESAMPLE_BLOCKS: usize = 6;

#[derive(Clone)]
struct AssetSeries {
    times: Vec<i64>,
    opens: Vec<f64>,
    funding_z: Vec<f64>,
    funding_events: Vec<(i64, f64)>,
}

#[derive(Clone, Copy, Debug)]
struct Config {
    entry_z: f64,
    min_spread_z: f64,
    hold_bars: usize,
}

#[derive(Default, Clone, Debug)]
struct EvalResult {
    total_return_pct: f64,
    price_return_pct: f64,
    funding_return_pct: f64,
    trades: usize,
    wins: usize,
    avg_trade_pct: f64,
}

impl EvalResult {
    fn win_rate(&self) -> f64 {
        if self.trades == 0 {
            0.0
        } else {
            self.wins as f64 / self.trades as f64
        }
    }
}

fn main_configs() -> Vec<Config> {
    let mut out = Vec::new();
    for &entry_z in &[1.0, 1.5, 2.0] {
        for &min_spread_z in &[2.0, 2.5, 3.0] {
            for &hold_bars in &[1usize, 3usize, 6usize] {
                out.push(Config {
                    entry_z,
                    min_spread_z,
                    hold_bars,
                });
            }
        }
    }
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== REALIZED FUNDING CARRY SPREAD TEST ===\n");
    println!("Universe: {:?}", SYMBOLS);
    println!("Interval: {}", INTERVAL);
    println!("Execution: signal at close, long most negative funding / short most positive funding at next open");
    println!(
        "PnL model: price convergence + realized funding cashflows - {:.1}% taker each side",
        FEE_PER_SIDE * 100.0
    );
    println!(
        "Grid: entry_z in {{1.0,1.5,2.0}}, spread_z in {{2.0,2.5,3.0}}, hold in {{1,3,6}} bars\n"
    );

    let loader = DataLoader::new(None, None);
    let funding_loader = FundingRateLoader::with_cache_dir("examples/funding_cache");

    let mut raw_maps = Vec::new();
    for &symbol in SYMBOLS {
        print!("Loading {}... ", symbol);
        let price_df = loader.fetch_with_cache(symbol, INTERVAL, CANDLES).await?;
        let tech_df = FeatureEngine::add_technicals(&price_df, None)?;
        let funding_df = funding_loader.fetch(symbol, None, None).await?;
        let aligned = align_to_ohlcv(&tech_df, &funding_df, Z_WINDOW)?;
        println!(
            "{} bars, {} funding rows",
            aligned.height(),
            funding_df.height()
        );
        raw_maps.push((
            symbol.to_string(),
            dataframe_to_asset_rows(&aligned, &funding_df)?,
        ));
    }

    let merged = merge_common_timestamps(&raw_maps)?;
    let bars = merged
        .values()
        .next()
        .ok_or_else(|| anyhow!("empty merged data"))?
        .opens
        .len();
    println!("Common aligned bars across all assets: {}\n", bars);

    let quarter_windows = quarter_windows(bars);
    let resample_sets = cpcv_style_windows(bars);

    let mut rows = Vec::new();
    for cfg in main_configs() {
        let full = evaluate(&merged, cfg, &[(WARMUP_BARS, bars)])?;
        let mut wf_passed = 0usize;
        for &(start, end) in &quarter_windows {
            let eval = evaluate(&merged, cfg, &[(start, end)])?;
            if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                wf_passed += 1;
            }
        }
        let mut rs_passed = 0usize;
        for windows in &resample_sets {
            let eval = evaluate(&merged, cfg, windows)?;
            if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                rs_passed += 1;
            }
        }
        rows.push((cfg, full, wf_passed, rs_passed));
    }

    rows.sort_by(|a, b| {
        b.3.cmp(&a.3).then_with(|| b.2.cmp(&a.2)).then_with(|| {
            b.1.total_return_pct
                .partial_cmp(&a.1.total_return_pct)
                .unwrap()
        })
    });

    println!(
        "{:<18} {:<10} {:>10} {:>10} {:>10} {:>8} {:>8} {:>12}",
        "Config", "Hold", "Combined%", "Price%", "Funding%", "Trades", "Win%", "Resamples"
    );
    for (cfg, full, _, rs_passed) in &rows {
        println!(
            "z>{:.1}/Δ>{:.1} {:>4} {:>10.1} {:>10.1} {:>10.1} {:>8} {:>7.1}% {:>6}/{}",
            cfg.entry_z,
            cfg.min_spread_z,
            cfg.hold_bars,
            full.total_return_pct,
            full.price_return_pct,
            full.funding_return_pct,
            full.trades,
            full.win_rate() * 100.0,
            rs_passed,
            resample_sets.len(),
        );
    }

    if let Some((best_cfg, best_full, best_wf, best_rs)) = rows.first() {
        println!("\nBest by chronology-first ranking:");
        println!(
            "- entry_z {:.1}, spread_z {:.1}, hold {} bars -> combined {:.1}%, price {:.1}%, funding {:.1}%, trades {}, win rate {:.1}%, avg trade {:.3}%, WF {}/{}, resamples {}/{}",
            best_cfg.entry_z,
            best_cfg.min_spread_z,
            best_cfg.hold_bars,
            best_full.total_return_pct,
            best_full.price_return_pct,
            best_full.funding_return_pct,
            best_full.trades,
            best_full.win_rate() * 100.0,
            best_full.avg_trade_pct,
            best_wf,
            quarter_windows.len(),
            best_rs,
            resample_sets.len(),
        );
    }

    println!("\nInterpretation:");
    println!("- Positive funding contribution with weak total PnL would mean the carry exists, but price convergence timing is poor.");
    println!("- Weak funding contribution as well would mean this simple cross-asset carry expression is not extracting much actual carry.");
    println!("- Even a positive result here would still be a research lead, not a production-neutral carry engine.");

    Ok(())
}

fn dataframe_to_asset_rows(
    df: &DataFrame,
    funding_df: &DataFrame,
) -> Result<(Vec<(i64, f64, f64)>, Vec<(i64, f64)>)> {
    let times = df.column("time")?.cast(&DataType::Int64)?;
    let opens = df.column("open")?.f64()?;
    let funding_z = df.column("funding_rate_z")?.f64()?;

    let mut bar_rows = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        bar_rows.push((
            times.i64()?.get(i).unwrap_or(0),
            opens.get(i).unwrap_or(0.0),
            funding_z.get(i).unwrap_or(0.0),
        ));
    }

    let fund_times = funding_df.column("time")?.cast(&DataType::Int64)?;
    let fund_rates = funding_df.column("funding_rate")?.f64()?;
    let mut funding_events = Vec::with_capacity(funding_df.height());
    for i in 0..funding_df.height() {
        funding_events.push((
            fund_times.i64()?.get(i).unwrap_or(0),
            fund_rates.get(i).unwrap_or(0.0),
        ));
    }

    Ok((bar_rows, funding_events))
}

fn merge_common_timestamps(
    raw_maps: &[(String, (Vec<(i64, f64, f64)>, Vec<(i64, f64)>))],
) -> Result<HashMap<String, AssetSeries>> {
    let mut common: Option<BTreeSet<i64>> = None;
    for (_, (rows, _)) in raw_maps {
        let set: BTreeSet<i64> = rows.iter().map(|(t, _, _)| *t).collect();
        common = Some(match common {
            None => set,
            Some(prev) => prev.intersection(&set).copied().collect(),
        });
    }
    let common = common.ok_or_else(|| anyhow!("no symbols loaded"))?;

    let mut merged = HashMap::new();
    for (symbol, (rows, funding_events)) in raw_maps {
        let map: HashMap<i64, (f64, f64)> = rows.iter().map(|(t, o, z)| (*t, (*o, *z))).collect();
        let mut times = Vec::with_capacity(common.len());
        let mut opens = Vec::with_capacity(common.len());
        let mut funding_z = Vec::with_capacity(common.len());
        for t in &common {
            let (open, z) = map
                .get(t)
                .ok_or_else(|| anyhow!("missing {} timestamp {}", symbol, t))?;
            times.push(*t);
            opens.push(*open);
            funding_z.push(*z);
        }
        merged.insert(
            symbol.clone(),
            AssetSeries {
                times,
                opens,
                funding_z,
                funding_events: funding_events.clone(),
            },
        );
    }
    Ok(merged)
}

fn evaluate(
    data: &HashMap<String, AssetSeries>,
    cfg: Config,
    windows: &[(usize, usize)],
) -> Result<EvalResult> {
    let bars = data
        .values()
        .next()
        .ok_or_else(|| anyhow!("empty data"))?
        .opens
        .len();
    let mut allowed = vec![false; bars];
    for &(start, end) in windows {
        let s = start.min(bars);
        let e = end.min(bars);
        for i in s..e {
            allowed[i] = true;
        }
    }

    let mut combined_returns = Vec::new();
    let mut price_returns = Vec::new();
    let mut funding_returns = Vec::new();

    let mut i = WARMUP_BARS;
    while i + cfg.hold_bars + 1 < bars {
        if !allowed[i] {
            i += 1;
            continue;
        }

        let mut ranked: Vec<(&str, f64)> = SYMBOLS
            .iter()
            .map(|s| (*s, data.get(*s).unwrap().funding_z[i]))
            .collect();
        ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        let (long_symbol, low_z) = ranked[0];
        let (short_symbol, high_z) = ranked[ranked.len() - 1];
        let spread_z = high_z - low_z;

        if low_z <= -cfg.entry_z && high_z >= cfg.entry_z && spread_z >= cfg.min_spread_z {
            let exit_idx = i + cfg.hold_bars;
            let inside_window = (i + 1..=exit_idx).all(|j| allowed[j]);
            if !inside_window {
                i += 1;
                continue;
            }

            let long_asset = data.get(long_symbol).unwrap();
            let short_asset = data.get(short_symbol).unwrap();

            let entry_idx = i + 1;
            let entry_time = long_asset.times[entry_idx];
            let exit_time = long_asset.times[exit_idx];

            let long_entry = long_asset.opens[entry_idx];
            let long_exit = long_asset.opens[exit_idx];
            let short_entry = short_asset.opens[entry_idx];
            let short_exit = short_asset.opens[exit_idx];

            if long_entry > 0.0 && long_exit > 0.0 && short_entry > 0.0 && short_exit > 0.0 {
                let long_price_ret = 0.5 * ((long_exit / long_entry) - 1.0) - FEE_PER_SIDE;
                let short_price_ret = 0.5 * ((short_entry / short_exit) - 1.0) - FEE_PER_SIDE;
                let price_ret = long_price_ret + short_price_ret;

                let long_funding_ret = realized_funding_return(
                    &long_asset.funding_events,
                    entry_time,
                    exit_time,
                    true,
                );
                let short_funding_ret = realized_funding_return(
                    &short_asset.funding_events,
                    entry_time,
                    exit_time,
                    false,
                );
                let funding_ret = 0.5 * long_funding_ret + 0.5 * short_funding_ret;

                combined_returns.push(price_ret + funding_ret);
                price_returns.push(price_ret);
                funding_returns.push(funding_ret);
                i = exit_idx;
                continue;
            }
        }

        i += 1;
    }

    summarize_returns(&combined_returns, &price_returns, &funding_returns)
}

fn realized_funding_return(
    events: &[(i64, f64)],
    entry_time: i64,
    exit_time: i64,
    is_long: bool,
) -> f64 {
    let mut total = 0.0;
    for &(ts, rate) in events {
        if ts > entry_time && ts <= exit_time {
            total += if is_long { -rate } else { rate };
        }
    }
    total
}

fn summarize_returns(combined: &[f64], price: &[f64], funding: &[f64]) -> Result<EvalResult> {
    if combined.len() != price.len() || combined.len() != funding.len() {
        return Err(anyhow!("return vectors length mismatch"));
    }

    let mut result = EvalResult::default();
    result.trades = combined.len();
    if result.trades == 0 {
        return Ok(result);
    }

    let mut combined_equity = 1.0f64;
    let mut price_equity = 1.0f64;
    let mut funding_equity = 1.0f64;
    let mut wins = 0usize;
    let mut sum_trade_pct = 0.0;

    for idx in 0..combined.len() {
        let c = combined[idx];
        let p = price[idx];
        let f = funding[idx];
        if c > 0.0 {
            wins += 1;
        }
        combined_equity *= 1.0 + c;
        price_equity *= 1.0 + p;
        funding_equity *= 1.0 + f;
        sum_trade_pct += c * 100.0;
    }

    result.total_return_pct = (combined_equity - 1.0) * 100.0;
    result.price_return_pct = (price_equity - 1.0) * 100.0;
    result.funding_return_pct = (funding_equity - 1.0) * 100.0;
    result.wins = wins;
    result.avg_trade_pct = sum_trade_pct / result.trades as f64;
    Ok(result)
}

fn quarter_windows(bars: usize) -> Vec<(usize, usize)> {
    let usable = bars.saturating_sub(WARMUP_BARS);
    let chunk = usable / 4;
    let mut windows = Vec::new();
    for q in 0..4 {
        let start = WARMUP_BARS + q * chunk;
        let end = if q == 3 {
            bars
        } else {
            WARMUP_BARS + (q + 1) * chunk
        };
        windows.push((start, end));
    }
    windows
}

fn cpcv_style_windows(bars: usize) -> Vec<Vec<(usize, usize)>> {
    let usable = bars.saturating_sub(WARMUP_BARS);
    let block = usable / RESAMPLE_BLOCKS;
    let mut blocks = Vec::new();
    for i in 0..RESAMPLE_BLOCKS {
        let start = WARMUP_BARS + i * block;
        let end = if i == RESAMPLE_BLOCKS - 1 {
            bars
        } else {
            WARMUP_BARS + (i + 1) * block
        };
        blocks.push((start, end));
    }

    let mut windows = Vec::new();
    for i in 0..RESAMPLE_BLOCKS {
        for j in i + 1..RESAMPLE_BLOCKS {
            windows.push(vec![blocks[i], blocks[j]]);
        }
    }
    windows
}

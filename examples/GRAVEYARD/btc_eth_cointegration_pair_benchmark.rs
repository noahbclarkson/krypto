//! BTC-ETH cointegration pair benchmark.
//!
//! Goal:
//! - open a genuinely new Track C lane: mean-reversion via BTC/ETH spread equilibrium
//! - keep the same daily honesty bar used across the rest of the lab
//!
//! Construction:
//! - align BTCUSDT and ETHUSDT daily bars on common timestamps
//! - estimate a rolling hedge ratio from log-price covariance / variance
//! - compute the spread = log(ETH) - beta * log(BTC)
//! - z-score the spread on a rolling window
//! - when z is extreme, trade mean-reversion in a beta-scaled pair
//!
//! Execution assumptions:
//! - signal at close using only information available through that bar
//! - enter both legs at next open
//! - exit at next open when z mean-reverts through exit threshold or max hold is hit
//! - 0.1% taker per side; both legs included

use anyhow::{anyhow, Result};
use krypto::data::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::{BTreeSet, HashMap};

const BTC: &str = "BTCUSDT";
const ETH: &str = "ETHUSDT";
const INTERVAL: &str = "1d";
const CANDLES: u32 = 3000;
const FEE_PER_SIDE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 12;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;

#[derive(Clone, Copy, Debug)]
struct Config {
    beta_window: usize,
    z_window: usize,
    entry_z: f64,
    exit_z: f64,
    max_hold: usize,
}

#[derive(Default, Clone, Debug)]
struct EvalResult {
    total_return_pct: f64,
    trades: usize,
    wins: usize,
    avg_trade_pct: f64,
    max_drawdown_pct: f64,
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

#[derive(Clone)]
struct AssetSeries {
    opens: Vec<f64>,
    closes: Vec<f64>,
}

fn main_configs() -> Vec<Config> {
    let mut out = Vec::new();
    for &beta_window in &[63usize, 126usize, 252usize] {
        for &z_window in &[42usize, 63usize, 126usize] {
            for &entry_z in &[1.5, 2.0, 2.5] {
                for &exit_z in &[0.0, 0.5] {
                    for &max_hold in &[5usize, 10usize, 21usize] {
                        if z_window <= beta_window {
                            out.push(Config {
                                beta_window,
                                z_window,
                                entry_z,
                                exit_z,
                                max_hold,
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== BTC-ETH COINTEGRATION PAIR BENCHMARK ===\n");
    println!("Symbols: {BTC}, {ETH}");
    println!("Interval: {INTERVAL}");
    println!("Signal: rolling log-price spread z-score with rolling hedge beta");
    println!("Execution: signal at close, pair entry next open, mean-reversion exit or max hold");
    println!(
        "Cost model: {:.1}% taker per side with both legs included\n",
        FEE_PER_SIDE * 100.0
    );

    let loader = DataLoader::new(None, None);
    let btc_df = FeatureEngine::add_technicals(
        &loader.fetch_with_cache(BTC, INTERVAL, CANDLES).await?,
        None,
    )?;
    let eth_df = FeatureEngine::add_technicals(
        &loader.fetch_with_cache(ETH, INTERVAL, CANDLES).await?,
        None,
    )?;

    let merged = merge_common_timestamps(&[
        (BTC.to_string(), dataframe_to_rows(&btc_df)?),
        (ETH.to_string(), dataframe_to_rows(&eth_df)?),
    ])?;
    let btc = merged
        .get(BTC)
        .ok_or_else(|| anyhow!("missing BTC series"))?;
    let eth = merged
        .get(ETH)
        .ok_or_else(|| anyhow!("missing ETH series"))?;
    let bars = btc.opens.len();
    println!("Common aligned bars: {}\n", bars);

    let quarter_windows = quarter_windows(bars);
    let resample_sets = cpcv_style_windows(bars);

    let mut rows = Vec::new();
    for cfg in main_configs() {
        let warmup = (cfg.beta_window.max(cfg.z_window) + 5).max(252);
        if warmup + cfg.max_hold + 2 >= bars {
            continue;
        }
        let full = evaluate(btc, eth, cfg, &[(warmup, bars)])?;
        let mut wf_passed = 0usize;
        for &(start, end) in &quarter_windows {
            let eval = evaluate(btc, eth, cfg, &[(start.max(warmup), end)])?;
            if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                wf_passed += 1;
            }
        }
        let mut rs_passed = 0usize;
        for windows in &resample_sets {
            let adjusted: Vec<_> = windows
                .iter()
                .filter_map(|&(s, e)| {
                    let s2 = s.max(warmup);
                    (s2 < e).then_some((s2, e))
                })
                .collect();
            let eval = evaluate(btc, eth, cfg, &adjusted)?;
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
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    println!(
        "{:<28} {:>10} {:>8} {:>8} {:>8} {:>10} {:>12}",
        "Config", "Return%", "Trades", "Win%", "MaxDD", "WF", "Resamples"
    );
    for (cfg, full, wf_passed, rs_passed) in rows.iter().take(18) {
        println!(
            "β{:>3}/z{:>3}/e{:.1}/x{:.1}/h{:>2} {:>10.1} {:>8} {:>7.1}% {:>7.1}% {:>4}/{} {:>6}/{}",
            cfg.beta_window,
            cfg.z_window,
            cfg.entry_z,
            cfg.exit_z,
            cfg.max_hold,
            full.total_return_pct,
            full.trades,
            full.win_rate() * 100.0,
            full.max_drawdown_pct,
            wf_passed,
            quarter_windows.len(),
            rs_passed,
            resample_sets.len(),
        );
    }

    if let Some((cfg, full, wf, rs)) = rows.first() {
        println!("\nBest by chronology-first ranking:");
        println!(
            "- beta_window {} | z_window {} | entry {:.1} | exit {:.1} | max_hold {}",
            cfg.beta_window, cfg.z_window, cfg.entry_z, cfg.exit_z, cfg.max_hold
        );
        println!(
            "- return {:.1}%, trades {}, win rate {:.1}%, avg trade {:.2}%, max DD {:.1}%, WF {}/{}, resamples {}/{}",
            full.total_return_pct,
            full.trades,
            full.win_rate() * 100.0,
            full.avg_trade_pct,
            full.max_drawdown_pct,
            wf,
            quarter_windows.len(),
            rs,
            resample_sets.len(),
        );
    }

    println!("\nInterpretation:");
    println!("- This is a first structural breadth test for the BTC/ETH mean-reversion lane, not a production stat-arb engine.");
    println!("- If chronology stays weak, cointegration is not yet strong enough under realistic fees and daily execution.");
    println!("- If one pocket survives honestly, the next step is attribution by market regime and integration with ETH-containing universes, not immediate promotion.");

    Ok(())
}

fn dataframe_to_rows(df: &DataFrame) -> Result<Vec<(i64, f64, f64)>> {
    let times = df.column("time")?.cast(&DataType::Int64)?;
    let opens = df.column("open")?.f64()?;
    let closes = df.column("close")?.f64()?;

    let mut rows = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        rows.push((
            times.i64()?.get(i).unwrap_or(0),
            opens.get(i).unwrap_or(0.0),
            closes.get(i).unwrap_or(0.0),
        ));
    }
    Ok(rows)
}

fn merge_common_timestamps(
    raw_maps: &[(String, Vec<(i64, f64, f64)>)],
) -> Result<HashMap<String, AssetSeries>> {
    let mut common: Option<BTreeSet<i64>> = None;
    for (_, rows) in raw_maps {
        let set: BTreeSet<i64> = rows.iter().map(|(t, _, _)| *t).collect();
        common = Some(match common {
            None => set,
            Some(prev) => prev.intersection(&set).copied().collect(),
        });
    }
    let common = common.ok_or_else(|| anyhow!("no symbols loaded"))?;

    let mut merged = HashMap::new();
    for (symbol, rows) in raw_maps {
        let map: HashMap<i64, (f64, f64)> = rows.iter().map(|(t, o, c)| (*t, (*o, *c))).collect();
        let mut opens = Vec::with_capacity(common.len());
        let mut closes = Vec::with_capacity(common.len());
        for t in &common {
            let (open, close) = map
                .get(t)
                .ok_or_else(|| anyhow!("missing {} timestamp {}", symbol, t))?;
            opens.push(*open);
            closes.push(*close);
        }
        merged.insert(symbol.clone(), AssetSeries { opens, closes });
    }
    Ok(merged)
}

fn evaluate(
    btc: &AssetSeries,
    eth: &AssetSeries,
    cfg: Config,
    windows: &[(usize, usize)],
) -> Result<EvalResult> {
    let bars = btc.opens.len();
    if bars != eth.opens.len() || bars != btc.closes.len() || bars != eth.closes.len() {
        return Err(anyhow!("series length mismatch"));
    }
    let mut allowed = vec![false; bars];
    for &(start, end) in windows {
        let s = start.min(bars);
        let e = end.min(bars);
        for slot in allowed.iter_mut().take(e).skip(s) {
            *slot = true;
        }
    }

    let btc_log: Vec<f64> = btc.closes.iter().map(|p| p.ln()).collect();
    let eth_log: Vec<f64> = eth.closes.iter().map(|p| p.ln()).collect();
    let beta = rolling_beta(&eth_log, &btc_log, cfg.beta_window);
    let spread: Vec<f64> = eth_log
        .iter()
        .zip(btc_log.iter())
        .zip(beta.iter())
        .map(|((&e, &b), &h)| e - h * b)
        .collect();
    let zscores = rolling_zscore(&spread, cfg.z_window);

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trade_returns = Vec::new();

    let mut i = cfg.beta_window.max(cfg.z_window).max(2);
    while i + cfg.max_hold + 1 < bars {
        if !allowed[i] {
            i += 1;
            continue;
        }

        let z = zscores[i];
        let direction = if z <= -cfg.entry_z {
            1.0 // long ETH, short BTC
        } else if z >= cfg.entry_z {
            -1.0 // short ETH, long BTC
        } else {
            i += 1;
            continue;
        };

        let beta_abs = beta[i].abs().clamp(0.1, 5.0);
        let w_eth = 1.0 / (1.0 + beta_abs);
        let w_btc = beta_abs / (1.0 + beta_abs);
        let entry = i + 1;
        let mut exit = None;
        let hard_end = (i + cfg.max_hold).min(bars - 2);
        for j in i + 1..=hard_end {
            if !allowed[j] {
                continue;
            }
            let z_now = zscores[j];
            if (direction > 0.0 && z_now >= -cfg.exit_z) || (direction < 0.0 && z_now <= cfg.exit_z)
            {
                exit = Some(j + 1);
                break;
            }
        }
        let exit = exit.unwrap_or(hard_end + 1);
        if exit >= bars || entry >= bars || exit <= entry {
            i += 1;
            continue;
        }

        let eth_ret = eth.opens[exit] / eth.opens[entry] - 1.0;
        let btc_ret = btc.opens[exit] / btc.opens[entry] - 1.0;
        let gross = if direction > 0.0 {
            w_eth * eth_ret - w_btc * btc_ret
        } else {
            -w_eth * eth_ret + w_btc * btc_ret
        };
        let net = gross - 2.0 * FEE_PER_SIDE;
        trade_returns.push(net);

        equity *= (1.0 + net).max(0.0);
        peak = peak.max(equity);
        let dd = if peak > 0.0 { 1.0 - equity / peak } else { 0.0 };
        max_dd = max_dd.max(dd);

        i = exit;
    }

    let trades = trade_returns.len();
    let wins = trade_returns.iter().filter(|&&r| r > 0.0).count();
    let total_return_pct = (equity - 1.0) * 100.0;
    let avg_trade_pct = if trades > 0 {
        trade_returns.iter().sum::<f64>() / trades as f64 * 100.0
    } else {
        0.0
    };

    Ok(EvalResult {
        total_return_pct,
        trades,
        wins,
        avg_trade_pct,
        max_drawdown_pct: max_dd * 100.0,
    })
}

fn rolling_beta(y: &[f64], x: &[f64], window: usize) -> Vec<f64> {
    let mut out = vec![1.0; y.len()];
    for i in 0..y.len() {
        if i + 1 < window {
            continue;
        }
        let start = i + 1 - window;
        let xs = &x[start..=i];
        let ys = &y[start..=i];
        let x_mean = xs.iter().sum::<f64>() / xs.len() as f64;
        let y_mean = ys.iter().sum::<f64>() / ys.len() as f64;
        let mut cov = 0.0;
        let mut var = 0.0;
        for (&xv, &yv) in xs.iter().zip(ys.iter()) {
            cov += (xv - x_mean) * (yv - y_mean);
            var += (xv - x_mean).powi(2);
        }
        out[i] = if var > 1e-12 {
            cov / var
        } else {
            out[i.saturating_sub(1)]
        };
    }
    out
}

fn rolling_zscore(values: &[f64], window: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in 0..values.len() {
        if i + 1 < window {
            continue;
        }
        let start = i + 1 - window;
        let slice = &values[start..=i];
        let mean = slice.iter().sum::<f64>() / slice.len() as f64;
        let var = slice.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / slice.len() as f64;
        let std = var.sqrt();
        out[i] = if std > 1e-12 {
            (values[i] - mean) / std
        } else {
            0.0
        };
    }
    out
}

fn quarter_windows(bars: usize) -> Vec<(usize, usize)> {
    let start = 252.min(bars);
    let usable = bars.saturating_sub(start);
    if usable < 8 {
        return vec![(start, bars)];
    }
    let chunk = (usable / 4).max(1);
    let mut out = Vec::new();
    let mut s = start;
    for idx in 0..4 {
        let e = if idx == 3 {
            bars
        } else {
            (s + chunk).min(bars)
        };
        if s < e {
            out.push((s, e));
        }
        s = e;
    }
    out
}

fn cpcv_style_windows(bars: usize) -> Vec<Vec<(usize, usize)>> {
    let start = 252.min(bars);
    let usable = bars.saturating_sub(start);
    if usable < RESAMPLE_BLOCKS {
        return vec![vec![(start, bars)]];
    }
    let block = (usable / RESAMPLE_BLOCKS).max(1);
    let mut blocks = Vec::new();
    let mut s = start;
    for idx in 0..RESAMPLE_BLOCKS {
        let e = if idx == RESAMPLE_BLOCKS - 1 {
            bars
        } else {
            (s + block).min(bars)
        };
        blocks.push((s, e));
        s = e;
    }

    let mut out = Vec::new();
    let total_masks = 1usize << RESAMPLE_BLOCKS;
    for mask in 0..total_masks {
        if mask.count_ones() as usize != RESAMPLE_TRAIN_BLOCKS {
            continue;
        }
        let mut windows = Vec::new();
        for (idx, &(bs, be)) in blocks.iter().enumerate() {
            if ((mask >> idx) & 1) == 1 {
                windows.push((bs, be));
            }
        }
        if !windows.is_empty() {
            out.push(windows);
        }
    }
    out
}

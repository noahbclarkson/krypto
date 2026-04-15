//! Walk-forward: RSI threshold mean-reversion on Base5
//!
//! Thesis: the project has zero surviving mean-reversion family after Bollinger died on
//! execution realism. This is a deliberately simple daily RSI threshold test under the
//! same fair assumptions used elsewhere: signal at close, next-open entry, 0.1% taker,
//! single-position book, realistic walk-forward windows.
//!
//! Rules:
//! - Long when RSI(14) < 20
//! - Short when RSI(14) > 80
//! - Exit when RSI crosses back through 50 OR after 21 bars
//! - If multiple symbols qualify, rank by distance from 50 (most stretched first)

use anyhow::Result;
use chrono::Utc;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const RSI_LOWER: f64 = 20.0;
const RSI_UPPER: f64 = 80.0;
const RSI_EXIT: f64 = 50.0;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;

#[derive(Clone)]
struct SymData {
    open: Vec<f64>,
    close: Vec<f64>,
    rsi: Vec<f64>,
}

#[derive(Clone)]
struct Rec {
    wi: usize,
    test_trades: usize,
    test_return: f64,
    test_sharpe: f64,
    test_dd: f64,
    passed: bool,
}

#[derive(Clone)]
struct Position {
    sym: String,
    side: i32,
    entry_bar: usize,
    entry_price: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ RSI Threshold Mean-Reversion Walk-Forward ═══");
    println!(
        "RSI<{:.0} long | RSI>{:.0} short | exit at RSI {:.0} or {} bars\n",
        RSI_LOWER, RSI_UPPER, RSI_EXIT, HOLD_BARS
    );

    let loader = DataLoader::new(None, None);
    let syms = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in syms {
        let raw = loader.fetch_with_cache(s, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        min_len = min_len.min(df.height());
        cache.insert(s.to_string(), df);
    }

    let n = min_len.min(2800);
    for df in cache.values_mut() {
        if df.height() > n {
            *df = df.slice(0, n);
        }
    }

    let total_windows = n.saturating_sub(TRAIN_BARS) / TEST_BARS;
    println!(
        "{} syms, {} bars, {} test windows\n",
        syms.len(),
        n,
        total_windows
    );

    let mut data: HashMap<String, SymData> = HashMap::new();
    for s in syms {
        let df = cache.get(s).unwrap();
        data.insert(s.to_string(), extract_data(df)?);
    }

    let mut records = Vec::new();
    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        if tend.saturating_sub(tstart) < HOLD_BARS + 2 {
            continue;
        }

        let (ret, sh, dd, trades, long_trades, short_trades) = run_backtest(&data, tstart, tend);
        let passed = trades >= MIN_TRADES && ret > 0.0;
        println!(
            "  RSI {:02}: {}t (L{} / S{}) {:+.1}% sh={:.2} DD={:+.1}% | {}",
            wi,
            trades,
            long_trades,
            short_trades,
            ret,
            sh,
            dd,
            if passed { "PASS" } else { "FAIL" }
        );
        records.push(Rec {
            wi,
            test_trades: trades,
            test_return: ret,
            test_sharpe: sh,
            test_dd: dd,
            passed,
        });
    }

    let total = records.len();
    let pass = records.iter().filter(|r| r.passed).count();
    let avg_ret = if total > 0 {
        records.iter().map(|r| r.test_return).sum::<f64>() / total as f64
    } else {
        0.0
    };
    let avg_sh = if total > 0 {
        records.iter().map(|r| r.test_sharpe).sum::<f64>() / total as f64
    } else {
        0.0
    };
    let worst_dd = records
        .iter()
        .map(|r| r.test_dd)
        .fold(0.0_f64, |a, v| a.min(v));

    println!("\n═══ SUMMARY ═══");
    println!("{}/{} passed", pass, total);
    println!(
        "Avg OOS: {:+.1}% | Sharpe {:.2} | Worst DD {:.1}%",
        avg_ret, avg_sh, worst_dd
    );

    write_snapshot(&records)?;
    Ok(())
}

fn extract_data(df: &DataFrame) -> Result<SymData> {
    let n = df.height();
    let open = df.column("open")?.f64()?;
    let close = df.column("close")?.f64()?;
    let rsi = df.column("rsi")?.f64()?;

    Ok(SymData {
        open: (0..n).map(|i| open.get(i).unwrap_or(0.0)).collect(),
        close: (0..n).map(|i| close.get(i).unwrap_or(0.0)).collect(),
        rsi: (0..n).map(|i| rsi.get(i).unwrap_or(50.0)).collect(),
    })
}

fn run_backtest(
    data: &HashMap<String, SymData>,
    start: usize,
    end: usize,
) -> (f64, f64, f64, usize, usize, usize) {
    let mut equity = 1.0;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut rets = Vec::new();
    let mut trades = 0usize;
    let mut long_trades = 0usize;
    let mut short_trades = 0usize;
    let mut pos: Option<Position> = None;
    let syms: Vec<String> = data.keys().cloned().collect();

    let mut bar = start;
    while bar + 1 < end {
        if let Some(p) = &pos {
            let sd = data.get(&p.sym).unwrap();
            let idx = bar.saturating_sub(1);
            let rsi_prev = sd.rsi.get(idx).copied().unwrap_or(50.0);
            let exit_now = (p.side == 1 && rsi_prev >= RSI_EXIT)
                || (p.side == -1 && rsi_prev <= RSI_EXIT)
                || bar >= p.entry_bar + HOLD_BARS
                || bar >= end - 1;

            if exit_now {
                let exit_price = sd.close.get(bar).copied().unwrap_or(0.0);
                if p.entry_price > 0.0 && exit_price > 0.0 {
                    let gross = if p.side == 1 {
                        exit_price / p.entry_price - 1.0
                    } else {
                        p.entry_price / exit_price - 1.0
                    } - TAKER_FEE * 2.0;
                    equity *= 1.0 + gross;
                    rets.push(gross);
                    trades += 1;
                    if p.side == 1 {
                        long_trades += 1;
                    } else {
                        short_trades += 1;
                    }
                }
                pos = None;
            }
        }

        if pos.is_none() {
            let idx = bar.saturating_sub(1);
            let mut best: Option<(String, i32, f64)> = None;
            for sym in &syms {
                let sd = data.get(sym).unwrap();
                let rsi_prev = sd.rsi.get(idx).copied().unwrap_or(50.0);
                let candidate = if rsi_prev < RSI_LOWER {
                    Some((sym.clone(), 1, (RSI_EXIT - rsi_prev).abs()))
                } else if rsi_prev > RSI_UPPER {
                    Some((sym.clone(), -1, (rsi_prev - RSI_EXIT).abs()))
                } else {
                    None
                };
                if let Some(c) = candidate {
                    match &best {
                        Some((_, _, score)) if *score >= c.2 => {}
                        _ => best = Some(c),
                    }
                }
            }

            if let Some((sym, side, _)) = best {
                let entry_price = data
                    .get(&sym)
                    .unwrap()
                    .open
                    .get(bar)
                    .copied()
                    .unwrap_or(0.0);
                if entry_price > 0.0 {
                    pos = Some(Position {
                        sym,
                        side,
                        entry_bar: bar,
                        entry_price,
                    });
                }
            }
        }

        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);
        bar += 1;
    }

    if let Some(p) = pos {
        let sd = data.get(&p.sym).unwrap();
        let exit_price = sd
            .close
            .get((end - 1).min(sd.close.len() - 1))
            .copied()
            .unwrap_or(0.0);
        if p.entry_price > 0.0 && exit_price > 0.0 {
            let gross = if p.side == 1 {
                exit_price / p.entry_price - 1.0
            } else {
                p.entry_price / exit_price - 1.0
            } - TAKER_FEE * 2.0;
            equity *= 1.0 + gross;
            rets.push(gross);
            trades += 1;
            if p.side == 1 {
                long_trades += 1;
            } else {
                short_trades += 1;
            }
        }
    }

    peak = peak.max(equity);
    max_dd = max_dd.min(equity / peak - 1.0);

    let ret = (equity - 1.0) * 100.0;
    let sh = if rets.len() < 2 {
        0.0
    } else {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let std = (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64).sqrt();
        if std == 0.0 {
            0.0
        } else {
            mean / std * 252.0_f64.sqrt()
        }
    };

    (ret, sh, max_dd * 100.0, trades, long_trades, short_trades)
}

fn write_snapshot(records: &[Rec]) -> Result<()> {
    let snap_dir = PathBuf::from("snapshots");
    std::fs::create_dir_all(&snap_dir)?;
    let now = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

    let mut lines = vec!["wi,test_trades,test_return,test_sharpe,test_dd,passed".to_string()];
    for r in records {
        lines.push(format!(
            "{},{},{:.2},{:.2},{:.2},{}",
            r.wi, r.test_trades, r.test_return, r.test_sharpe, r.test_dd, r.passed
        ));
    }
    let csv = lines.join("\n");
    std::fs::write(
        snap_dir.join(format!("rsi_threshold_mean_reversion_{}.csv", now)),
        &csv,
    )?;
    std::fs::write(
        snap_dir.join("rsi_threshold_mean_reversion_latest.csv"),
        csv,
    )?;
    Ok(())
}

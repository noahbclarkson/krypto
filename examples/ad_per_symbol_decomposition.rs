//! A/D Per-Symbol Decomposition — Kira 2026-04-02
//!
//! Question: where does the W01 +2278% come from? And why does W05 (-40.4%) fail?

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const AD_PERIOD: usize = 5;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;

#[derive(Clone)]
struct SymData {
    open: Vec<f64>,
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    volume: Vec<f64>,
}

impl SymData {
    fn from_df(df: &DataFrame) -> Result<Self> {
        fn parse_col(df: &DataFrame, name: &str) -> Vec<f64> {
            df.column(name)
                .unwrap()
                .f64()
                .unwrap()
                .to_vec()
                .into_iter()
                .map(|v| v.unwrap_or(0.0))
                .collect()
        }
        Ok(Self {
            open: parse_col(df, "open"),
            close: parse_col(df, "close"),
            high: parse_col(df, "high"),
            low: parse_col(df, "low"),
            volume: parse_col(df, "volume"),
        })
    }

    fn ad_now(&self, i: usize) -> f64 {
        if i < AD_PERIOD {
            return 0.0;
        }
        let mut ad = 0.0f64;
        for j in (i.saturating_sub(AD_PERIOD))..i {
            let range = self.high[j] - self.low[j];
            if range > 1e-8 {
                let cfo = (self.close[j] - self.low[j] - (self.high[j] - self.close[j])) / range;
                ad += cfo * self.volume[j];
            }
        }
        ad
    }

    fn ad_momentum(&self, i: usize) -> f64 {
        if i < AD_PERIOD * 2 {
            return 0.0;
        }
        self.ad_now(i) - self.ad_now(i.saturating_sub(AD_PERIOD))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("═══ A/D Per-Symbol Decomposition ═══\n");

    let loader = DataLoader::new(None, None);
    let syms: Vec<&str> = vec![
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
    ];

    let mut data: HashMap<&str, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    for s in &syms {
        let raw = loader.fetch_with_cache(s, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        let sd = SymData::from_df(&df)?;
        min_len = min_len.min(sd.close.len());
        data.insert(*s, sd);
    }

    let n = min_len.min(2800);
    // Truncate all series
    let syms_owned: Vec<&str> = syms.clone();
    for s in &syms_owned {
        let sd = data.get_mut(s).unwrap();
        sd.open.truncate(n);
        sd.close.truncate(n);
        sd.high.truncate(n);
        sd.low.truncate(n);
        sd.volume.truncate(n);
    }

    let total_windows = n.saturating_sub(TRAIN_BARS + AD_PERIOD * 2 + HOLD_BARS) / TEST_BARS;
    println!(
        "Symbols: {:?}\nBars: {} | Train: {} | Test: {} | Hold: {}\nWindows: {}\n",
        syms, n, TRAIN_BARS, TEST_BARS, HOLD_BARS, total_windows
    );

    // ── Walk-forward ────────────────────────────────────────────────────────
    let mut window_results: Vec<WindowRec> = Vec::new();

    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);

        if tend.saturating_sub(tstart) < HOLD_BARS + AD_PERIOD * 2 + 5 {
            continue;
        }

        // A/D momentum ranking
        let mut ad_scores: Vec<(&str, f64)> = Vec::new();
        for s in &syms {
            let sd = data.get(s).unwrap();
            let sig_bar = tstart
                .saturating_sub(1)
                .min(sd.close.len().saturating_sub(1));
            ad_scores.push((*s, sd.ad_momentum(sig_bar)));
        }
        ad_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let longs: Vec<&str> = ad_scores.iter().take(2).map(|(s, _)| *s).collect();
        let shorts: Vec<&str> = ad_scores.iter().rev().take(2).map(|(s, _)| *s).collect();
        let leg_n = (longs.len() + shorts.len()) as f64;

        // Per-symbol PnL
        let mut sym_pnl: HashMap<&str, f64> = HashMap::new();
        for s in &longs {
            let sd = data.get(s).unwrap();
            let entry_px = sd.open[tstart] * (1.0 + TAKER_FEE);
            let exit_bar = ((tstart + HOLD_BARS).min(tend)).saturating_sub(1);
            let exit_px = sd.close[exit_bar] * (1.0 - TAKER_FEE);
            sym_pnl.insert(*s, (exit_px / entry_px - 1.0) / leg_n);
        }
        for s in &shorts {
            let sd = data.get(s).unwrap();
            let entry_px = sd.open[tstart] * (1.0 - TAKER_FEE);
            let exit_bar = ((tstart + HOLD_BARS).min(tend)).saturating_sub(1);
            let exit_px = sd.close[exit_bar] * (1.0 + TAKER_FEE);
            sym_pnl.insert(*s, (entry_px / exit_px - 1.0) / leg_n);
        }

        let total_ret = sym_pnl.values().sum::<f64>() * 100.0;
        let n_trades = sym_pnl.len();
        let passed = n_trades >= MIN_TRADES && total_ret > 0.0;

        println!(
            "  W{:02}: {}t {:+.1}% | longs={:?} shorts={:?}",
            wi, n_trades, total_ret, longs, shorts
        );
        let mut sorted: Vec<_> = sym_pnl.iter().map(|(s, p)| (*s, *p)).collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        for (s, pnl) in sorted {
            println!("    {:12} {:+.2}%", s, pnl * 100.0);
        }

        window_results.push(WindowRec {
            wi,
            tstart,
            tend,
            n_trades,
            total_ret,
            longs,
            shorts,
            sym_pnl,
            passed,
        });
    }

    let n_windows = window_results.len();
    let n_pass = window_results.iter().filter(|r| r.passed).count();
    let avg_ret = window_results.iter().map(|r| r.total_ret).sum::<f64>() / n_windows as f64;

    println!("\n═══ A/D SUMMARY ═══");
    println!("{}/{} pass | Avg OOS {:+.1}%\n", n_pass, n_windows, avg_ret);

    // ── W01 and W05 deep dive ──────────────────────────────────────────────
    println!("═══ WINDOW ATTRIBUTION (ALL) ═══");
    for rec in &window_results {
        let btc_sd = data.get("BTCUSDT").unwrap();
        let eth_sd = data.get("ETHUSDT").unwrap();
        let btc_ret = if rec.tstart < btc_sd.close.len() {
            (btc_sd.close[rec.tend.saturating_sub(1)] / btc_sd.close[rec.tstart] - 1.0) * 100.0
        } else {
            0.0
        };
        let eth_ret = if rec.tstart < eth_sd.close.len() {
            (eth_sd.close[rec.tend.saturating_sub(1)] / eth_sd.close[rec.tstart] - 1.0) * 100.0
        } else {
            0.0
        };

        println!(
            "\n  W{:02} (bars {}-{}): A/D {:+.1}% | BTC {:+.1}% ETH {:+.1}%",
            rec.wi, rec.tstart, rec.tend, rec.total_ret, btc_ret, eth_ret
        );
        println!("    A/D longs={:?} shorts={:?}", rec.longs, rec.shorts);
        let mut sorted: Vec<_> = rec.sym_pnl.iter().map(|(s, p)| (*s, *p)).collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        for (s, pnl) in sorted {
            println!("    {:12} {:+.2}%", s, pnl * 100.0);
        }
    }

    // ── Per-symbol aggregate ────────────────────────────────────────────────
    println!("\n═══ AGGREGATE PER-SYMBOL ═══");
    let mut sym_tot: HashMap<&str, (f64, usize)> = HashMap::new();
    for rec in &window_results {
        for (s, pnl) in &rec.sym_pnl {
            let e = sym_tot.entry(*s).or_insert((0.0, 0));
            e.0 += *pnl;
            e.1 += 1;
        }
    }
    let mut sorted_syms: Vec<_> = sym_tot.iter().map(|(s, (sum, n))| (*s, *sum, *n)).collect();
    sorted_syms.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let grand: f64 = sorted_syms.iter().map(|(_, s, _)| *s).sum();
    for (s, sum, n) in sorted_syms {
        println!(
            "  {:12}: {:+.1}% total | {:+.2}% avg | {} windows",
            s,
            sum * 100.0,
            sum / n as f64 * 100.0,
            n
        );
    }
    println!("  GRAND TOTAL: {:+.1}%\n", grand * 100.0);

    println!("⏱  Done in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

struct WindowRec<'a> {
    wi: usize,
    tstart: usize,
    tend: usize,
    n_trades: usize,
    total_ret: f64,
    longs: Vec<&'a str>,
    shorts: Vec<&'a str>,
    sym_pnl: HashMap<&'a str, f64>,
    passed: bool,
}

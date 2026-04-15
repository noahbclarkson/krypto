//! Walk-Forward: A/D Accumulation/Distribution Momentum on Base5
//!
//! A/D = Σ ((Close - Low) - (High - Close)) / (High - Low) × Volume
//! A/D Momentum = A/D(t) - A/D(t-N)  (rate of change, no division to avoid /0)
//! Strategy: long top-2 A/D momentum, short bottom-2 A/D momentum
//! Entry: next open, Hold: 21 bars, Fee: 0.1% taker each side
//! Benchmark comparison: vs Turtle+MACD baseline
//!
//! A/D is a completely different factor family from everything tested:
//! - NOT momentum (price-based)
//! - NOT mean-reversion (price-based)
//! - NOT cross-sectional relative-value
//! - A/D is a VOLUME-WEIGHTED price pressure indicator
//!   Smart money accumulation (rising A/D + falling price) = divergence signal
//!   Distribution (falling A/D + rising price) = warning signal

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 54; // Optimized: was 21, winner from full 3-63 integer sweep (composite score 0.2855, WF Sharpe +0.49)
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;
const TOP_K: usize = 2; // top-K long and short

const TURTLE_ENTRY: usize = 21; // hyperopt 2026-04-10: full 5-100 sweep found EP=21 is global max Sharpe (0.176) and most robust (78% universes positive)

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ A/D Accumulation/Distribution Walk-Forward ═══");
    println!(
        "AD period: {} bars | Train {}b / Test {}b / Hold {}b\n",
        AD_PERIOD, TRAIN_BARS, TEST_BARS, HOLD_BARS
    );

    // ── Load data ──────────────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let syms = [
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
    ];

    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in syms {
        let raw = loader.fetch_with_cache(s, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        min_len = min_len.min(df.height());
        cache.insert(s.to_string(), df);
    }

    let n = min_len.min(2800);
    for (_, df) in &mut cache {
        if df.height() > n {
            *df = df.slice(0, n);
        }
    }

    let total_windows = n.saturating_sub(TRAIN_BARS + AD_PERIOD) / TEST_BARS;
    println!(
        "{} syms, {} bars, {} test windows\n",
        syms.len(),
        n,
        total_windows
    );

    // ── Extract A/D data per symbol ────────────────────────────────────────────
    let mut ad_data: HashMap<String, AdSymData> = HashMap::new();
    for s in syms {
        let df = cache.get(s).unwrap();
        ad_data.insert(s.to_string(), extract_ad_data(df)?);
    }

    // ── Run A/D walk-forward ───────────────────────────────────────────────────
    println!("═══ A/D Momentum Walk-Forward ═══");
    let mut ad_records: Vec<Rec> = Vec::new();

    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        if tend.saturating_sub(tstart) < HOLD_BARS + AD_PERIOD + 2 {
            continue;
        }

        let t0 = Instant::now();
        let (ret, sh, dd, trades) = run_ad_backtest(&ad_data, tstart, tend);
        let passed = trades >= MIN_TRADES && ret > 0.0;

        println!(
            "  AD  {:02}: {}t {:+.1}% sh={:.2} DD={:+.1}% | {} ({:.1}s)",
            wi,
            trades,
            ret,
            sh,
            dd,
            if passed { "PASS" } else { "FAIL" },
            t0.elapsed().as_secs_f64()
        );
        ad_records.push(Rec {
            wi,
            test_trades: trades,
            test_return: ret,
            test_sharpe: sh,
            test_dd: dd,
            passed,
        });
    }

    let ad_tot = ad_records.len();
    let ad_pass = ad_records.iter().filter(|r| r.passed).count();
    let ad_avg_r = if ad_tot > 0 {
        ad_records.iter().map(|r| r.test_return).sum::<f64>() / ad_tot as f64
    } else {
        0.0
    };
    let ad_avg_s = if ad_tot > 0 {
        ad_records.iter().map(|r| r.test_sharpe).sum::<f64>() / ad_tot as f64
    } else {
        0.0
    };
    let ad_worst = ad_records
        .iter()
        .map(|r| r.test_dd)
        .fold(0.0_f64, |a, v| a.min(v));

    println!("\n═══ A/D SUMMARY ═══");
    println!(
        "{}/{} passed ({:.0}% fail)",
        ad_pass,
        ad_tot,
        if ad_tot > 0 {
            (ad_tot - ad_pass) as f64 / ad_tot as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "Avg OOS: {:+.1}% | Sharpe {:.2} | Worst DD {:.1}%\n",
        ad_avg_r, ad_avg_s, ad_worst
    );

    // ── Run Turtle+MACD baseline (same windows, for head-to-head) ──────────────
    println!("═══ Turtle+MACD Baseline (same windows) ═══");
    let mut turtle_data: HashMap<String, TurtleSymData> = HashMap::new();
    for s in syms {
        let df = cache.get(s).unwrap();
        turtle_data.insert(s.to_string(), extract_turtle_data(df)?);
    }

    let mut turtle_records: Vec<Rec> = Vec::new();
    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        if tend.saturating_sub(tstart) < HOLD_BARS + TURTLE_ENTRY + 2 {
            continue;
        }

        let t0 = Instant::now();
        let (ret, sh, dd, trades) = run_turtle_backtest(&turtle_data, tstart, tend);
        let passed = trades >= MIN_TRADES && ret > 0.0;

        println!(
            "  TUR {:02}: {}t {:+.1}% sh={:.2} DD={:+.1}% | {} ({:.1}s)",
            wi,
            trades,
            ret,
            sh,
            dd,
            if passed { "PASS" } else { "FAIL" },
            t0.elapsed().as_secs_f64()
        );
        turtle_records.push(Rec {
            wi,
            test_trades: trades,
            test_return: ret,
            test_sharpe: sh,
            test_dd: dd,
            passed,
        });
    }

    let turtle_tot = turtle_records.len();
    let turtle_pass = turtle_records.iter().filter(|r| r.passed).count();
    let turtle_avg_r = if turtle_tot > 0 {
        turtle_records.iter().map(|r| r.test_return).sum::<f64>() / turtle_tot as f64
    } else {
        0.0
    };
    let turtle_avg_s = if turtle_tot > 0 {
        turtle_records.iter().map(|r| r.test_sharpe).sum::<f64>() / turtle_tot as f64
    } else {
        0.0
    };
    let turtle_worst = turtle_records
        .iter()
        .map(|r| r.test_dd)
        .fold(0.0_f64, |a, v| a.min(v));

    println!("\n═══ Turtle+MACD SUMMARY ═══");
    println!(
        "{}/{} passed ({:.0}% fail)",
        turtle_pass,
        turtle_tot,
        if turtle_tot > 0 {
            (turtle_tot - turtle_pass) as f64 / turtle_tot as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "Avg OOS: {:+.1}% | Sharpe {:.2} | Worst DD {:.1}%\n",
        turtle_avg_r, turtle_avg_s, turtle_worst
    );

    // ── Head-to-head comparison ─────────────────────────────────────────────────
    println!("═══ Head-to-Head (same windows) ═══");
    let n_compare = ad_records.len().min(turtle_records.len());
    let mut ad_wins = 0usize;
    for i in 0..n_compare {
        let ad_r = ad_records.get(i).map(|r| r.test_return).unwrap_or(0.0);
        let tu_r = turtle_records.get(i).map(|r| r.test_return).unwrap_or(0.0);
        let winner = if ad_r > tu_r { "AD" } else { "Turtle" };
        if ad_r > tu_r {
            ad_wins += 1;
        }
        println!(
            "  W{:02}: AD {:+.1}% vs Turtle {:+.1}% → {}",
            i, ad_r, tu_r, winner
        );
    }
    println!(
        "\nA/D wins: {}/{} windows ({:.0}%)\n",
        ad_wins,
        n_compare,
        ad_wins as f64 / n_compare as f64 * 100.0
    );

    // ── Snapshot ───────────────────────────────────────────────────────────────
    write_snapshots(&ad_records, &turtle_records)?;

    Ok(())
}

// ─── A/D per-symbol data ──────────────────────────────────────────────────────

struct AdSymData {
    close: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    volume: Vec<f64>,
    /// Cumulative A/D line
    ad_line: Vec<f64>,
    /// A/D momentum = A/D(t) - A/D(t-N)
    ad_momentum: Vec<f64>,
}

fn compute_ad_line(high: &[f64], low: &[f64], close: &[f64], volume: &[f64]) -> Vec<f64> {
    let n = high.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let h = high[i];
        let l = low[i];
        let c = close[i];
        let v = volume[i];
        let range = h - l;
        let mf = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        let money_flow = mf * v;
        ad[i] = if i == 0 {
            money_flow
        } else {
            ad[i - 1] + money_flow
        };
    }
    ad
}

fn extract_ad_data(df: &DataFrame) -> Result<AdSymData> {
    let n = df.height();
    let close_ch = df.column("close")?.f64()?;
    let open_ch = df.column("open")?.f64()?;
    let high_ch = df.column("high")?.f64()?;
    let low_ch = df.column("low")?.f64()?;
    let volume_ch = df.column("volume")?.f64()?;

    let close: Vec<f64> = (0..n).map(|i| close_ch.get(i).unwrap_or(0.0)).collect();
    let open: Vec<f64> = (0..n).map(|i| open_ch.get(i).unwrap_or(0.0)).collect();
    let high: Vec<f64> = (0..n).map(|i| high_ch.get(i).unwrap_or(0.0)).collect();
    let low: Vec<f64> = (0..n).map(|i| low_ch.get(i).unwrap_or(0.0)).collect();
    let volume: Vec<f64> = (0..n).map(|i| volume_ch.get(i).unwrap_or(0.0)).collect();

    let ad_line = compute_ad_line(&high, &low, &close, &volume);

    // A/D momentum = current A/D - A/D N bars ago
    let mut ad_momentum = vec![0.0; n];
    for i in AD_PERIOD..n {
        ad_momentum[i] = ad_line[i] - ad_line[i - AD_PERIOD];
    }

    Ok(AdSymData {
        close,
        open,
        high,
        low,
        volume,
        ad_line,
        ad_momentum,
    })
}

// ─── Turtle+MACD per-symbol data ──────────────────────────────────────────────

struct TurtleSymData {
    close: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    macd: Vec<f64>,
    macd_signal: Vec<f64>,
    sigs: Vec<i32>,
}

fn extract_turtle_data(df: &DataFrame) -> Result<TurtleSymData> {
    let n = df.height();
    let close_ch = df.column("close")?.f64()?;
    let open_ch = df.column("open")?.f64()?;
    let high_ch = df.column("high")?.f64()?;
    let low_ch = df.column("low")?.f64()?;
    let macd_ch = df.column("macd")?.f64()?;
    let macd_sig_ch = df.column("macd_signal")?.f64()?;

    let close: Vec<f64> = (0..n).map(|i| close_ch.get(i).unwrap_or(0.0)).collect();
    let open: Vec<f64> = (0..n).map(|i| open_ch.get(i).unwrap_or(0.0)).collect();
    let high: Vec<f64> = (0..n).map(|i| high_ch.get(i).unwrap_or(0.0)).collect();
    let low: Vec<f64> = (0..n).map(|i| low_ch.get(i).unwrap_or(0.0)).collect();
    let macd: Vec<f64> = (0..n).map(|i| macd_ch.get(i).unwrap_or(0.0)).collect();
    let macd_signal: Vec<f64> = (0..n).map(|i| macd_sig_ch.get(i).unwrap_or(0.0)).collect();

    // Turtle 20-bar Donchian
    let mut donch_high = vec![0.0; n];
    let mut donch_low = vec![0.0; n];
    for i in TURTLE_ENTRY..n {
        let mut hh = f64::MIN;
        let mut ll = f64::MAX;
        for j in (i.saturating_sub(TURTLE_ENTRY))..i {
            hh = hh.max(high[j]);
            ll = ll.min(low[j]);
        }
        donch_high[i] = hh;
        donch_low[i] = ll;
    }

    // Turtle+MACD: breakout + MACD confirmation
    let mut sigs = vec![0i32; n];
    for i in TURTLE_ENTRY..n {
        let price = close[i];
        let m = macd[i];
        let ms = macd_signal[i];
        let hh = donch_high[i];
        let ll = donch_low[i];
        if m > ms && price > hh {
            sigs[i] = 1;
        } else if m < ms && price < ll {
            sigs[i] = -1;
        }
    }

    Ok(TurtleSymData {
        close,
        open,
        high,
        low,
        macd,
        macd_signal,
        sigs,
    })
}

// ─── A/D backtest ─────────────────────────────────────────────────────────────

fn run_ad_backtest(
    data: &HashMap<String, AdSymData>,
    start: usize,
    end: usize,
) -> (f64, f64, f64, usize) {
    if end <= start || end - start < HOLD_BARS + AD_PERIOD + 2 {
        return (0.0, 0.0, 0.0, 0);
    }

    let sym0 = data.keys().next().unwrap();
    let n0 = data.get(sym0).unwrap().open.len();
    let eff_end = end.min(n0);

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets: Vec<f64> = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None; // (symbol, entry_bar, entry_price)

    let sym_list: Vec<String> = data.keys().cloned().collect();
    let mut bar = start;

    while bar + 1 < eff_end {
        if pos.is_none() {
            // Rank all symbols by A/D momentum at bar-1
            let mut longs: Vec<(&String, f64)> = Vec::new();
            let mut shorts: Vec<(&String, f64)> = Vec::new();

            for sym in &sym_list {
                let sd = data.get(sym).unwrap();
                let idx = bar.saturating_sub(1);
                if idx < AD_PERIOD {
                    continue;
                }
                let mom = *sd.ad_momentum.get(idx).unwrap_or(&0.0);
                if mom > 0.0 {
                    longs.push((sym, mom));
                } else {
                    shorts.push((sym, mom));
                }
            }

            longs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            shorts.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            let top_syms: Vec<String> = longs
                .iter()
                .take(TOP_K)
                .map(|(s, _)| (*s).clone())
                .collect();
            let bot_syms: Vec<String> = shorts
                .iter()
                .take(TOP_K)
                .map(|(s, _)| (*s).clone())
                .collect();

            if !top_syms.is_empty() {
                let sym = &top_syms[0];
                let sd = data.get(sym).unwrap();
                let entry_price = *sd.open.get(bar).unwrap_or(&0.0);
                if entry_price > 0.0 {
                    pos = Some((sym.clone(), bar, entry_price));
                }
            }

            bar += 1;
            continue;
        }

        // Position management
        let (sym, entry_bar, entry_price) = pos.as_ref().unwrap();
        let sd = data.get(sym).unwrap();
        let current_bar = bar;

        if current_bar >= entry_bar + HOLD_BARS || current_bar >= eff_end - 1 {
            // Exit
            let exit_price = *sd
                .close
                .get(current_bar)
                .unwrap_or(&sd.close[sd.close.len() - 1]);
            if *entry_price > 0.0 && exit_price > 0.0 {
                let gross = (exit_price / entry_price - 1.0) - TAKER_FEE;
                equity *= 1.0 + gross;
                trades += 1;
                rets.push(gross);
            }
            pos = None;
        }

        // Track drawdown
        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);

        bar += 1;
    }

    // Close any open position at end
    if let Some((sym, entry_bar, entry_price)) = pos {
        let sd = data.get(&sym).unwrap();
        let exit_price = *sd
            .close
            .get((end - 1).min(sd.close.len() - 1))
            .unwrap_or(&sd.close[sd.close.len() - 1]);
        if entry_price > 0.0 && exit_price > 0.0 {
            let gross = (exit_price / entry_price - 1.0) - TAKER_FEE;
            equity *= 1.0 + gross;
            trades += 1;
            rets.push(gross);
        }
    }

    peak = peak.max(equity);
    max_dd = max_dd.min(equity / peak - 1.0);

    let ret = (equity - 1.0) * 100.0;
    let sh = if rets.is_empty() || rets.iter().map(|r| r.powi(2)).sum::<f64>() == 0.0 {
        0.0
    } else {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let std = (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64).sqrt();
        if std == 0.0 {
            0.0
        } else {
            mean / std * (252.0_f64.sqrt())
        }
    };

    (ret, sh, max_dd * 100.0, trades)
}

// ─── Turtle+MACD backtest (single-position version) ──────────────────────────

fn run_turtle_backtest(
    data: &HashMap<String, TurtleSymData>,
    start: usize,
    end: usize,
) -> (f64, f64, f64, usize) {
    if end <= start || end - start < HOLD_BARS + TURTLE_ENTRY + 2 {
        return (0.0, 0.0, 0.0, 0);
    }

    let sym0 = data.keys().next().unwrap();
    let n0 = data.get(sym0).unwrap().open.len();
    let eff_end = end.min(n0);

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets: Vec<f64> = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;

    let sym_list: Vec<String> = data.keys().cloned().collect();
    let mut bar = start;

    while bar + 1 < eff_end {
        if pos.is_none() {
            let mut cand: Vec<(String, f64)> = Vec::new();
            for sym in &sym_list {
                let sd = data.get(sym).unwrap();
                let idx = bar.saturating_sub(1);
                if idx < TURTLE_ENTRY {
                    continue;
                }
                let sig = *sd.sigs.get(idx).unwrap_or(&0);
                if sig == 0 {
                    continue;
                }
                let macd_gap = (sd.macd.get(idx).copied().unwrap_or(0.0)
                    - sd.macd_signal.get(idx).copied().unwrap_or(0.0))
                .abs();
                cand.push((sym.clone(), macd_gap));
            }
            cand.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            if !cand.is_empty() {
                let sym = &cand[0].0;
                let sd = data.get(sym).unwrap();
                let entry_price = *sd.open.get(bar).unwrap_or(&0.0);
                if entry_price > 0.0 {
                    pos = Some((sym.clone(), bar, entry_price));
                }
            }
            bar += 1;
            continue;
        }

        let (sym, entry_bar, entry_price) = pos.as_ref().unwrap();
        let sd = data.get(sym).unwrap();
        let current_bar = bar;

        if current_bar >= entry_bar + HOLD_BARS || current_bar >= eff_end - 1 {
            let exit_price = *sd
                .close
                .get(current_bar)
                .unwrap_or(&sd.close[sd.close.len() - 1]);
            if *entry_price > 0.0 && exit_price > 0.0 {
                let gross = (exit_price / entry_price - 1.0) - TAKER_FEE;
                equity *= 1.0 + gross;
                trades += 1;
                rets.push(gross);
            }
            pos = None;
        }

        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);

        bar += 1;
    }

    if let Some((sym, entry_bar, entry_price)) = pos {
        let sd = data.get(&sym).unwrap();
        let exit_price = *sd
            .close
            .get((end - 1).min(sd.close.len() - 1))
            .unwrap_or(&sd.close[sd.close.len() - 1]);
        if entry_price > 0.0 && exit_price > 0.0 {
            let gross = (exit_price / entry_price - 1.0) - TAKER_FEE;
            equity *= 1.0 + gross;
            trades += 1;
            rets.push(gross);
        }
    }

    peak = peak.max(equity);
    max_dd = max_dd.min(equity / peak - 1.0);

    let ret = (equity - 1.0) * 100.0;
    let sh = if rets.is_empty() || rets.iter().map(|r| r.powi(2)).sum::<f64>() == 0.0 {
        0.0
    } else {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let std = (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64).sqrt();
        if std == 0.0 {
            0.0
        } else {
            mean / std * (252.0_f64.sqrt())
        }
    };

    (ret, sh, max_dd * 100.0, trades)
}

// ─── Snapshot ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Rec {
    wi: usize,
    test_trades: usize,
    test_return: f64,
    test_sharpe: f64,
    test_dd: f64,
    passed: bool,
}

fn write_snapshots(ad_records: &[Rec], turtle_records: &[Rec]) -> Result<()> {
    let snap_dir = PathBuf::from("snapshots");
    std::fs::create_dir_all(&snap_dir)?;

    let now = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

    // AD records
    {
        let mut lines = vec!["wi,test_trades,test_return,test_sharpe,test_dd,passed".to_string()];
        for r in ad_records {
            lines.push(format!(
                "{},{},{:.2},{:.2},{:.2},{}",
                r.wi, r.test_trades, r.test_return, r.test_sharpe, r.test_dd, r.passed
            ));
        }
        std::fs::write(
            snap_dir.join(format!("ad_walk_forward_{}.csv", now)),
            lines.join("\n"),
        )?;
        std::fs::write(
            snap_dir.join("ad_walk_forward_latest.csv"),
            lines.join("\n"),
        )?;
    }

    // Turtle records
    {
        let mut lines = vec!["wi,test_trades,test_return,test_sharpe,test_dd,passed".to_string()];
        for r in turtle_records {
            lines.push(format!(
                "{},{},{:.2},{:.2},{:.2},{}",
                r.wi, r.test_trades, r.test_return, r.test_sharpe, r.test_dd, r.passed
            ));
        }
        std::fs::write(
            snap_dir.join(format!("turtle_walk_forward_same_windows_{}.csv", now)),
            lines.join("\n"),
        )?;
        std::fs::write(
            snap_dir.join("turtle_walk_forward_same_windows_latest.csv"),
            lines.join("\n"),
        )?;
    }

    println!("Snapshots written.");
    Ok(())
}

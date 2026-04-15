//! Walk-Forward Validation: MACD+Regime on Base5

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ MACD+Regime Walk-Forward: Base5 ═══");
    println!(
        "Train {}b / Test {}b / Min {} trades\n",
        TRAIN_BARS, TEST_BARS, MIN_TRADES
    );

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

    let total_windows = n.saturating_sub(TRAIN_BARS) / TEST_BARS;
    println!(
        "{} syms, {} bars, {} windows\n",
        syms.len(),
        n,
        total_windows
    );

    let mut records: Vec<Rec> = Vec::new();

    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        if tend - tstart < HOLD_BARS + 2 {
            continue;
        }

        let t0 = Instant::now();

        // Extract per-symbol data
        let mut sym_data: HashMap<String, SymData> = HashMap::new();
        for s in syms {
            let df = cache.get(&s.to_string()).unwrap();
            sym_data.insert(s.to_string(), extract_sym_data(df)?);
        }

        let (ret, sh, dd, trades) = run_backtest(&sym_data, tstart, tend);
        let passed = trades >= MIN_TRADES && ret > 0.0;

        println!(
            "  {:02}: {}t {:+.1}% sh={:.2} DD={:.1}% | {} ({:.1}s)",
            wi,
            trades,
            ret,
            sh,
            dd,
            if passed { "PASS" } else { "FAIL" },
            t0.elapsed().as_secs_f64()
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

    let tot = records.len();
    let pass = records.iter().filter(|r| r.passed).count();
    let avg_r = if tot > 0 {
        records.iter().map(|r| r.test_return).sum::<f64>() / tot as f64
    } else {
        0.0
    };
    let avg_s = if tot > 0 {
        records.iter().map(|r| r.test_sharpe).sum::<f64>() / tot as f64
    } else {
        0.0
    };
    let worst = records
        .iter()
        .map(|r| r.test_dd)
        .fold(0.0_f64, |a, v| a.min(v));

    println!("\n═══ SUMMARY ═══");
    println!(
        "{}/{} passed ({:.0}% fail)",
        pass,
        tot,
        if tot > 0 {
            (tot - pass) as f64 / tot as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "Avg OOS: {:+.1}% | Sharpe {:.2} | Worst DD {:.1}%",
        avg_r, avg_s, worst
    );

    write_snapshots(&records)?;
    println!();
    if tot == 0 {
        println!("⚠️  No valid windows");
    } else if pass == tot {
        println!("✅ ALL PASSED — MACD+Regime is OOS robust");
    } else if pass >= (tot * 3) / 4 {
        println!("🟡 {}/{} windows passed", pass, tot);
    } else {
        println!("🔴 <75% windows failed — MACD+Regime fails OOS on Base5");
    }

    Ok(())
}

// ─── Data extraction ──────────────────────────────────────────────────────────

struct SymData {
    close: Vec<f64>,
    open: Vec<f64>,
    macd: Vec<f64>,
    macd_signal: Vec<f64>,
    sigs: Vec<i32>,
}

fn extract_sym_data(df: &DataFrame) -> Result<SymData> {
    let n = df.height();
    let close_ch = df.column("close")?.f64()?;
    let macd_ch = df.column("macd")?.f64()?;
    let macd_sig_ch = df.column("macd_signal")?.f64()?;
    let open_ch = df.column("open")?.f64()?;

    let close: Vec<f64> = (0..n).map(|i| close_ch.get(i).unwrap_or(0.0)).collect();
    let macd: Vec<f64> = (0..n).map(|i| macd_ch.get(i).unwrap_or(0.0)).collect();
    let macd_signal: Vec<f64> = (0..n).map(|i| macd_sig_ch.get(i).unwrap_or(0.0)).collect();
    let open: Vec<f64> = (0..n).map(|i| open_ch.get(i).unwrap_or(0.0)).collect();

    // SMA200
    let mut sma200 = vec![0.0; n];
    for i in 200..n {
        let mut sum = 0.0_f64;
        for j in (i - 200)..i {
            sum += close[j];
        }
        sma200[i] = sum / 200.0;
    }

    // MACD+Regime signals
    let mut sigs = vec![0i32; n];
    for i in 0..n {
        let price = close[i];
        let m = macd[i];
        let ms = macd_signal[i];
        let s = sma200[i];
        if s <= 0.0 {
            continue;
        }
        if m > ms && price > s {
            sigs[i] = 1;
        } else if m < ms && price < s {
            sigs[i] = -1;
        }
    }

    Ok(SymData {
        close,
        open,
        macd,
        macd_signal,
        sigs,
    })
}

// ─── Backtest ─────────────────────────────────────────────────────────────────

fn run_backtest(
    data: &HashMap<String, SymData>,
    start: usize,
    end: usize,
) -> (f64, f64, f64, usize) {
    if end <= start || end - start < HOLD_BARS + 2 {
        return (0.0, 0.0, 0.0, 0);
    }

    // Use indexed access — all data has same length
    let sym0 = data.keys().next().unwrap();
    let n = data.get(sym0).unwrap().open.len();
    let eff_end = end.min(n);

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets: Vec<f64> = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None; // (symbol, entry_bar, entry_price)

    // Iterate in deterministic order
    let sym_list: Vec<String> = data.keys().cloned().collect();
    let mut bar = start;

    while bar + 1 < eff_end {
        if pos.is_none() {
            // Top-3 by MACD strength at bar-1
            let mut cand: Vec<(String, f64)> = Vec::new();
            for sym in &sym_list {
                let sd = data.get(sym).unwrap();
                if bar == 0 {
                    continue;
                }
                let sig = *sd.sigs.get(bar - 1).unwrap_or(&0);
                if sig == 0 {
                    continue;
                }
                let str = sd
                    .macd
                    .get(bar.saturating_sub(1))
                    .copied()
                    .unwrap_or(0.0)
                    .abs();
                cand.push((sym.clone(), str));
            }
            cand.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            for (sym, _) in cand.into_iter().take(3) {
                let sd = data.get(&sym).unwrap();
                let sig = *sd.sigs.get(bar.saturating_sub(1)).unwrap_or(&0);
                if sig == 0 {
                    continue;
                }
                let ep = *sd.open.get(bar).unwrap_or(&0.0);
                if ep > 0.0 {
                    pos = Some((sym, bar, ep));
                    break;
                }
            }
        } else {
            let (sym, entry_bar, entry_px) = pos.clone().unwrap();
            let held = bar - entry_bar;
            if held >= HOLD_BARS {
                let xi = (bar + 1).min(eff_end - 1);
                let sd = data.get(&sym).unwrap();
                let xp = *sd.open.get(xi).unwrap_or(&entry_px);
                if entry_px > 0.0 && xp > 0.0 {
                    let gross = (xp / entry_px) - 1.0;
                    let net = gross - (2.0 * TAKER_FEE);
                    rets.push(net);
                    trades += 1;
                    equity *= 1.0 + net;
                    peak = peak.max(equity);
                    max_dd = max_dd.max((peak - equity) / peak * 100.0);
                }
                pos = None;
            }
        }
        bar += 1;
    }

    if trades == 0 {
        return (0.0, 0.0, 0.0, 0);
    }
    let ret_pct = (equity - 1.0) * 100.0;
    let avg = rets.iter().sum::<f64>() / trades as f64;
    let var = rets.iter().map(|r| (r - avg).powi(2)).sum::<f64>() / trades as f64;
    let sd = var.sqrt().max(1e-10);
    let sh = (avg / sd) * (252.0_f64).sqrt();
    (ret_pct, sh, -max_dd, trades)
}

// ─── Records ──────────────────────────────────────────────────────────────────

struct Rec {
    wi: usize,
    test_trades: usize,
    test_return: f64,
    test_sharpe: f64,
    test_dd: f64,
    passed: bool,
}

fn write_snapshots(recs: &[Rec]) -> anyhow::Result<()> {
    std::fs::create_dir_all("snapshots")?;
    let cp = PathBuf::from("snapshots/macd_regime_walk_forward_latest.csv");
    let mp = PathBuf::from("snapshots/macd_regime_walk_forward_latest.md");

    let mut csv = String::from("window,test_trades,test_return,test_sharpe,test_dd,passed\n");
    for r in recs {
        csv.push_str(&format!(
            "{},{},{:.1},{},{},{}\n",
            r.wi, r.test_trades, r.test_return, r.test_sharpe, r.test_dd, r.passed
        ));
    }
    std::fs::write(&cp, &csv)?;

    let tot = recs.len();
    let pass = recs.iter().filter(|r| r.passed).count();
    let avg_r = if tot > 0 {
        recs.iter().map(|r| r.test_return).sum::<f64>() / tot as f64
    } else {
        0.0
    };
    let avg_s = if tot > 0 {
        recs.iter().map(|r| r.test_sharpe).sum::<f64>() / tot as f64
    } else {
        0.0
    };

    let mut md = format!(
        "# MACD+Regime Walk-Forward: Base5\n\n\
        ## Summary\n\n\
        - Universes: Base5 | Train {}b / Test {}b / Hold {}b\n\
        - Windows: {}/{} passed ({:.0}% fail)\n\
        - Avg OOS return: {:+.1}% | Avg OOS Sharpe: {:.2}\n\n\
        | Window | Test Trades | Test Ret% | Test Sharpe | Test MaxDD% | Pass |\n\
        |--------|------------|-----------|-------------|------------|------|\n",
        TRAIN_BARS,
        TEST_BARS,
        HOLD_BARS,
        pass,
        tot,
        if tot > 0 {
            (tot - pass) as f64 / tot as f64 * 100.0
        } else {
            0.0
        },
        avg_r,
        avg_s
    );
    for r in recs {
        md.push_str(&format!(
            "| {:02} | {} | {:+.1}% | {:.2} | {:.1}% | {} |\n",
            r.wi,
            r.test_trades,
            r.test_return,
            r.test_sharpe,
            r.test_dd,
            if r.passed { "✅" } else { "❌" }
        ));
    }
    std::fs::write(&mp, &md)?;
    println!("\nSnapshots: {} {}", cp.display(), mp.display());
    Ok(())
}

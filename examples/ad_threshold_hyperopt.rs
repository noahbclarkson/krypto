//! A/D Momentum Entry Threshold Hyperopt

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const HOLD_BASELINE: usize = 54;
const ATR_PERIOD: usize = 45;
const ATR_MULT: f64 = 2.5;

// Threshold values: try much larger ones, raw ad_momentum is unscaled here.
const THRESHOLDS: &[f64] = &[
    0.0, 0.5, 1.0, 2.0, 3.0, 5.0, 7.5, 10.0, 15.0, 20.0, 30.0, 50.0,
];

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "NoDOGE",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT",
        ],
    ),
    (
        "Legacy4",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "_", "_"],
    ),
    (
        "Legacy5BNB",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "_"],
    ),
    (
        "OldGuardNoBNB",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "_"],
    ),
    (
        "LargeCaps5",
        &["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT", "_"],
    ),
    ("Legacy3", &["BTCUSDT", "ETHUSDT", "XRPUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    ("OldGuard4", &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
];

struct SymbolData {
    close: Vec<f64>,
    open: Vec<f64>,
    atr: Vec<f64>,
    ad_momentum: Vec<f64>,
}

#[derive(Clone)]
struct ThresholdAgg {
    avg_sharpe: f64,
    avg_ret: f64,
    avg_dd: f64,
    total_qp: usize,
    total_trades: usize,
    total_chand: usize,
    merged_equity: Vec<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    std::fs::create_dir_all("snapshots")?;
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            if s != "_" {
                all_syms.insert(s);
            }
        }
    }

    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in all_syms.iter() {
        if let Ok(raw) = loader.fetch_with_cache(s, "1d", CANDLES).await {
            if let Ok(df) = FeatureEngine::add_technicals(&raw, None) {
                let n = df.height();
                min_len = min_len.min(n);
                cache.insert(s.to_string(), df);
            }
        }
    }
    let n = min_len.saturating_sub(62).min(2900);
    for df in cache.values_mut() {
        if df.height() > n {
            *df = df.slice(0, n);
        }
    }
    let windows = (n.saturating_sub(TRAIN_BARS + 62)) / TEST_BARS;

    let mut sym_data: HashMap<String, SymbolData> = HashMap::new();
    for (sym, df) in &cache {
        if let Ok(sd) = compute_symbol_data(df) {
            sym_data.insert(sym.clone(), sd);
        }
    }

    // First find median absolute non-zero momentum to scale our thresholds reasonably
    let mut all_moms = Vec::new();
    for sd in sym_data.values() {
        for &m in &sd.ad_momentum {
            if m.abs() > 1e-9 {
                all_moms.push(m.abs());
            }
        }
    }
    all_moms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_mom = if all_moms.is_empty() {
        1.0
    } else {
        all_moms[all_moms.len() / 2]
    };
    println!("  Median |A/D Momentum|: {}", median_mom);

    // Scale thresholds to be multiples of median momentum
    let scaled_thresholds: Vec<f64> = THRESHOLDS.iter().map(|&t| t * median_mom).collect();

    let mut global_results: HashMap<usize, ThresholdAgg> = HashMap::new();
    for (idx, _thr) in scaled_thresholds.iter().enumerate() {
        global_results.insert(
            idx,
            ThresholdAgg {
                avg_sharpe: 0.0,
                avg_ret: 0.0,
                avg_dd: 0.0,
                total_qp: 0,
                total_trades: 0,
                total_chand: 0,
                merged_equity: vec![1.0],
            },
        );
    }

    println!("\n  A/D MOMENTUM THRESHOLD HYPEROPT (0 to 50x median)\n");
    for &(uni_name, syms) in UNIVERSES {
        let valid_syms: Vec<String> = syms
            .iter()
            .filter(|&&s| s != "_" && sym_data.contains_key(s))
            .map(|s| s.to_string())
            .collect();
        if valid_syms.len() < 2 {
            continue;
        }

        let mut best_sh = -999.0;
        let mut best_idx = 0;
        for (idx, &thr) in scaled_thresholds.iter().enumerate() {
            let (qp, ash, art, add, atr, ach, eq) =
                run_universe(&sym_data, &valid_syms, n, windows, thr);
            if ash > best_sh {
                best_sh = ash;
                best_idx = idx;
            }

            let n_u = UNIVERSES.len() as f64;
            if let Some(g) = global_results.get_mut(&idx) {
                g.avg_sharpe += ash / n_u;
                g.avg_ret += art / n_u;
                g.avg_dd += add.abs() / n_u;
                g.total_qp += qp;
                g.total_trades += atr;
                g.total_chand += ach;
                if !eq.is_empty() {
                    let base_val = *g.merged_equity.last().unwrap_or(&1.0);
                    g.merged_equity.extend(eq.iter().map(|&v| base_val * v));
                }
            }
        }
        println!(
            "{:<18} best: thr={}x Sharpe={:+.3}",
            uni_name, THRESHOLDS[best_idx], best_sh
        );
    }

    let mut ranked: Vec<(usize, f64, f64, f64, usize, usize, usize)> = Vec::new();
    for (idx, agg) in &global_results {
        ranked.push((
            *idx,
            agg.avg_sharpe,
            agg.avg_ret,
            agg.avg_dd,
            agg.total_qp,
            agg.total_trades,
            agg.total_chand,
        ));
    }
    ranked.sort_by(|a, b| {
        score(b.1, b.4, windows)
            .partial_cmp(&score(a.1, a.4, windows))
            .unwrap()
    });

    println!("\n  GLOBAL RANKING");
    for &(idx, sh, ret, dd, qp, trades, _) in &ranked {
        println!(
            "  thr={:>4.1}x | Sharpe={:>6.3} | Ret={:>7.1}% | DD={:>5.1}% | QP={:>2} | Trades={}",
            THRESHOLDS[idx], sh, ret, dd, qp, trades
        );
    }

    export_csv(&ranked, &global_results)?;
    println!("\n  Winner: thr={:.1}x", THRESHOLDS[ranked[0].0]);
    Ok(())
}

fn compute_symbol_data(df: &DataFrame) -> Result<SymbolData> {
    let n = df.height();
    let close: Vec<f64> = df
        .column("close")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let open: Vec<f64> = df
        .column("open")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let atr: Vec<f64> = if df.column("atr").is_ok() {
        df.column("atr")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect()
    } else {
        vec![0.0; n]
    };
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let vol = df.column("volume")?.f64()?;

    let mut ad_line: Vec<f64> = Vec::with_capacity(n);
    let mut ad: f64 = 0.0;
    for i in 0..n {
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let c = close[i];
        let v = vol.get(i).unwrap_or(0.0);
        let range = h - l;
        let mf = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        ad += mf * v;
        ad_line.push(ad);
    }

    let mut ad_momentum: Vec<f64> = vec![0.0; n];
    for i in AD_PERIOD..n {
        ad_momentum[i] = ad_line[i] - ad_line[i - AD_PERIOD];
    }

    Ok(SymbolData {
        close,
        open,
        atr,
        ad_momentum,
    })
}

fn run_universe(
    sym_data: &HashMap<String, SymbolData>,
    symbols: &[String],
    n: usize,
    windows: usize,
    threshold: f64,
) -> (usize, f64, f64, f64, usize, usize, Vec<f64>) {
    let (mut tqp, mut tsh, mut trt, mut tdd, mut ttr, mut tch, mut n_v) =
        (0, 0.0, 0.0, 0.0, 0, 0, 0);
    let mut merged_eq = vec![1.0];
    for wi in 0..windows {
        let ts = TRAIN_BARS + wi * TEST_BARS;
        let te = (ts + TEST_BARS).min(n - 2);
        if te <= ts + 10 {
            continue;
        }
        let (ret, sh, dd, trades, chand, eq) = run_window(sym_data, symbols, ts, te, threshold);
        if trades >= 3 {
            n_v += 1;
            tsh += sh;
            trt += ret;
            tdd += dd.abs();
            ttr += trades;
            tch += chand;
            if ret > 0.0 {
                tqp += 1;
            }
            let base = *merged_eq.last().unwrap_or(&1.0);
            merged_eq.extend(eq.iter().map(|&v| base * v));
        }
    }
    let nv = n_v.max(1) as f64;
    let eq_out = if merged_eq.len() > 1500 {
        let step = (merged_eq.len() as f64 / 1200.0).ceil() as usize;
        merged_eq
            .iter()
            .enumerate()
            .filter(|(i, _)| i % step == 0)
            .map(|(_, &v)| v)
            .collect()
    } else {
        merged_eq
    };
    (
        tqp,
        tsh / nv,
        trt / nv,
        tdd / nv,
        ttr / n_v.max(1),
        tch / n_v.max(1),
        eq_out,
    )
}

fn run_window(
    sym_data: &HashMap<String, SymbolData>,
    symbols: &[String],
    tstart: usize,
    tend: usize,
    threshold: f64,
) -> (f64, f64, f64, usize, usize, Vec<f64>) {
    let (mut equity, mut peak, mut max_dd, mut trades, mut chand, mut bar) =
        (1.0f64, 1.0f64, 0.0f64, 0, 0, tstart);
    let mut rets = Vec::new();
    let mut eq_curve = vec![equity];
    let mut pos: Option<(String, usize, f64)> = None;
    let mut highest = 0.0;

    while bar < tend {
        if pos.is_none() {
            let mut best_sym: Option<(String, f64)> = None;
            for sym in symbols {
                if let Some(sd) = sym_data.get(sym) {
                    if bar >= AD_PERIOD + 1 && bar < sd.ad_momentum.len() {
                        let mom = sd.ad_momentum[bar];
                        let price = sd.close[bar];
                        if mom > threshold && price > 0.0 {
                            if best_sym.is_none() || mom > best_sym.as_ref().unwrap().1 {
                                best_sym = Some((sym.clone(), mom));
                            }
                        }
                    }
                }
            }
            if let Some((sym, _)) = best_sym {
                if let Some(sd) = sym_data.get(&sym) {
                    let ep = if bar + 1 < sd.open.len() {
                        sd.open[bar + 1]
                    } else {
                        sd.close[bar]
                    };
                    if ep > 0.0 {
                        pos = Some((sym, bar, ep));
                        highest = ep;
                    }
                }
            }
            eq_curve.push(equity);
            bar += 1;
            continue;
        }

        let (sym_name, ebar, epx) = pos.as_ref().unwrap().clone();
        let sd = sym_data.get(&sym_name).unwrap();
        if bar < sd.close.len() {
            highest = highest.max(sd.close[bar]);
        }
        let mut exited = false;

        if bar >= ebar + HOLD_BASELINE || bar >= tend - 1 {
            let xpx = sd
                .close
                .get(bar.min(sd.close.len() - 1))
                .copied()
                .unwrap_or(epx);
            if epx > 0.0 && xpx > 0.0 {
                let net = ((xpx - epx) / epx) - 2.0 * TAKER_FEE;
                equity *= 1.0 + net;
                trades += 1;
                rets.push(net);
            }
            pos = None;
            exited = true;
        }

        if !exited && bar >= ebar + ATR_PERIOD {
            let cx = sd
                .close
                .get(bar.min(sd.close.len() - 1))
                .copied()
                .unwrap_or(epx);
            let ca = sd
                .atr
                .get(bar.min(sd.atr.len() - 1))
                .copied()
                .unwrap_or(0.0);
            if cx < highest - ATR_MULT * ca {
                if epx > 0.0 && cx > 0.0 {
                    let net = ((cx - epx) / epx) - 2.0 * TAKER_FEE;
                    equity *= 1.0 + net;
                    trades += 1;
                    chand += 1;
                    rets.push(net);
                }
                pos = None;
            }
        }
        peak = peak.max(equity);
        max_dd = max_dd.max(1.0 - equity / peak);
        eq_curve.push(equity);
        bar += 1;
    }
    let nr = rets.len().max(1) as f64;
    let mean = rets.iter().sum::<f64>() / nr;
    let std = (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / nr.max(1.0)).sqrt();
    let sharpe = if std > 0.0 {
        (mean / std) * 252.0_f64.sqrt()
    } else {
        0.0
    };
    (
        (equity - 1.0) * 100.0,
        sharpe,
        max_dd * 100.0,
        trades,
        chand,
        eq_curve,
    )
}

fn score(sh: f64, qp: usize, w: usize) -> f64 {
    (qp as f64 / w as f64) * 20.0 + sh.max(0.0)
}

fn export_csv(
    ranked: &[(usize, f64, f64, f64, usize, usize, usize)],
    global: &HashMap<usize, ThresholdAgg>,
) -> Result<()> {
    let mut f = File::create("snapshots/ad_threshold_sweep_latest.csv")?;
    writeln!(
        f,
        "threshold,avg_sharpe,avg_ret_pct,avg_dd_pct,total_qp,total_trades"
    )?;
    for &(idx, sh, ret, dd, qp, trades, _) in ranked {
        writeln!(
            f,
            "{},{},{},{},{},{}",
            THRESHOLDS[idx], sh, ret, dd, qp, trades
        )?;
    }

    let mut f2 = File::create("snapshots/ad_threshold_equity.csv")?;
    let mut h = "bar".to_string();
    for &t in THRESHOLDS {
        h.push_str(&format!(",thr_{}", t));
    }
    writeln!(f2, "{}", h)?;
    let max_l = global
        .values()
        .map(|a| a.merged_equity.len())
        .max()
        .unwrap_or(0);
    for i in 0..max_l {
        let mut p = vec![i.to_string()];
        for (_idx, &t) in THRESHOLDS.iter().enumerate() {
            let actual_idx = THRESHOLDS.iter().position(|&x| x == t).unwrap();
            if let Some(agg) = global.get(&actual_idx) {
                p.push(
                    agg.merged_equity
                        .get(i)
                        .map(|x| format!("{:.6}", x))
                        .unwrap_or_default(),
                );
            } else {
                p.push(String::new());
            }
        }
        writeln!(f2, "{}", p.join(","))?;
    }
    Ok(())
}

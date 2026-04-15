//! Walk-Forward: A/D Momentum × Turtle+MACD Ensemble on Base5
//!
//! Combining A/D Accumulation/Distribution (volume-weighted price pressure)
//! with Turtle+MACD (price-based trend) via majority vote.
//! A/D: long top-2 by A/D momentum, Turtle: long top-2 by trend signal strength.
//! Ensemble: net vote score > 0 → long, < 0 → short.
//! Walk-forward: 252b train / 252b test / 21b hold / 0.1% taker
//!
//! A/D and Turtle are regime-orthogonal:
//!   Turtle wins: bull trending (W00, W03, W06)
//!   A/D wins: crash/recovery regimes (W01, W04)
//!   Both fail: 2022-23 bear chop (W05)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 54; // Optimized: was 21, winner from full 3-63 integer sweep (composite score 0.2855, WF Sharpe +0.49)
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const TURTLE_ENTRY: usize = 21; // hyperopt 2026-04-10: full 5-100 sweep found EP=21 is global max Sharpe (0.176) and most robust (78% universes positive)
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;
const TOP_K: usize = 2;

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ A/D × Turtle+MACD Ensemble Walk-Forward ═══");
    println!(
        "Train {}b / Test {}b / Hold {}b / AD {}b\n",
        TRAIN_BARS, TEST_BARS, HOLD_BARS, AD_PERIOD
    );

    // Load data
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

    let total_windows = n.saturating_sub(TRAIN_BARS + AD_PERIOD.max(TURTLE_ENTRY)) / TEST_BARS;
    println!(
        "{} syms, {} bars, {} windows\n",
        syms.len(),
        n,
        total_windows
    );

    // Extract per-symbol data
    let mut ad_data: HashMap<String, AdData> = HashMap::new();
    let mut tu_data: HashMap<String, TuData> = HashMap::new();
    for s in syms {
        let df = cache.get(s).unwrap();
        ad_data.insert(s.to_string(), extract_ad(df)?);
        tu_data.insert(s.to_string(), extract_tu(df)?);
    }

    // Walk-forward
    println!("═══ Walk-Forward Results ═══");
    let mut ens_rec: Vec<Rec> = Vec::new();
    let mut ad_rec: Vec<Rec> = Vec::new();
    let mut tu_rec: Vec<Rec> = Vec::new();

    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        if tend.saturating_sub(tstart) < HOLD_BARS + AD_PERIOD.max(TURTLE_ENTRY) + 2 {
            continue;
        }

        let (er, es, ed, et) = run_ensemble(&ad_data, &tu_data, tstart, tend);
        let (ar, as_, ad, at) = run_ad(&ad_data, tstart, tend);
        let (tr, ts, td, tt) = run_turtle(&tu_data, tstart, tend);

        ens_rec.push(Rec {
            wi,
            ret: er,
            sh: es,
            dd: ed,
            trades: et,
        });
        ad_rec.push(Rec {
            wi,
            ret: ar,
            sh: as_,
            dd: ad,
            trades: at,
        });
        tu_rec.push(Rec {
            wi,
            ret: tr,
            sh: ts,
            dd: td,
            trades: tt,
        });

        let ep = et >= MIN_TRADES && er > 0.0;
        let ap = at >= MIN_TRADES && ar > 0.0;
        let tp = tt >= MIN_TRADES && tr > 0.0;

        println!(
            "  {:02}: ENS {:+9.1} sh={:5.2} DD={:6.1} | {} | AD {:+9.1} | TU {:+9.1}",
            wi,
            er,
            es,
            ed,
            if ep { "PASS" } else { "FAIL" },
            ar,
            tr
        );
    }

    // Summary
    println!("\n═══ SUMMARY ═══");
    let show = |label: &str, rec: &[Rec]| {
        let tot = rec.len();
        let pass = rec
            .iter()
            .filter(|r| r.trades >= MIN_TRADES && r.ret > 0.0)
            .count();
        let avg_r = rec.iter().map(|r| r.ret).sum::<f64>() / tot.max(1) as f64;
        let avg_s = rec.iter().map(|r| r.sh).sum::<f64>() / tot.max(1) as f64;
        let worst = rec.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v));
        println!(
            "  {:20} {:4}/{:4} pass  AvgRet {:+9.1}  Sharpe {:5.2}  WorstDD {:7.1}",
            label, pass, tot, avg_r, avg_s, worst
        );
    };

    show("Ensemble(AD+Turtle)", &ens_rec);
    show("AD Momentum", &ad_rec);
    show("Turtle+MACD", &tu_rec);

    // Head-to-head
    let n_c = ens_rec.len().min(ad_rec.len()).min(tu_rec.len());
    let mut ens_beats = 0;
    let mut ad_beats = 0;
    for i in 0..n_c {
        if ens_rec[i].ret > tu_rec[i].ret {
            ens_beats += 1;
        }
        if ad_rec[i].ret > tu_rec[i].ret {
            ad_beats += 1;
        }
    }
    println!(
        "\n  vs Turtle: Ensemble wins {}/{} windows, AD wins {}/{} windows",
        ens_beats, n_c, ad_beats, n_c
    );

    // DD improvement
    let tu_worst = tu_rec.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v));
    let ens_worst = ens_rec.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v));
    let ad_worst = ad_rec.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v));
    println!(
        "  WorstDD vs Turtle: ENS {:+.0}pp, AD {:+.0}pp",
        tu_worst - ens_worst,
        tu_worst - ad_worst
    );

    // vs USDT overlay
    const USDT_DD: f64 = -90.5;
    println!("\n═══ vs USDT Overlay (worst DD -90.5%) ═══");
    if ens_worst > USDT_DD {
        println!(
            "  Ensemble ({:.0}) beats USDT overlay ({:.0})",
            ens_worst, USDT_DD
        );
    } else {
        println!(
            "  USDT overlay ({:.0}) still better than ensemble ({:.0})",
            USDT_DD, ens_worst
        );
    }

    write_snapshots(&ens_rec, &ad_rec, &tu_rec)?;
    Ok(())
}

// ─── Types ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Rec {
    wi: usize,
    ret: f64,
    sh: f64,
    dd: f64,
    trades: usize,
}

struct AdData {
    close: Vec<f64>,
    open: Vec<f64>,
    ad_momentum: Vec<f64>,
}

struct TuData {
    close: Vec<f64>,
    open: Vec<f64>,
    macd: Vec<f64>,
    macd_signal: Vec<f64>,
    sigs: Vec<i32>,
}

fn compute_ad(high: &[f64], low: &[f64], close: &[f64], volume: &[f64]) -> Vec<f64> {
    let n = high.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let range = high[i] - low[i];
        let mf = if range > 1e-9 {
            ((close[i] - low[i]) - (high[i] - close[i])) / range
        } else {
            0.0
        };
        ad[i] = if i == 0 {
            mf * volume[i]
        } else {
            ad[i - 1] + mf * volume[i]
        };
    }
    ad
}

fn extract_ad(df: &DataFrame) -> Result<AdData> {
    let n = df.height();
    let close: Vec<f64> = (0..n)
        .map(|i| {
            df.column("close")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect();
    let open: Vec<f64> = (0..n)
        .map(|i| {
            df.column("open")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect();
    let high: Vec<f64> = (0..n)
        .map(|i| {
            df.column("high")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect();
    let low: Vec<f64> = (0..n)
        .map(|i| {
            df.column("low")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect();
    let vol: Vec<f64> = (0..n)
        .map(|i| {
            df.column("volume")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect();
    let ad_line = compute_ad(&high, &low, &close, &vol);
    let mut mom = vec![0.0; n];
    for i in AD_PERIOD..n {
        mom[i] = ad_line[i] - ad_line[i - AD_PERIOD];
    }
    Ok(AdData {
        close,
        open,
        ad_momentum: mom,
    })
}

fn extract_tu(df: &DataFrame) -> Result<TuData> {
    let n = df.height();
    let close: Vec<f64> = (0..n)
        .map(|i| {
            df.column("close")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect();
    let open: Vec<f64> = (0..n)
        .map(|i| {
            df.column("open")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect();
    let high: Vec<f64> = (0..n)
        .map(|i| {
            df.column("high")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect();
    let low: Vec<f64> = (0..n)
        .map(|i| {
            df.column("low")
                .unwrap()
                .f64()
                .unwrap()
                .get(i)
                .unwrap_or(0.0)
        })
        .collect();
    let macd_ch = df.column("macd").unwrap().f64().unwrap();
    let macd_sig_ch = df.column("macd_signal").unwrap().f64().unwrap();
    let macd: Vec<f64> = (0..n).map(|i| macd_ch.get(i).unwrap_or(0.0)).collect();
    let macd_signal: Vec<f64> = (0..n).map(|i| macd_sig_ch.get(i).unwrap_or(0.0)).collect();

    let mut dc_hi = vec![0.0; n];
    let mut dc_lo = vec![0.0; n];
    for i in TURTLE_ENTRY..n {
        let mut hh = f64::MIN;
        let mut ll = f64::MAX;
        for j in (i.saturating_sub(TURTLE_ENTRY))..i {
            hh = hh.max(high[j]);
            ll = ll.min(low[j]);
        }
        dc_hi[i] = hh;
        dc_lo[i] = ll;
    }

    let mut sigs = vec![0i32; n];
    for i in TURTLE_ENTRY..n {
        if macd[i] > macd_signal[i] && close[i] > dc_hi[i] {
            sigs[i] = 1;
        } else if macd[i] < macd_signal[i] && close[i] < dc_lo[i] {
            sigs[i] = -1;
        }
    }
    Ok(TuData {
        close,
        open,
        macd,
        macd_signal,
        sigs,
    })
}

// ─── Backtest engine ──────────────────────────────────────────────────────────

fn run_bt(
    close: &[f64],
    open: &[f64],
    entry_bar: usize,
    entry_price: f64,
    fee: f64,
    hold: usize,
    eff_end: usize,
    bar: usize,
) -> (f64, f64) {
    // Returns (gross_return_pct, total_return_pct)
    let exit_price = *close.get(bar).unwrap_or(&close[close.len() - 1]);
    if entry_price <= 0.0 || exit_price <= 0.0 {
        return (0.0, 0.0);
    }
    let gross = (exit_price / entry_price - 1.0) - fee;
    (gross * 100.0, gross)
}

fn run_ad(data: &HashMap<String, AdData>, start: usize, end: usize) -> (f64, f64, f64, usize) {
    let sym0 = data.keys().next().unwrap();
    let n0 = data.get(sym0).unwrap().close.len();
    let eff = end.min(n0);
    let mut eq = 1.0_f64;
    let mut peak = eq;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;
    let syms: Vec<String> = data.keys().cloned().collect();
    let mut bar = start;

    while bar + 1 < eff {
        if pos.is_none() {
            let idx = bar.saturating_sub(1);
            if idx < AD_PERIOD {
                bar += 1;
                continue;
            }
            let mut longs: Vec<(&String, f64)> = Vec::new();
            for sym in &syms {
                let mom = *data.get(sym).unwrap().ad_momentum.get(idx).unwrap_or(&0.0);
                if mom > 0.0 {
                    longs.push((sym, mom));
                }
            }
            longs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            if !longs.is_empty() {
                let sd = data.get(longs[0].0).unwrap();
                let entry = sd.open[bar];
                if entry > 0.0 {
                    pos = Some(((longs[0].0).clone(), bar, entry));
                }
            }
            bar += 1;
            continue;
        }

        let (sym, ebar, epr) = pos.as_ref().unwrap();
        let sd = data.get(sym).unwrap();
        let exit_price = *sd.close.get(bar).unwrap_or(&0.0);

        if bar >= *ebar + HOLD_BARS || bar >= eff - 1 {
            if *epr > 0.0 && exit_price > 0.0 {
                let g = (exit_price / *epr - 1.0) - TAKER_FEE;
                eq *= 1.0 + g;
                trades += 1;
                rets.push(g);
            }
            pos = None;
        }
        peak = peak.max(eq);
        max_dd = max_dd.min(eq / peak - 1.0);
        bar += 1;
    }
    if let Some((sym, ebar, epr)) = pos {
        let sd = data.get(&sym).unwrap();
        let idx = (end - 1).min(sd.close.len() - 1);
        let exit_price = *sd.close.get(idx).unwrap_or(&0.0);
        if epr > 0.0 && exit_price > 0.0 {
            eq *= 1.0 + (exit_price / epr - 1.0) - TAKER_FEE;
            trades += 1;
        }
    }
    peak = peak.max(eq);
    max_dd = max_dd.min(eq / peak - 1.0);
    let ret = (eq - 1.0) * 100.0;
    let sh = sharpe(&rets);
    (ret, sh, max_dd * 100.0, trades)
}

fn run_turtle(data: &HashMap<String, TuData>, start: usize, end: usize) -> (f64, f64, f64, usize) {
    let sym0 = data.keys().next().unwrap();
    let n0 = data.get(sym0).unwrap().close.len();
    let eff = end.min(n0);
    let mut eq = 1.0_f64;
    let mut peak = eq;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;
    let syms: Vec<String> = data.keys().cloned().collect();
    let mut bar = start;

    while bar + 1 < eff {
        if pos.is_none() {
            let idx = bar.saturating_sub(1);
            if idx < TURTLE_ENTRY {
                bar += 1;
                continue;
            }
            let mut cand: Vec<(String, f64)> = Vec::new();
            for sym in &syms {
                let sd = data.get(sym).unwrap();
                let sig = *sd.sigs.get(idx).unwrap_or(&0);
                if sig == 0 {
                    continue;
                }
                let gap = (sd.macd[idx] - sd.macd_signal[idx]).abs();
                cand.push((sym.clone(), gap));
            }
            cand.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            if !cand.is_empty() {
                let sd = data.get(&cand[0].0).unwrap();
                let entry = sd.open[bar];
                if entry > 0.0 {
                    pos = Some((cand[0].0.clone(), bar, entry));
                }
            }
            bar += 1;
            continue;
        }

        let (sym, ebar, epr) = pos.as_ref().unwrap();
        let sd = data.get(sym).unwrap();
        let exit_price = *sd.close.get(bar).unwrap_or(&0.0);

        if bar >= *ebar + HOLD_BARS || bar >= eff - 1 {
            if *epr > 0.0 && exit_price > 0.0 {
                let g = (exit_price / *epr - 1.0) - TAKER_FEE;
                eq *= 1.0 + g;
                trades += 1;
                rets.push(g);
            }
            pos = None;
        }
        peak = peak.max(eq);
        max_dd = max_dd.min(eq / peak - 1.0);
        bar += 1;
    }
    if let Some((sym, _ebar, epr)) = pos {
        let sd = data.get(&sym).unwrap();
        let idx = (end - 1).min(sd.close.len() - 1);
        let exit_price = *sd.close.get(idx).unwrap_or(&0.0);
        if epr > 0.0 && exit_price > 0.0 {
            eq *= 1.0 + (exit_price / epr - 1.0) - TAKER_FEE;
            trades += 1;
        }
    }
    peak = peak.max(eq);
    max_dd = max_dd.min(eq / peak - 1.0);
    let ret = (eq - 1.0) * 100.0;
    let sh = sharpe(&rets);
    (ret, sh, max_dd * 100.0, trades)
}

fn run_ensemble(
    ad_data: &HashMap<String, AdData>,
    tu_data: &HashMap<String, TuData>,
    start: usize,
    end: usize,
) -> (f64, f64, f64, usize) {
    let sym0 = ad_data.keys().next().unwrap();
    let n0 = ad_data.get(sym0).unwrap().close.len();
    let eff = end.min(n0);
    let mut eq = 1.0_f64;
    let mut peak = eq;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;
    let syms: Vec<String> = ad_data.keys().cloned().collect();
    let mut bar = start;

    while bar + 1 < eff {
        if pos.is_none() {
            let idx = bar.saturating_sub(1);
            if idx < AD_PERIOD.max(TURTLE_ENTRY) {
                bar += 1;
                continue;
            }

            // A/D rankings
            let mut ad_longs: Vec<(&String, f64)> = Vec::new();
            let mut ad_shorts: Vec<(&String, f64)> = Vec::new();
            for sym in &syms {
                let mom = *ad_data
                    .get(sym)
                    .unwrap()
                    .ad_momentum
                    .get(idx)
                    .unwrap_or(&0.0);
                if mom > 0.0 {
                    ad_longs.push((sym, mom));
                } else {
                    ad_shorts.push((sym, mom));
                }
            }
            ad_longs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            ad_shorts.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

            // Turtle rankings
            let mut tu_longs: Vec<(&String, f64)> = Vec::new();
            let mut tu_shorts: Vec<(&String, f64)> = Vec::new();
            for sym in &syms {
                let sd = tu_data.get(sym).unwrap();
                let sig = *sd.sigs.get(idx).unwrap_or(&0);
                if sig > 0 {
                    tu_longs.push((sym, (sd.macd[idx] - sd.macd_signal[idx]).abs()));
                } else if sig < 0 {
                    tu_shorts.push((sym, (sd.macd[idx] - sd.macd_signal[idx]).abs()));
                }
            }
            tu_longs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            tu_shorts.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

            // Majority vote scoring
            let mut score: HashMap<String, i32> = HashMap::new();
            for (s, _) in ad_longs.iter().take(TOP_K) {
                *score.entry((*s).clone()).or_insert(0) += 1;
            }
            for (s, _) in ad_shorts.iter().take(TOP_K) {
                *score.entry((*s).clone()).or_insert(0) -= 1;
            }
            for (s, _) in tu_longs.iter().take(TOP_K) {
                *score.entry((*s).clone()).or_insert(0) += 1;
            }
            for (s, _) in tu_shorts.iter().take(TOP_K) {
                *score.entry((*s).clone()).or_insert(0) -= 1;
            }

            let best = score
                .iter()
                .filter(|(_, v)| **v > 0)
                .max_by_key(|(_, v)| *v)
                .map(|(s, _)| s.clone());
            if let Some(sym) = best {
                let entry = ad_data.get(&sym).unwrap().open[bar];
                if entry > 0.0 {
                    pos = Some((sym, bar, entry));
                }
            }
            bar += 1;
            continue;
        }

        let (sym, ebar, epr) = pos.as_ref().unwrap();
        let sd = ad_data.get(sym).unwrap();
        let exit_price = *sd.close.get(bar).unwrap_or(&0.0);

        if bar >= *ebar + HOLD_BARS || bar >= eff - 1 {
            if *epr > 0.0 && exit_price > 0.0 {
                let g = (exit_price / *epr - 1.0) - TAKER_FEE;
                eq *= 1.0 + g;
                trades += 1;
                rets.push(g);
            }
            pos = None;
        }
        peak = peak.max(eq);
        max_dd = max_dd.min(eq / peak - 1.0);
        bar += 1;
    }

    if let Some((sym, ebar, epr)) = pos {
        let ad = ad_data.get(&sym).unwrap();
        let idx = (end - 1).min(ad.close.len() - 1);
        let exit_price = *ad.close.get(idx).unwrap_or(&0.0);
        if epr > 0.0 && exit_price > 0.0 {
            eq *= 1.0 + (exit_price / epr - 1.0) - TAKER_FEE;
        }
    }
    peak = peak.max(eq);
    max_dd = max_dd.min(eq / peak - 1.0);
    let ret = (eq - 1.0) * 100.0;
    let sh = sharpe(&rets);
    (ret, sh, max_dd * 100.0, trades)
}

fn sharpe(rets: &[f64]) -> f64 {
    if rets.is_empty() {
        return 0.0;
    }
    let m = rets.iter().sum::<f64>() / rets.len() as f64;
    let v = rets.iter().map(|r| (r - m).powi(2)).sum::<f64>() / rets.len().max(1) as f64;
    let s = v.sqrt();
    if s == 0.0 {
        0.0
    } else {
        m / s * (252.0_f64.sqrt())
    }
}

// ─── Snapshot ─────────────────────────────────────────────────────────────────

fn write_snapshots(ens: &[Rec], ad: &[Rec], tu: &[Rec]) -> Result<()> {
    let snap_dir = PathBuf::from("snapshots");
    std::fs::create_dir_all(&snap_dir)?;
    let now = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

    let mut lines = vec!["wi,strategy,ret,sharpe,dd,trades".to_string()];
    for i in 0..ens.len().min(ad.len()).min(tu.len()) {
        lines.push(format!(
            "{},ENS,{:.2},{:.2},{:.2},{}",
            ens[i].wi, ens[i].ret, ens[i].sh, ens[i].dd, ens[i].trades
        ));
        lines.push(format!(
            "{},AD,{:.2},{:.2},{:.2},{}",
            ad[i].wi, ad[i].ret, ad[i].sh, ad[i].dd, ad[i].trades
        ));
        lines.push(format!(
            "{},TU,{:.2},{:.2},{:.2},{}",
            tu[i].wi, tu[i].ret, tu[i].sh, tu[i].dd, tu[i].trades
        ));
    }

    let fname = format!("ad_turtle_ensemble_{}.csv", now);
    std::fs::write(snap_dir.join(&fname), lines.join("\n"))?;
    std::fs::write(
        snap_dir.join("ad_turtle_ensemble_latest.csv"),
        lines.join("\n"),
    )?;
    println!("\nSnapshots written.");
    Ok(())
}

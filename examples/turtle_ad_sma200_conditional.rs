//! BTC SMA200-Conditional Turtle × A/D Ensemble Walk-Forward
//!
//! Design:
//! - BTC > SMA200 (bull): weight Turtle 1.0×, A/D 0.3×
//! - BTC < SMA200 (bear): weight A/D 1.0×, Turtle 0.3×
//! - Exit to cash when BTC < SMA200 AND Turtle signal bearish
//! - Top-1 by weighted score, 21-bar hold, 0.1% taker
//!
//! Walk-forward: 252b train / 252b test / 21b hold / 0.1% taker / Base5
//!
//! Prior findings:
//! - Majority vote (A/D + Turtle): 4/7 pass — WORSE than either component (6/7)
//! - A/D wins crash windows (W01, W04, W06), Turtle wins bull (W00, W02, W03, W05)
//! - Simple voting cancels the strongest signals when they disagree
//!
//! This design uses BTC SMA200 as the regime signal — simpler than HMM,
//! more robust than majority vote. Pre-declared weights, no optimization.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const TURTLE_ENTRY: usize = 21; // hyperopt 2026-04-10: full 5-100 sweep found EP=21 is global max Sharpe (0.176) and most robust (78% universes positive)
const SMA_PERIOD: usize = 200;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;

// Regime-conditional weights (pre-declared, not tuned)
const BULL_TURTLE_WT: f64 = 1.0;
const BULL_AD_WT: f64 = 0.3;
const BEAR_AD_WT: f64 = 1.0;
const BEAR_TURTLE_WT: f64 = 0.3;

// ─── Period labels for context ───────────────────────────────────────────────
const PERIOD_LABELS: &[&str] = &[
    "2017-18 Bull",
    "2018-19 Bear",
    "2019-20 Chop",
    "2020-21 MegaBull",
    "2021-22 War",
    "2022-23 Bear",
    "2023-24 Recovery",
];

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ BTC SMA200-Conditional Turtle × A/D Ensemble ═══");
    println!(
        "Train {}b / Test {}b / Hold {}b / Top-1",
        TRAIN_BARS, TEST_BARS, HOLD_BARS
    );
    println!(
        "Bull regime: Turtle {:.1}× A/D {:.1}× | Bear regime: A/D {:.1}× Turtle {:.1}×\n",
        BULL_TURTLE_WT, BULL_AD_WT, BEAR_AD_WT, BEAR_TURTLE_WT
    );

    // ── Load data ────────────────────────────────────────────────────────────
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

    let total_windows =
        n.saturating_sub(TRAIN_BARS + SMA_PERIOD.max(AD_PERIOD.max(TURTLE_ENTRY))) / TEST_BARS;
    println!(
        "{} syms, {} bars, {} windows\n",
        syms.len(),
        n,
        total_windows
    );

    // ── Extract per-symbol data ───────────────────────────────────────────────
    let mut ad_data: HashMap<String, AdData> = HashMap::new();
    let mut tu_data: HashMap<String, TuData> = HashMap::new();
    let mut btc_close: Vec<f64> = Vec::new();

    for s in syms {
        let df = cache.get(s).unwrap();
        ad_data.insert(s.to_string(), extract_ad(df)?);
        tu_data.insert(s.to_string(), extract_tu(df)?);
        if s == "BTCUSDT" {
            btc_close = (0..df.height())
                .map(|i| {
                    df.column("close")
                        .unwrap()
                        .f64()
                        .unwrap()
                        .get(i)
                        .unwrap_or(0.0)
                })
                .collect();
        }
    }

    // ── Walk-forward ─────────────────────────────────────────────────────────
    println!("═══ Walk-Forward Results ═══\n");
    let mut cond_rec: Vec<Rec> = Vec::new();
    let mut ad_rec: Vec<Rec> = Vec::new();
    let mut tu_rec: Vec<Rec> = Vec::new();
    let mut cash_rec: Vec<Rec> = Vec::new(); // cash baseline for context

    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        let min_req = HOLD_BARS + SMA_PERIOD.max(AD_PERIOD.max(TURTLE_ENTRY)) + 2;
        if tend.saturating_sub(tstart) < min_req {
            continue;
        }

        // Conditional ensemble
        let (cr, cs, cd, ct) = run_conditional(&ad_data, &tu_data, &btc_close, tstart, tend);
        // A/D only
        let (ar, as_, ad_dd, at) = run_ad_only(&ad_data, tstart, tend);
        // Turtle only
        let (tr, ts, td, tt) = run_turtle_only(&tu_data, tstart, tend);
        // Cash baseline
        let (cash_r, cash_sh, cash_dd) = cash_baseline(tstart, tend);

        cond_rec.push(Rec {
            wi,
            ret: cr,
            sh: cs,
            dd: cd,
            trades: ct,
        });
        ad_rec.push(Rec {
            wi,
            ret: ar,
            sh: as_,
            dd: ad_dd,
            trades: at,
        });
        tu_rec.push(Rec {
            wi,
            ret: tr,
            sh: ts,
            dd: td,
            trades: tt,
        });
        cash_rec.push(Rec {
            wi: 0,
            ret: cash_r,
            sh: cash_sh,
            dd: cash_dd,
            trades: 0,
        });

        let label = PERIOD_LABELS.get(wi).copied().unwrap_or("Unknown");
        let cp = ct >= MIN_TRADES && cr > 0.0;
        let ap = at >= MIN_TRADES && ar > 0.0;
        let tp = tt >= MIN_TRADES && tr > 0.0;

        println!("  {:02} [{}]", wi, label);
        println!(
            "    COND {:+.1}% sh={:.2} DD={:.0}% {}t | {}",
            cr,
            cs,
            cd,
            ct,
            if cp { "PASS" } else { "FAIL" }
        );
        println!(
            "    AD   {:+.1}% sh={:.2} DD={:.0}% {}t | {}",
            ar,
            as_,
            ad_dd,
            at,
            if ap { "PASS" } else { "FAIL" }
        );
        println!(
            "    TU   {:+.1}% sh={:.2} DD={:.0}% {}t | {}",
            tr,
            ts,
            td,
            tt,
            if tp { "PASS" } else { "FAIL" }
        );
        println!("    CASH {:+.1}% (baseline)", cash_r);
        println!();
    }

    // ── Summary ──────────────────────────────────────────────────────────────
    println!("═══ SUMMARY ═══");
    let show = |label: &str, rec: &[Rec]| {
        let tot = rec.len();
        if tot == 0 {
            return;
        }
        let pass = rec
            .iter()
            .filter(|r| r.trades >= MIN_TRADES && r.ret > 0.0)
            .count();
        let avg_r = rec.iter().map(|r| r.ret).sum::<f64>() / tot as f64;
        let avg_s = rec.iter().map(|r| r.sh).sum::<f64>() / tot as f64;
        let worst = rec.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v));
        println!(
            "  {:20} {:3}/{:3} pass  Avg {:+9.1}%  Sharpe {:5.2}  WorstDD {:6.0}%",
            label, pass, tot, avg_r, avg_s, worst
        );
    };

    show("COND(AD×TU|SMA)", &cond_rec);
    show("AD Momentum", &ad_rec);
    show("Turtle+MACD", &tu_rec);
    show("Cash", &cash_rec);

    // ── Head-to-head ─────────────────────────────────────────────────────────
    let n_c = cond_rec.len().min(ad_rec.len()).min(tu_rec.len());
    let mut cond_vs_tu = 0usize;
    let mut ad_vs_tu = 0usize;
    let mut cond_vs_ad = 0usize;
    for i in 0..n_c {
        if cond_rec[i].ret > tu_rec[i].ret {
            cond_vs_tu += 1;
        }
        if ad_rec[i].ret > tu_rec[i].ret {
            ad_vs_tu += 1;
        }
        if cond_rec[i].ret > ad_rec[i].ret {
            cond_vs_ad += 1;
        }
    }
    println!(
        "\n  vs Turtle: COND wins {}/{}, AD wins {}/{}",
        cond_vs_tu, n_c, ad_vs_tu, n_c
    );
    println!("  vs AD:    COND wins {}/{}", cond_vs_ad, n_c);

    // Worst DD comparison
    let tu_worst = tu_rec.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v));
    let ad_worst = ad_rec.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v));
    let cond_worst = cond_rec.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v));
    println!(
        "\n  WorstDD: COND {:+.0}pp vs TU, {:+.0}pp vs AD",
        cond_worst - tu_worst,
        cond_worst - ad_worst
    );
    println!(
        "  vs Turtle+MACD: {} | vs A/D: {}",
        cond_worst > tu_worst,
        cond_worst > ad_worst
    );

    // ── vs USDT overlay ─────────────────────────────────────────────────────
    const USDT_DD: f64 = -90.5;
    if cond_worst > USDT_DD {
        println!(
            "\n  ✅ COND worstDD ({:.0}%) beats USDT overlay (-90.5%)",
            cond_worst
        );
    } else {
        println!(
            "\n  ⚠️  USDT overlay (-90.5%) still better than COND ({:.0}%)",
            cond_worst
        );
    }

    write_snapshots(&cond_rec, &ad_rec, &tu_rec)?;
    Ok(())
}

// ─── Types ────────────────────────────────────────────────────────────────────

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
    sigs: Vec<i32>, // +1 long, -1 short, 0 flat
}

// ─── A/D computation ──────────────────────────────────────────────────────────

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

// ─── BTC SMA200 ───────────────────────────────────────────────────────────────

fn btc_sma200(btc_close: &[f64], bar: usize) -> f64 {
    if bar < SMA_PERIOD {
        return f64::MAX;
    }
    let start = bar.saturating_sub(SMA_PERIOD);
    let slice = &btc_close[start..bar];
    slice.iter().sum::<f64>() / slice.len() as f64
}

fn is_bull_regime(btc_close: &[f64], bar: usize) -> bool {
    if bar < SMA_PERIOD {
        return true;
    } // default bull if insufficient data
    let sma = btc_sma200(btc_close, bar);
    btc_close[bar] > sma
}

// ─── Conditional ensemble: BTC SMA200 → pick which family to weight ─────────

fn run_conditional(
    ad_data: &HashMap<String, AdData>,
    tu_data: &HashMap<String, TuData>,
    btc_close: &[f64],
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
            if idx < AD_PERIOD.max(TURTLE_ENTRY) || idx < SMA_PERIOD {
                bar += 1;
                continue;
            }

            let bull = is_bull_regime(btc_close, idx);

            // Score each symbol by weighted conviction
            let mut scores: Vec<(&String, f64)> = Vec::new();
            for sym in &syms {
                let ad_sd = ad_data.get(sym).unwrap();
                let tu_sd = tu_data.get(sym).unwrap();

                let ad_mom = *ad_sd.ad_momentum.get(idx).unwrap_or(&0.0);
                let tu_sig = *tu_sd.sigs.get(idx).unwrap_or(&0);
                let tu_macd_gap = (tu_sd.macd[idx] - tu_sd.macd_signal[idx]).abs();

                // Cash signal: BTC bear AND Turtle bearish → stay in cash
                let btc_bear_and_tu_bear = !bull && tu_sig < 0;

                let ad_score = ad_mom; // positive = A/D bullish
                let tu_score = tu_sig as f64 * tu_macd_gap; // signed score

                let weighted = if bull {
                    BULL_TURTLE_WT * tu_score + BULL_AD_WT * ad_score
                } else {
                    BEAR_AD_WT * ad_score + BEAR_TURTLE_WT * tu_score
                };

                // Skip: cash signal
                if btc_bear_and_tu_bear {
                    bar += 1;
                    continue;
                }

                scores.push((sym, weighted));
            }

            scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            if let Some((sym, _score)) = scores.into_iter().next() {
                let entry = ad_data.get(sym).unwrap().open[bar];
                if entry > 0.0 {
                    pos = Some((sym.clone(), bar, entry));
                }
            }
        } else {
            let (sym, ebar, epr) = pos.as_ref().unwrap();
            let ad_sd = ad_data.get(sym).unwrap();

            // Exit check: if BTC < SMA200 AND Turtle signal bearish → exit to cash
            let idx = bar
                .saturating_sub(1)
                .min(ad_sd.close.len().saturating_sub(1));
            if bar > *ebar && idx >= SMA_PERIOD {
                let bull = is_bull_regime(btc_close, idx);
                let tu_sd = tu_data.get(sym).unwrap();
                let tu_sig = *tu_sd.sigs.get(idx).unwrap_or(&0);
                if !bull && tu_sig < 0 {
                    // Exit at close price (same bar, no additional return for this bar)
                    let exit_price = *ad_sd
                        .close
                        .get(bar)
                        .unwrap_or(&ad_sd.close[ad_sd.close.len().saturating_sub(1)]);
                    if *epr > 0.0 && exit_price > 0.0 {
                        let g = (exit_price / *epr - 1.0) - TAKER_FEE;
                        eq *= 1.0 + g;
                        trades += 1;
                        rets.push(g);
                    }
                    pos = None;
                    peak = peak.max(eq);
                    max_dd = max_dd.min(eq / peak - 1.0);
                    bar += 1;
                    continue;
                }
            }

            // Hold expiry exit
            let held = bar - *ebar;
            if held >= HOLD_BARS || bar >= eff - 1 {
                let ad_sd = ad_data.get(sym).unwrap();
                let exit_price = *ad_sd
                    .close
                    .get(bar)
                    .unwrap_or(&ad_sd.close[ad_sd.close.len().saturating_sub(1)]);
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
        }
        bar += 1;
    }

    // Close open position at end
    if let Some((sym, ebar, epr)) = pos {
        let ad_sd = ad_data.get(&sym).unwrap();
        let idx = (end - 1).min(ad_sd.close.len() - 1);
        let exit_price = *ad_sd.close.get(idx).unwrap_or(&0.0);
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

// ─── A/D only (for comparison) ───────────────────────────────────────────────

fn run_ad_only(data: &HashMap<String, AdData>, start: usize, end: usize) -> (f64, f64, f64, usize) {
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
            longs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            if let Some((sym, _)) = longs.into_iter().next() {
                let entry = data.get(sym).unwrap().open[bar];
                if entry > 0.0 {
                    pos = Some((sym.clone(), bar, entry));
                }
            }
        } else {
            let (sym, ebar, epr) = pos.as_ref().unwrap();
            let sd = data.get(sym).unwrap();
            let held = bar - *ebar;
            if held >= HOLD_BARS || bar >= eff - 1 {
                let exit_price = *sd.close.get(bar).unwrap_or(&sd.close[sd.close.len() - 1]);
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
        }
        bar += 1;
    }

    if let Some((sym, _, epr)) = pos {
        let sd = data.get(&sym).unwrap();
        let idx = (end - 1).min(sd.close.len() - 1);
        let exit_price = *sd.close.get(idx).unwrap_or(&0.0);
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

// ─── Turtle only (for comparison) ─────────────────────────────────────────────

fn run_turtle_only(
    data: &HashMap<String, TuData>,
    start: usize,
    end: usize,
) -> (f64, f64, f64, usize) {
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
            cand.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            if let Some((sym, _)) = cand.into_iter().next() {
                let entry = data.get(&sym).unwrap().open[bar];
                if entry > 0.0 {
                    pos = Some((sym.clone(), bar, entry));
                }
            }
        } else {
            let (sym, ebar, epr) = pos.as_ref().unwrap();
            let sd = data.get(sym).unwrap();
            let held = bar - *ebar;
            if held >= HOLD_BARS || bar >= eff - 1 {
                let exit_price = *sd.close.get(bar).unwrap_or(&sd.close[sd.close.len() - 1]);
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
        }
        bar += 1;
    }

    if let Some((sym, _, epr)) = pos {
        let sd = data.get(&sym).unwrap();
        let idx = (end - 1).min(sd.close.len() - 1);
        let exit_price = *sd.close.get(idx).unwrap_or(&0.0);
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

// ─── Cash baseline ───────────────────────────────────────────────────────────

fn cash_baseline(start: usize, end: usize) -> (f64, f64, f64) {
    let bars = end.saturating_sub(start);
    let eq = 1.0_f64;
    let ret = (eq - 1.0) * 100.0;
    (ret, 0.0, 0.0)
}

// ─── Sharpe ─────────────────────────────────────────────────────────────────

fn sharpe(rets: &[f64]) -> f64 {
    if rets.len() < 2 {
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

// ─── Snapshots ───────────────────────────────────────────────────────────────

fn write_snapshots(cond: &[Rec], ad: &[Rec], tu: &[Rec]) -> Result<()> {
    let snap_dir = PathBuf::from("snapshots");
    std::fs::create_dir_all(&snap_dir)?;
    let now = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

    // CSV
    let mut csv_lines = vec!["wi,strategy,ret,sharpe,dd,trades".to_string()];
    for i in 0..cond.len().min(ad.len()).min(tu.len()) {
        csv_lines.push(format!(
            "{},COND,{:.2},{:.2},{:.2},{}",
            cond[i].wi, cond[i].ret, cond[i].sh, cond[i].dd, cond[i].trades
        ));
        csv_lines.push(format!(
            "{},AD,{:.2},{:.2},{:.2},{}",
            ad[i].wi, ad[i].ret, ad[i].sh, ad[i].dd, ad[i].trades
        ));
        csv_lines.push(format!(
            "{},TU,{:.2},{:.2},{:.2},{}",
            tu[i].wi, tu[i].ret, tu[i].sh, tu[i].dd, tu[i].trades
        ));
    }
    let csv_name = format!("turtle_ad_sma200_conditional_{}.csv", now);
    std::fs::write(snap_dir.join(&csv_name), csv_lines.join("\n"))?;
    std::fs::write(
        snap_dir.join("turtle_ad_sma200_conditional_latest.csv"),
        csv_lines.join("\n"),
    )?;

    // Markdown summary
    let tot = cond.len();
    let pass = cond
        .iter()
        .filter(|r| r.trades >= MIN_TRADES && r.ret > 0.0)
        .count();
    let avg_r = if tot > 0 {
        cond.iter().map(|r| r.ret).sum::<f64>() / tot as f64
    } else {
        0.0
    };
    let avg_s = if tot > 0 {
        cond.iter().map(|r| r.sh).sum::<f64>() / tot as f64
    } else {
        0.0
    };
    let worst = cond.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v));
    let ad_pass = ad
        .iter()
        .filter(|r| r.trades >= MIN_TRADES && r.ret > 0.0)
        .count();
    let tu_pass = tu
        .iter()
        .filter(|r| r.trades >= MIN_TRADES && r.ret > 0.0)
        .count();

    let mut md = format!(
        "# BTC SMA200-Conditional Turtle × A/D Ensemble\n\n\
        ## Summary\n\n\
        | Strategy | Pass | Avg Ret% | Sharpe | Worst DD% |\n\
        |----------|------|----------|--------|----------|\n\
        | **COND** | **{}/{}** | {:+.1} | {:.2} | {:.0} |\n\
        | AD Momentum | {}/{} | {:+.1} | {:.2} | {:.0} |\n\
        | Turtle+MACD | {}/{} | {:+.1} | {:.2} | {:.0} |\n\n",
        pass,
        tot,
        avg_r,
        avg_s,
        worst,
        ad_pass,
        ad.len(),
        ad.iter().map(|r| r.ret).sum::<f64>() / ad.len().max(1) as f64,
        ad.iter().map(|r| r.sh).sum::<f64>() / ad.len().max(1) as f64,
        ad.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v)),
        tu_pass,
        tu.len(),
        tu.iter().map(|r| r.ret).sum::<f64>() / tu.len().max(1) as f64,
        tu.iter().map(|r| r.sh).sum::<f64>() / tu.len().max(1) as f64,
        tu.iter().map(|r| r.dd).fold(0.0_f64, |a, v| a.min(v)),
    );

    md.push_str("## Per-Window Results\n\n");
    md.push_str("| Window | Period | Strategy | Ret% | Sharpe | DD% | Trades | Pass |\n");
    md.push_str("|--------|--------|----------|------|--------|-----|--------|------|\n");
    for i in 0..tot {
        let label = PERIOD_LABELS.get(i).copied().unwrap_or("Unknown");
        let cp = cond[i].trades >= MIN_TRADES && cond[i].ret > 0.0;
        let ap = ad[i].trades >= MIN_TRADES && ad[i].ret > 0.0;
        let tp = tu[i].trades >= MIN_TRADES && tu[i].ret > 0.0;
        md.push_str(&format!(
            "| {:02} | {} | **COND** | {:+.1} | {:.2} | {:.0} | {} | {} |\n",
            i,
            label,
            cond[i].ret,
            cond[i].sh,
            cond[i].dd,
            cond[i].trades,
            if cp { "✅" } else { "❌" }
        ));
        md.push_str(&format!(
            "|    | | AD | {:+.1} | {:.2} | {:.0} | {} | {} |\n",
            ad[i].ret,
            ad[i].sh,
            ad[i].dd,
            ad[i].trades,
            if ap { "✅" } else { "❌" }
        ));
        md.push_str(&format!(
            "|    | | TU | {:+.1} | {:.2} | {:.0} | {} | {} |\n",
            tu[i].ret,
            tu[i].sh,
            tu[i].dd,
            tu[i].trades,
            if tp { "✅" } else { "❌" }
        ));
    }

    // H2H summary
    let n_c = cond.len().min(ad.len()).min(tu.len());
    let cond_vs_tu = (0..n_c).filter(|i| cond[*i].ret > tu[*i].ret).count();
    let cond_vs_ad = (0..n_c).filter(|i| cond[*i].ret > ad[*i].ret).count();
    let ad_vs_tu = (0..n_c).filter(|i| ad[*i].ret > tu[*i].ret).count();

    md.push_str(&format!(
        "\n## Head-to-Head ({} windows)\n\n\
        - COND vs Turtle: {} wins\n\
        - COND vs A/D:    {} wins\n\
        - A/D vs Turtle:  {} wins\n\n",
        n_c, cond_vs_tu, cond_vs_ad, ad_vs_tu
    ));

    md.push_str("## Design\n\n");
    md.push_str(&format!(
        "- BTC > SMA200 (bull): Turtle **{:.1}×**, A/D **{:.1}×**\n\
        - BTC < SMA200 (bear): A/D **{:.1}×**, Turtle **{:.1}×**\n\
        - Exit to cash when BTC bear AND Turtle bearish\n\
        - Top-1 by weighted score, 21-bar hold, 0.1% taker\n\n",
        BULL_TURTLE_WT, BULL_AD_WT, BEAR_AD_WT, BEAR_TURTLE_WT
    ));

    let md_name = format!("turtle_ad_sma200_conditional_{}.md", now);
    std::fs::write(snap_dir.join(&md_name), &md)?;
    std::fs::write(snap_dir.join("turtle_ad_sma200_conditional_latest.md"), &md)?;

    println!(
        "\nSnapshots: {}",
        snap_dir
            .join("turtle_ad_sma200_conditional_latest.md")
            .display()
    );
    Ok(())
}

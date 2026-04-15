//! =========================================================
//! HYPEROPT: Regime Weight Sweep for Turtle+A/D SMA200 Ensemble
//! =========================================================
//!
//! TARGET: BULL_TURTLE_WT, BULL_AD_WT, BEAR_AD_WT, BEAR_TURTLE_WT
//! STATUS:  All 4 weights currently hardcoded with NO validation:
//!   BULL_TURTLE_WT=1.0, BULL_AD_WT=0.3, BEAR_AD_WT=1.0, BEAR_TURTLE_WT=0.3
//! METHOD:  Joint sweep — 8 configs across 3 universes (Base5+NoDOGE+Legacy4)
//!          Walk-forward 252/252, 21-bar hold, 0.1% taker fee
//!
//! Configs tested:
//!   C0 [BASELINE]: WT=1.0/0.3/1.0/0.3 (current unvalidated defaults)
//!   C1 [DUAL_AGGRESSIVE]: WT=1.0/0.5/1.0/0.5 (equal weight in each regime)
//!   C2 [TURTLE_HEAVY]: WT=1.0/0.1/0.3/1.0 (favor Turtle in bull, Turtle-heavy bear)
//!   C3 [AD_HEAVY_BEAR]: WT=1.0/0.3/1.0/0.0 (A/D only in bear — no Turtle)
//!   C4 [AD_BALANCED]: WT=0.8/0.5/0.8/0.5 (more balanced weights)
//!   C5 [AD_AGGRESSIVE]: WT=0.5/1.0/1.0/0.5 (A/D dominant in both regimes)
//!   C6 [TURTLE_DUAL]: WT=1.0/0.3/0.0/1.0 (Turtle dominant in both, A/D in bull only)
//!   C7 [SYMMETRIC]: WT=1.0/0.5/1.0/0.5 (symmetric — equal conviction in both regimes)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const AD_PERIOD: usize = 2; // hyperopt winner 2026-04-15
const TURTLE_ENTRY: usize = 21;
const SMA_PERIOD: usize = 200;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;

// ─── Weight configurations ───────────────────────────────────────────────────
#[derive(Clone, Copy)]
struct WtCfg {
    bull_tu: f64,
    bull_ad: f64,
    bear_ad: f64,
    bear_tu: f64,
    label: &'static str,
}

const CONFIGS: &[WtCfg] = &[
    WtCfg { bull_tu: 1.0, bull_ad: 0.3, bear_ad: 1.0, bear_tu: 0.3, label: "C0_BASELINE" },
    WtCfg { bull_tu: 1.0, bull_ad: 0.5, bear_ad: 1.0, bear_tu: 0.5, label: "C1_DUAL_AGGRESSIVE" },
    WtCfg { bull_tu: 1.0, bull_ad: 0.1, bear_ad: 0.3, bear_tu: 1.0, label: "C2_TURTLE_HEAVY" },
    WtCfg { bull_tu: 1.0, bull_ad: 0.3, bear_ad: 1.0, bear_tu: 0.0, label: "C3_AD_HEAVY_BEAR" },
    WtCfg { bull_tu: 0.8, bull_ad: 0.5, bear_ad: 0.8, bear_tu: 0.5, label: "C4_AD_BALANCED" },
    WtCfg { bull_tu: 0.5, bull_ad: 1.0, bear_ad: 1.0, bear_tu: 0.5, label: "C5_AD_AGGRESSIVE" },
    WtCfg { bull_tu: 1.0, bull_ad: 0.3, bear_ad: 0.0, bear_tu: 1.0, label: "C6_TURTLE_DUAL" },
    WtCfg { bull_tu: 1.0, bull_ad: 0.5, bear_ad: 1.0, bear_tu: 0.5, label: "C7_SYMMETRIC" },
];
const N_CFG: usize = CONFIGS.len();

// Universes
const S_BASE5: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const S_NODOGE: [&str; 5] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT",
];
const S_L4: [&str; 4] = [
    "BTCUSDT", "ETHUSDT", "XRPUSDT", "ADAUSDT",
];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5", &S_BASE5),
    ("NoDOGE", &S_NODOGE),
    ("Legacy4", &S_L4),
];

// ─── Types ────────────────────────────────────────────────────────────────────
struct AdData {
    close: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    volume: Vec<f64>,
    ad_momentum: Vec<f64>,
    ad_line: Vec<f64>,
}

struct TuData {
    close: Vec<f64>,
    open: Vec<f64>,
    sigs: Vec<i32>,
    macd: Vec<f64>,
    macd_signal: Vec<f64>,
}

struct Rec {
    wi: usize,
    ret: f64,
    sh: f64,
    dd: f64,
    trades: usize,
}

// ─── A/D computation ─────────────────────────────────────────────────────────

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
        ad[i] = if i == 0 { mf * volume[i] } else { ad[i - 1] + mf * volume[i] };
    }
    ad
}

fn extract_ad(df: &DataFrame) -> Result<AdData> {
    let n = df.height();
    let close: Vec<f64> = (0..n)
        .map(|i| df.column("close").unwrap().f64().unwrap().get(i).unwrap_or(0.0))
        .collect();
    let open: Vec<f64> = (0..n)
        .map(|i| df.column("open").unwrap().f64().unwrap().get(i).unwrap_or(0.0))
        .collect();
    let high: Vec<f64> = (0..n)
        .map(|i| df.column("high").unwrap().f64().unwrap().get(i).unwrap_or(0.0))
        .collect();
    let low: Vec<f64> = (0..n)
        .map(|i| df.column("low").unwrap().f64().unwrap().get(i).unwrap_or(0.0))
        .collect();
    let volume: Vec<f64> = (0..n)
        .map(|i| df.column("volume").unwrap().f64().unwrap().get(i).unwrap_or(0.0))
        .collect();

    let ad_line = compute_ad(&high, &low, &close, &volume);
    let mut ad_momentum = vec![0.0; n];
    for i in AD_PERIOD..n {
        ad_momentum[i] = ad_line[i] - ad_line[i - AD_PERIOD];
    }

    Ok(AdData { close, open, high, low, volume, ad_momentum, ad_line })
}

fn extract_tu(df: &DataFrame) -> Result<TuData> {
    let n = df.height();
    let close: Vec<f64> = (0..n)
        .map(|i| df.column("close").unwrap().f64().unwrap().get(i).unwrap_or(0.0))
        .collect();
    let open: Vec<f64> = (0..n)
        .map(|i| df.column("open").unwrap().f64().unwrap().get(i).unwrap_or(0.0))
        .collect();
    let macd_ch = df.column("macd").unwrap().f64().unwrap();
    let macd_sig_ch = df.column("macd_signal").unwrap().f64().unwrap();
    let macd: Vec<f64> = (0..n).map(|i| macd_ch.get(i).unwrap_or(0.0)).collect();
    let macd_signal: Vec<f64> = (0..n).map(|i| macd_sig_ch.get(i).unwrap_or(0.0)).collect();

    // Donchian 20-bar high/low
    let mut dc_hi = vec![0.0; n];
    let mut dc_lo = vec![0.0; n];
    for i in TURTLE_ENTRY..n {
        let mut hh = f64::MIN;
        let mut ll = f64::MAX;
        for j in (i.saturating_sub(TURTLE_ENTRY))..i {
            hh = hh.max(close[j]);
            ll = ll.min(close[j]);
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

    Ok(TuData { close, open, sigs, macd, macd_signal })
}

fn is_bull_regime(btc_close: &[f64], bar: usize) -> bool {
    if bar < SMA_PERIOD { return true; }
    let start = bar.saturating_sub(SMA_PERIOD);
    let slice = &btc_close[start..bar];
    let sma = slice.iter().sum::<f64>() / slice.len() as f64;
    btc_close[bar] > sma
}

fn run_conditional(
    ad_data: &HashMap<String, AdData>,
    tu_data: &HashMap<String, TuData>,
    btc_close: &[f64],
    wt: WtCfg,
    tstart: usize,
    tend: usize,
) -> (f64, f64, f64, usize, Vec<f64>) {
    let sym0 = ad_data.keys().next().unwrap();
    let n0 = ad_data.get(sym0).unwrap().close.len();
    let eff = tend.min(n0);

    let mut eq = 1.0_f64;
    let mut peak = eq;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;
    let syms: Vec<String> = ad_data.keys().cloned().collect();
    let mut bar = tstart;
    let mut equity_slice = Vec::new();

    while bar + 1 < eff {
        if pos.is_none() {
            let idx = bar.saturating_sub(1);
            if idx < AD_PERIOD.max(TURTLE_ENTRY) || idx < SMA_PERIOD {
                equity_slice.push(eq);
                bar += 1;
                continue;
            }
            let bull = is_bull_regime(btc_close, idx);
            let mut scores: Vec<(&String, f64)> = Vec::new();

            for sym in &syms {
                let ad_sd = ad_data.get(sym).unwrap();
                let tu_sd = tu_data.get(sym).unwrap();

                let ad_mom = *ad_sd.ad_momentum.get(idx).unwrap_or(&0.0);
                let tu_sig = *tu_sd.sigs.get(idx).unwrap_or(&0);
                let tu_macd_gap = (tu_sd.macd[idx] - tu_sd.macd_signal[idx]).abs();
                let btc_bear_and_tu_bear = !bull && tu_sig < 0;

                let ad_score = ad_mom;
                let tu_score = tu_sig as f64 * tu_macd_gap;
                let weighted = if bull {
                    wt.bull_tu * tu_score + wt.bull_ad * ad_score
                } else {
                    wt.bear_ad * ad_score + wt.bear_tu * tu_score
                };

                if btc_bear_and_tu_bear {
                    equity_slice.push(eq);
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

            let idx = bar.saturating_sub(1).min(ad_sd.close.len().saturating_sub(1));
            if bar > *ebar && idx >= SMA_PERIOD {
                let bull = is_bull_regime(btc_close, idx);
                let tu_sd = tu_data.get(sym).unwrap();
                let tu_sig = *tu_sd.sigs.get(idx).unwrap_or(&0);
                if !bull && tu_sig < 0 {
                    let exit_price = *ad_sd.close.get(bar).unwrap_or(&ad_sd.close[ad_sd.close.len() - 1]);
                    if *epr > 0.0 && exit_price > 0.0 {
                        let g = (exit_price / *epr - 1.0) - TAKER_FEE;
                        eq *= 1.0 + g;
                        trades += 1;
                        rets.push(g);
                    }
                    pos = None;
                    peak = peak.max(eq);
                    max_dd = max_dd.min(eq / peak - 1.0);
                    equity_slice.push(eq);
                    bar += 1;
                    continue;
                }
            }

            let held = bar - *ebar;
            if held >= HOLD_BARS || bar >= eff - 1 {
                let exit_price = *ad_sd.close.get(bar).unwrap_or(&ad_sd.close[ad_sd.close.len() - 1]);
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

        equity_slice.push(eq);
        bar += 1;
    }

    if let Some((sym, _, epr)) = pos {
        let ad_sd = ad_data.get(&sym).unwrap();
        let idx = (tend - 1).min(ad_sd.close.len() - 1);
        let exit_price = *ad_sd.close.get(idx).unwrap_or(&0.0);
        if epr > 0.0 && exit_price > 0.0 {
            eq *= 1.0 + (exit_price / epr - 1.0) - TAKER_FEE;
        }
    }
    peak = peak.max(eq);
    max_dd = max_dd.min(eq / peak - 1.0);

    let ret = (eq - 1.0) * 100.0;
    let sh = if rets.len() < 2 {
        0.0
    } else {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let std = (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64).sqrt();
        if std < 1e-9 { 0.0 } else { mean / std * (252.0_f64.sqrt()) }
    };
    (ret, sh, max_dd * 100.0, trades, equity_slice)
}

// ─── Main sweep ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();
    std::fs::create_dir_all("snapshots")?;

    println!("========================================================================");
    println!("  HYPEROPT: Regime Weight Sweep for Turtle+A/D SMA200 Ensemble");
    println!("  Configs: {} | Universes: Base5 + NoDOGE + Legacy4", N_CFG);
    println!("========================================================================\n");

    // Collect all unique symbols needed
    let mut all_syms: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for &(_, syms) in UNIVERSES {
        for &s in syms {
            if !s.is_empty() && s != "_" {
                all_syms.insert(s);
            }
        }
    }

    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;

    for &sym in all_syms.iter() {
        let raw = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        let n = df.height();
        min_len = min_len.min(n);
        raw_cache.insert(sym.to_string(), df);
    }

    // Trim all to common minimum
    let trim = min_len.min(2800);
    for (_, df) in &mut raw_cache {
        if df.height() > trim {
            *df = df.slice(0, trim);
        }
    }
    println!("Loaded {} symbols, {} bars.\n", raw_cache.len(), trim);

    // Pre-extract Turtle data once (doesn't depend on weight config)
    let mut tu_cache: HashMap<String, TuData> = HashMap::new();
    for &sym in all_syms.iter() {
        if let Some(df) = raw_cache.get(sym) {
            tu_cache.insert(sym.to_string(), extract_tu(df)?);
        }
    }

    let mut results: Vec<Vec<Rec>> = Vec::new();
    let mut all_eqs: Vec<Vec<Vec<f64>>> = Vec::new();
    let baseline_idx = 0; // C0_BASELINE
    let mut best_cfg = 0usize;
    let mut best_pass = 0;

    for &(uni_name, syms) in UNIVERSES {
        let valid_syms: Vec<&str> = syms.iter().filter(|s| !s.is_empty() && **s != "_").copied().collect();
        if valid_syms.is_empty() {
            continue;
        }

        let n: usize = valid_syms
            .iter()
            .filter_map(|s| raw_cache.get(*s))
            .map(|df| df.height())
            .min()
            .unwrap_or(0);

        if n < TRAIN_BARS + TEST_BARS + SMA_PERIOD + 100 {
            println!("  SKIP {} (n={})", uni_name, n);
            continue;
        }

        let n_win = n.saturating_sub(TRAIN_BARS + SMA_PERIOD) / TEST_BARS;
        println!("[{}] {} syms, {} bars, {} windows", uni_name, valid_syms.len(), n, n_win);

        // BTC close for SMA regime
        let btc_close: Vec<f64> = if let Some(df) = raw_cache.get("BTCUSDT") {
            (0..df.height().min(n))
                .map(|i| df.column("close").unwrap().f64().unwrap().get(i).unwrap_or(0.0))
                .collect()
        } else {
            vec![]
        };

        // Build A/D and Turtle data maps for this universe
        let mut ad_data: HashMap<String, AdData> = HashMap::new();
        let mut tu_data: HashMap<String, TuData> = HashMap::new();
        for &sym in &valid_syms {
            if let Some(df) = raw_cache.get(sym) {
                ad_data.insert(sym.to_string(), extract_ad(df)?);
                if let Some(td) = tu_cache.get(sym) {
                    tu_data.insert(sym.to_string(), TuData {
                        close: td.close.clone(),
                        open: td.open.clone(),
                        sigs: td.sigs.clone(),
                        macd: td.macd.clone(),
                        macd_signal: td.macd_signal.clone(),
                    });
                }
            }
        }

        let mut cfg_results: Vec<Rec> = Vec::new();
        let mut cfg_eqs: Vec<Vec<f64>> = Vec::new();

        for (ci, wt) in CONFIGS.iter().enumerate() {
            let mut total_ret = 0.0_f64;
            let mut total_sharpe = 0.0_f64;
            let mut total_dd = 0.0_f64;
            let mut total_trades = 0usize;
            let mut total_pass = 0usize;
            let mut eq_bar = 1.0_f64;
            let mut eq_curve: Vec<f64> = Vec::new();

            for wi in 0..n_win {
                let tstart = TRAIN_BARS + wi * TEST_BARS;
                let tend = (tstart + TEST_BARS).min(n);
                let min_req = HOLD_BARS + SMA_PERIOD.max(AD_PERIOD).max(TURTLE_ENTRY) + 2;
                if tend.saturating_sub(tstart) < min_req { continue; }

                let (ret, sh, dd, trades, eq_slice) =
                    run_conditional(&ad_data, &tu_data, &btc_close, *wt, tstart, tend);
                if trades >= MIN_TRADES && ret > 0.0 { total_pass += 1; }
                total_ret += ret;
                total_sharpe += sh;
                total_dd = total_dd.max(dd);
                total_trades += trades;

                // Compound equity across windows
                for i in 0..eq_slice.len() {
                    if i == 0 {
                        eq_bar = eq_slice[0];
                    } else {
                        let prev = eq_slice[i.saturating_sub(1)].max(0.001);
                        let daily_ret = (eq_slice[i] / prev).max(0.5).min(2.0);
                        eq_bar *= daily_ret;
                    }
                    eq_curve.push(eq_bar);
                }
            }

            let pass = total_pass;
            let wt_label = wt.label;
            println!(
                "  {:20}: {}/{} pass  Sharpe={:+6.2}  DD={:+5.1}%  {}t",
                wt_label, pass, n_win, total_sharpe / n_win as f64, total_dd, total_trades
            );
            cfg_results.push(Rec {
                wi: ci,
                ret: total_ret / n_win as f64,
                sh: total_sharpe / n_win as f64,
                dd: total_dd,
                trades: total_trades,
            });
            cfg_eqs.push(eq_curve);
        }

        results.push(cfg_results);
        all_eqs.push(cfg_eqs);
        println!();
    }

    // ─── Global summary ────────────────────────────────────────────────────────
    println!("========================================================================");
    println!("  GLOBAL Weight Config Comparison (3 universes)");
    println!("========================================================================");
    println!("{:25}  {:>4}  {:>8}  {:>7}", "Config", "Pass", "AvgSharpe", "AvgDD%");
    println!("------------------------------------------------------------------------");

    let mut global_pass = vec![0usize; N_CFG];
    let mut global_sharpe = vec![0.0_f64; N_CFG];
    let mut global_dd = vec![0.0_f64; N_CFG];

    for (ui, _) in UNIVERSES.iter().enumerate() {
        if ui >= results.len() { continue; }
        for (ci, rec) in results[ui].iter().enumerate() {
            if rec.trades >= MIN_TRADES && rec.sh > 0.0 {
                global_pass[ci] += 1;
            }
            global_sharpe[ci] += rec.sh;
            global_dd[ci] = global_dd[ci].max(rec.dd);
        }
    }

    let n_uni = UNIVERSES.len();
    let n_win_est = 6; // approximate windows per universe
    for (ci, wt) in CONFIGS.iter().enumerate() {
        let avg_sh = global_sharpe[ci] / n_uni as f64;
        let wt_label = wt.label;
        println!(
            "{:25}  {:>4}/{:>2}  {:>+8.2}  {:>+7.2}%",
            wt_label, global_pass[ci], n_uni * n_win_est, avg_sh, global_dd[ci]
        );
        if global_pass[ci] > best_pass {
            best_pass = global_pass[ci];
            best_cfg = ci;
        }
    }

    let baseline_pass = global_pass[baseline_idx];
    let winner_pass = global_pass[best_cfg];
    let base_label = CONFIGS[baseline_idx].label;
    let win_label = CONFIGS[best_cfg].label;
    println!("\n  Baseline: {} ({}/{} passes)", base_label, baseline_pass, n_uni * n_win_est);
    println!("  Winner:    {} ({}/{} passes)", win_label, winner_pass, n_uni * n_win_est);

    if best_cfg != baseline_idx {
        println!("  *** WINNER beats BASELINE by {} passes ***", winner_pass as i32 - baseline_pass as i32);
    }

    // ─── Write sweep CSV ───────────────────────────────────────────────────────
    let csv_path = "snapshots/turtle_ad_regime_sweep.csv";
    let mut f = File::create(csv_path)?;
    writeln!(f, "config,label,total_pass,total_windows,avg_sharpe,avg_dd_pct,total_trades")?;
    for (ci, wt) in CONFIGS.iter().enumerate() {
        let total_trades: usize = results.iter().filter_map(|r| r.get(ci)).map(|rec| rec.trades).sum();
        writeln!(
            f, "{},{},{},{},{:.3},{:.2},{}",
            ci, wt.label, global_pass[ci], n_uni * n_win_est,
            global_sharpe[ci] / n_uni as f64,
            global_dd[ci],
            total_trades
        )?;
    }
    println!("\n  Written: {}", csv_path);

    // ─── Equity curve CSV for all configs (Base5 universe) ───────────────────
    if !all_eqs.is_empty() {
        let eq_base5 = &all_eqs[0];
        let max_len = eq_base5.iter().map(|v| v.len()).max().unwrap_or(0);

        let eq_csv_path = "snapshots/turtle_ad_regime_equity.csv";
        let mut eq_f = File::create(eq_csv_path)?;

        // Header: bar, config0, config1, ...
        writeln!(eq_f, "{}", (0..N_CFG).map(|ci| CONFIGS[ci].label).collect::<Vec<_>>().join(","))?;
        for i in 0..max_len {
            let mut row = format!("{}", i);
            for peq in eq_base5 {
                let val = peq.get(i).copied().unwrap_or_else(|| peq.last().copied().unwrap_or(1.0));
                row.push_str(&format!(",{:.6}", val));
            }
            writeln!(eq_f, "{}", row)?;
        }
        println!("  Written equity: {}", eq_csv_path);
    }

    let elapsed = t0.elapsed();
    println!("\n  Done in {:.1}s.", elapsed.as_secs_f64());
    Ok(())
}

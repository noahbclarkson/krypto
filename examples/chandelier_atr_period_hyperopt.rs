//! Chandelier ATR Period Hyperopt — Simplified single-position approach
//!
//! Uses A/D momentum ranking (TOP_K=1, one position at a time, no concurrent positions)
//! to avoid the compounding bugs of concurrent-position tracking.
//!
//! Sweeps ATR period 10-100 (step 5) for Chandelier trailing stop.
//! Multiplier fixed at 2.5. Baseline: Fixed 54-bar hold.
//! 9 universes x walk-forward 252/252.
//!
//! Exports: snapshots/chandelier_atr_period_equity.csv
//!          snapshots/chandelier_atr_period_sweep_latest.csv

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
const TOP_K: usize = 1; // Single position to avoid concurrent compound bug
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const HOLD_BASELINE: usize = 54;
const ATR_MULT: f64 = 2.5;
const ATR_PERIOD_START: usize = 10;
const ATR_PERIOD_END: usize = 100;
const ATR_PERIOD_STEP: usize = 5;

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

struct PeriodAgg {
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
    let t0 = std::time::Instant::now();

    let atr_periods: Vec<usize> = (ATR_PERIOD_START..=ATR_PERIOD_END)
        .step_by(ATR_PERIOD_STEP)
        .collect();

    println!("\n{}", "=".repeat(72));
    println!("  CHANDELIER ATR PERIOD HYPEROPT (single-position)");
    println!(
        "  Period: {}-{} step {} ({} values)",
        ATR_PERIOD_START,
        ATR_PERIOD_END,
        ATR_PERIOD_STEP,
        atr_periods.len()
    );
    println!(
        "  Multiplier: {} (fixed) | Baseline: Fixed {} bar hold",
        ATR_MULT, HOLD_BASELINE
    );
    println!("{}\n", "=".repeat(72));

    // Load data
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
    println!(
        "Loaded {} symbols, {} bars, {} test windows\n",
        cache.len(),
        n,
        windows
    );

    // Pre-compute per-symbol data
    let mut sym_data: HashMap<String, SymbolData> = HashMap::new();
    for (sym, df) in &cache {
        if let Ok(sd) = compute_symbol_data(df) {
            sym_data.insert(sym.clone(), sd);
        }
    }

    // Sweep
    let mut period_results: HashMap<usize, PeriodAgg> = HashMap::new();

    for &(uni_name, syms) in UNIVERSES {
        print!("{:<18} ", uni_name);

        let valid_syms: Vec<String> = syms
            .iter()
            .filter(|&&s| s != "_" && sym_data.contains_key(s))
            .map(|s| s.to_string())
            .collect();
        if valid_syms.len() < 2 {
            println!("[SKIP]");
            continue;
        }

        let mut u_sharpe: HashMap<usize, f64> = HashMap::new();
        let mut u_ret: HashMap<usize, f64> = HashMap::new();
        let mut u_dd: HashMap<usize, f64> = HashMap::new();
        let mut u_qp: HashMap<usize, usize> = HashMap::new();
        let mut u_trades: HashMap<usize, usize> = HashMap::new();
        let mut u_chand: HashMap<usize, usize> = HashMap::new();
        let mut u_eq: HashMap<usize, Vec<f64>> = HashMap::new();

        for &atr_p in &atr_periods {
            let (qp, ash, art, add, atr, ach, eq) =
                run_universe(&sym_data, &valid_syms, n, windows, atr_p, false);
            *u_sharpe.entry(atr_p).or_insert(0.0) += ash;
            *u_ret.entry(atr_p).or_insert(0.0) += art;
            *u_dd.entry(atr_p).or_insert(0.0) += add;
            *u_qp.entry(atr_p).or_insert(0) += qp;
            *u_trades.entry(atr_p).or_insert(0) += atr;
            *u_chand.entry(atr_p).or_insert(0) += ach;

            let base = u_eq.entry(atr_p).or_insert_with(|| vec![1.0]);
            if base.len() == 1 && base[0] == 1.0 {
                *base = eq;
            } else {
                for (i, &v) in eq.iter().enumerate() {
                    if i < base.len() {
                        base[i] *= v;
                    }
                }
            }
        }

        // Baseline (fixed-hold)
        let (bqp, bsh, _, _, _, _, _) =
            run_universe(&sym_data, &valid_syms, n, windows, HOLD_BASELINE, true);

        let best_in_uni = atr_periods
            .iter()
            .max_by(|a, b| {
                u_sharpe
                    .get(*a)
                    .unwrap_or(&0.0)
                    .partial_cmp(u_sharpe.get(*b).unwrap_or(&0.0))
                    .unwrap()
            })
            .copied()
            .unwrap_or(ATR_PERIOD_START);
        let bs = u_sharpe.get(&best_in_uni).unwrap_or(&0.0);
        let qs = u_qp.get(&best_in_uni).unwrap_or(&0);
        print!(
            "period={:3} QP={:2}/{} Sharpe={:+.3} | ",
            best_in_uni, qs, windows, bs
        );

        let n_u = UNIVERSES.len() as f64;
        for &atr_p in &atr_periods {
            let agg = period_results.entry(atr_p).or_insert(PeriodAgg {
                avg_sharpe: 0.0,
                avg_ret: 0.0,
                avg_dd: 0.0,
                total_qp: 0,
                total_trades: 0,
                total_chand: 0,
                merged_equity: vec![1.0],
            });
            agg.avg_sharpe += u_sharpe.get(&atr_p).unwrap_or(&0.0) / n_u;
            agg.avg_ret += u_ret.get(&atr_p).unwrap_or(&0.0) / n_u;
            agg.avg_dd += u_dd.get(&atr_p).unwrap_or(&0.0) / n_u;
            agg.total_qp += u_qp.get(&atr_p).unwrap_or(&0);
            agg.total_trades += u_trades.get(&atr_p).unwrap_or(&0);
            agg.total_chand += u_chand.get(&atr_p).unwrap_or(&0);
            if let Some(eq) = u_eq.get(&atr_p) {
                for (i, &v) in eq.iter().enumerate() {
                    if i < agg.merged_equity.len() {
                        agg.merged_equity[i] *= v;
                    }
                }
            }
        }
        println!("FIXED(QP={:2}/{} Sharpe={:+.3})", bqp, windows, bsh);
    }

    // Normalize equity curves (geometric mean across universes)
    let n_u = UNIVERSES.len() as f64;
    for agg in period_results.values_mut() {
        for v in agg.merged_equity.iter_mut() {
            *v = v.powf(1.0 / n_u);
        }
    }

    // Rank
    let mut ranked: Vec<(usize, f64, f64, f64, usize, usize)> = Vec::new();
    for (&atr_p, agg) in &period_results {
        ranked.push((
            atr_p,
            agg.avg_sharpe,
            agg.avg_ret,
            agg.avg_dd,
            agg.total_qp,
            agg.total_chand,
        ));
    }
    ranked.sort_by(|a, b| {
        score_agg(a, windows)
            .partial_cmp(&score_agg(b, windows))
            .unwrap()
    });

    println!("\n\n{}", "=".repeat(72));
    println!("  AGGREGATE RANKING (9 universes x {} windows)", windows);
    println!("{}", "-".repeat(72));
    println!(
        "  {:>6} | {:>9} | {:>10} | {:>7} | {:>5} | {:>8}",
        "Period", "AvgSharpe", "AvgRet%", "AvgDD%", "Q-Pass%", "ChandExits"
    );
    println!("{}", "-".repeat(72));

    for (i, entry) in ranked.iter().enumerate() {
        let &(atr_p, sh, ret, dd, qp, chand) = entry;
        let qp_pct = qp as f64 / (UNIVERSES.len() * windows) as f64 * 100.0;
        let marker = if atr_p == ranked[0].0 { " *WIN" } else { "" };
        println!(
            "  {:>6}{} | {:>9.3} | {:>+10.1}% | {:>7.1}% | {:>5.0}% | {:>8}",
            atr_p, marker, sh, ret, dd, qp_pct, chand
        );
        if i >= 14 {
            break;
        }
    }

    // Export
    export_equity_csv(&period_results, &atr_periods)?;
    export_summary_csv(&ranked, windows)?;

    let winner = ranked[0].0;
    let elapsed = t0.elapsed();
    let baseline_sharpe = period_results.get(&45).map(|a| a.avg_sharpe).unwrap_or(0.0);
    println!(
        "\n  WINNER: ATR Period = {} (Sharpe={:.3})",
        winner, ranked[0].1
    );
    println!(
        "  Baseline(45): Sharpe={:.3}  delta={:+.3}",
        baseline_sharpe,
        ranked[0].1 - baseline_sharpe
    );
    println!("  Time: {:.1}s", elapsed.as_secs_f64());

    save_snapshots(&ranked, winner, elapsed)?;

    Ok(())
}

// ─── Per-symbol data ─────────────────────────────────────────────────────────

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

// ─── Universe-level backtest ─────────────────────────────────────────────────

fn run_universe(
    sym_data: &HashMap<String, SymbolData>,
    symbols: &[String],
    n: usize,
    windows: usize,
    atr_period: usize,
    is_baseline: bool,
) -> (usize, f64, f64, f64, usize, usize, Vec<f64>) {
    let mut tqp = 0usize;
    let mut tsh = 0.0f64;
    let mut trt = 0.0f64;
    let mut tdd = 0.0f64;
    let mut ttr = 0usize;
    let mut tch = 0usize;
    let mut n_v = 0usize;
    let mut merged_eq: Vec<f64> = vec![1.0];

    for wi in 0..windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let ts = train_end;
        let te = (ts + TEST_BARS).min(n - 2);
        if te <= ts + 10 {
            continue;
        }

        let (ret, sh, dd, trades, chand, eq) =
            run_window(sym_data, symbols, ts, te, atr_period, is_baseline);

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

            if !merged_eq.is_empty() && !eq.is_empty() {
                let base = *merged_eq.last().unwrap();
                merged_eq.extend(eq.iter().map(|&v| base * v));
            } else {
                merged_eq.extend(eq);
            }
        }
    }

    let nv = n_v.max(1);
    let eq_out = if merged_eq.len() > 1500 {
        let step = ((merged_eq.len() as f64) / 1200.0).ceil() as usize;
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
        tsh / nv as f64,
        trt / nv as f64,
        tdd / nv as f64,
        ttr / nv,
        tch / nv,
        eq_out,
    )
}

// ─── Per-window backtest — single position, simple loop ─────────────────────
//
// Entry: top-1 A/D momentum symbol when momentum > 0
// Exit:  Fixed hold (HOLD_BASELINE bars) OR Chandelier stop
//        Chandelier: highest_since_entry - atr_mult * ATR(atr_period)
// Returns: trade-level gross - fees

fn run_window(
    sym_data: &HashMap<String, SymbolData>,
    symbols: &[String],
    tstart: usize,
    tend: usize,
    atr_period: usize,
    is_baseline: bool,
) -> (f64, f64, f64, usize, usize, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut chand_exits = 0usize;
    let mut rets: Vec<f64> = Vec::new();
    let mut eq_curve: Vec<f64> = vec![equity];
    let mut pos: Option<(String, usize, f64)> = None; // (sym, entry_bar, entry_px)
    let mut highest_since_entry: f64 = 0.0;
    let mut bar = tstart;

    while bar < tend {
        // ── If flat: look for entry ──
        if pos.is_none() {
            let mut best_sym: Option<(String, f64)> = None;
            for sym in symbols {
                if let Some(sd) = sym_data.get(sym) {
                    if bar >= AD_PERIOD + 1 && bar < sd.ad_momentum.len() {
                        let mom = sd.ad_momentum[bar];
                        let price = sd.close[bar];
                        if mom > 0.0 && price > 0.0 {
                            match &best_sym {
                                None => best_sym = Some((sym.clone(), mom)),
                                Some((_, best_mom)) => {
                                    if mom > *best_mom {
                                        best_sym = Some((sym.clone(), mom));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some((sym, _)) = best_sym {
                if let Some(sd) = sym_data.get(&sym) {
                    let entry_price = if bar + 1 < sd.open.len() {
                        sd.open[bar + 1]
                    } else {
                        sd.close[bar]
                    };
                    if entry_price > 0.0 {
                        pos = Some((sym, bar, entry_price));
                        highest_since_entry = entry_price;
                    }
                }
            }

            eq_curve.push(equity);
            bar += 1;
            continue;
        }

        // ── Position open: check exit ──
        let sym_name: String;
        let entry_bar_val: usize;
        let entry_px_val: f64;
        if let Some(ref pd) = pos {
            sym_name = pd.0.clone();
            entry_bar_val = pd.1;
            entry_px_val = pd.2;
        } else {
            unreachable!();
        }
        let sd = sym_data.get(&sym_name).unwrap();

        // Update highest
        if bar < sd.close.len() {
            let cur_close = sd.close[bar];
            if cur_close > highest_since_entry {
                highest_since_entry = cur_close;
            }
        }

        let max_hold_bar = entry_bar_val + HOLD_BASELINE;
        let mut exited = false;
        let exit_bar_candidate = bar.min(sd.close.len().saturating_sub(1));

        // Check fixed-hold exit
        if bar >= max_hold_bar || bar >= tend - 1 {
            let exit_px = sd
                .close
                .get(exit_bar_candidate)
                .copied()
                .unwrap_or(entry_px_val);
            if entry_px_val > 0.0 && exit_px > 0.0 {
                let gross = (exit_px - entry_px_val) / entry_px_val;
                let net = gross - 2.0 * TAKER_FEE;
                equity *= 1.0 + net;
                trades += 1;
                rets.push(net);
            }
            pos = None;
            exited = true;
        }

        // Check Chandelier exit (only if not baseline and not already exited)
        if !exited && !is_baseline && bar >= entry_bar_val + atr_period {
            let cur_close = sd
                .close
                .get(bar.min(sd.close.len() - 1))
                .copied()
                .unwrap_or(entry_px_val);
            let cur_atr = sd
                .atr
                .get(bar.min(sd.atr.len() - 1))
                .copied()
                .unwrap_or(0.0);
            let stop_price = highest_since_entry - ATR_MULT * cur_atr;

            if cur_close < stop_price {
                if entry_px_val > 0.0 && cur_close > 0.0 {
                    let gross = (cur_close - entry_px_val) / entry_px_val;
                    let net = gross - 2.0 * TAKER_FEE;
                    equity *= 1.0 + net;
                    trades += 1;
                    chand_exits += 1;
                    rets.push(net);
                }
                pos = None;
                exited = true;
            }
        }

        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);
        eq_curve.push(equity);
        bar += 1;
    }

    // Close open position at end
    if let Some((sym, entry_bar, entry_px)) = pos {
        if let Some(sd) = sym_data.get(&sym) {
            let exit_px = sd
                .close
                .get((tend - 1).min(sd.close.len() - 1))
                .copied()
                .unwrap_or(entry_px);
            if entry_px > 0.0 && exit_px > 0.0 {
                let gross = (exit_px - entry_px) / entry_px;
                let net = gross - 2.0 * TAKER_FEE;
                equity *= 1.0 + net;
                trades += 1;
                rets.push(net);
            }
        }
    }

    let nr = rets.len().max(1) as f64;
    let mean = rets.iter().sum::<f64>() / nr;
    let std = (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / nr.max(1.0)).sqrt();
    let sharpe = if std > 0.0 {
        (mean / std) * (252.0_f64.sqrt())
    } else {
        0.0
    };

    (
        (equity - 1.0) * 100.0,
        sharpe,
        max_dd * 100.0,
        trades,
        chand_exits,
        eq_curve,
    )
}

// ─── Scoring ─────────────────────────────────────────────────────────────────

fn score_agg(r: &(usize, f64, f64, f64, usize, usize), windows: usize) -> f64 {
    let qr = r.4 as f64 / (UNIVERSES.len() * windows) as f64;
    qr * 20.0 + r.1.max(0.0)
}

// ─── CSV Export ───────────────────────────────────────────────────────────────

fn export_equity_csv(
    period_results: &HashMap<usize, PeriodAgg>,
    atr_periods: &[usize],
) -> Result<()> {
    let csv_path = "snapshots/chandelier_atr_period_equity.csv";
    let mut f = File::create(csv_path)?;

    let mut header = "bar".to_string();
    for &p in atr_periods {
        header.push_str(&format!(",atr_{}", p));
    }
    writeln!(f, "{}", header)?;

    let max_len = period_results
        .values()
        .map(|a| a.merged_equity.len())
        .max()
        .unwrap_or(0);

    for i in 0..max_len {
        let mut parts = vec![format!("{}", i)];
        for &p in atr_periods {
            if let Some(agg) = period_results.get(&p) {
                let v = agg
                    .merged_equity
                    .get(i)
                    .map(|x| format!("{:.6}", x))
                    .unwrap_or_default();
                parts.push(v);
            } else {
                parts.push(String::new());
            }
        }
        writeln!(f, "{}", parts.join(","))?;
    }
    println!("\n  Equity CSV -> {}", csv_path);
    Ok(())
}

fn export_summary_csv(
    ranked: &[(usize, f64, f64, f64, usize, usize)],
    windows: usize,
) -> Result<()> {
    let csv_path = "snapshots/chandelier_atr_period_sweep.csv";
    let mut f = File::create(csv_path)?;
    writeln!(
        f,
        "atr_period,avg_sharpe,avg_ret_pct,avg_dd_pct,q_pct,total_chand"
    )?;
    for &(atr_p, sh, ret, dd, qp, chand) in ranked {
        let qp_pct = qp as f64 / (UNIVERSES.len() * windows) as f64 * 100.0;
        writeln!(f, "{},{},{},{},{},{}", atr_p, sh, ret, dd, qp_pct, chand)?;
    }
    std::fs::copy(csv_path, "snapshots/chandelier_atr_period_sweep_latest.csv")?;
    println!("  Summary CSV -> snapshots/chandelier_atr_period_sweep_latest.csv");
    Ok(())
}

fn save_snapshots(
    ranked: &[(usize, f64, f64, f64, usize, usize)],
    winner: usize,
    elapsed: std::time::Duration,
) -> Result<()> {
    let ts = chrono_lite_timestamp();
    let md_path = format!("snapshots/chandelier_atr_period_hyperopt_{}.md", ts);
    let latest_md = "snapshots/chandelier_atr_period_hyperopt_latest.md";

    let mut f = File::create(&md_path)?;
    writeln!(f, "# Chandelier ATR Period Hyperopt -- {}", ts)?;
    writeln!(
        f,
        "\nMultiplier={} (fixed) | Baseline: Fixed {} bar hold",
        ATR_MULT, HOLD_BASELINE
    )?;
    writeln!(f, "\n**Global Winner: ATR Period = {}**\n", winner)?;
    writeln!(f, "Time: {:.1}s\n", elapsed.as_secs_f64())?;
    writeln!(
        f,
        "| Period | Avg Sharpe | Avg Ret% | Avg DD% | Q-Pass% | ChandExits |"
    )?;
    writeln!(
        f,
        "|--------|------------|---------|---------|---------|------------|"
    )?;

    let n_uni = UNIVERSES.len();
    for &(atr_p, sh, ret, dd, qp, chand) in ranked.iter().take(19) {
        let qp_pct = qp as f64 / (n_uni * 6) as f64 * 100.0;
        let marker = if atr_p == winner { " *" } else { "" };
        writeln!(
            f,
            "| **{}**{} | {:+.3} | {:+.1}% | {:.1}% | {:5.0}% | {} |",
            atr_p, marker, sh, ret, dd, qp_pct, chand
        )?;
    }

    std::fs::copy(&md_path, latest_md)?;
    println!("  Snapshots -> {}", md_path);
    Ok(())
}

fn chrono_lite_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}", 2026, 4, 9, h, m, s)
}

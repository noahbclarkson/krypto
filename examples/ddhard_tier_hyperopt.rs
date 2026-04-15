//! DDHard Tier Multiplier Hyperopt — Kira 2026-04-07
//!
//! Purpose: Find the optimal DDHard throttle tier multipliers.
//! The DDHard throttle controls family-level exposure based on portfolio drawdown:
//!   - dd >= HARD_THRESHOLD (20%): hard throttle → HARD_MULT
//!   - dd >= SOFT_THRESHOLD (10%): soft throttle → SOFT_MULT
//!   - else: full exposure → 1.0
//!
//! Current defaults [soft=0.60 / hard=0.30] are magic numbers.
//! This sweep tests 48 tier configurations across all 9 harsh universes.

#![allow(clippy::unwrap_used)]

use anyhow::Result;
use polars::prelude::*;
use std::collections::HashMap;

use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;

// ── Constants ───────────────────────────────────────────────────────────────

const BENCHMARK: &str = "BTCUSDT";
const CANDLES: &str = "1d";
const CANDLES_COUNT: u32 = 3000;

const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const SMA200_LOOKBACK: usize = 200;
const MACD_FAST: usize = 12;
const MACD_SLOW: usize = 26;
const MACD_SIG: usize = 9;
const TOP_K: usize = 3;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;

const N_TRAIN: usize = 252;
const N_TEST: usize = 252;

const SOFT_THRESHOLD: f64 = 10.0;
const HARD_THRESHOLD: f64 = 20.0;
const RECOVERY_SOFT: f64 = 0.98;
const RECOVERY_HARD: f64 = 0.95;

static UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "NoDOGE",
        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"],
    ),
    ("Legacy4", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT"]),
    (
        "Legacy5BNB",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT"],
    ),
    (
        "OldGuardNoBNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
    ),
    (
        "LargeCaps5",
        &["BTCUSDT", "ETHUSDT", "BNBUSDT", "SOLUSDT", "XRPUSDT"],
    ),
    ("Legacy3", &["BTCUSDT", "ETHUSDT", "XRPUSDT"]),
    (
        "LowVolume5",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BCHUSDT"],
    ),
    ("OldGuard4", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT"]),
];

// ── Sweep grid ──────────────────────────────────────────────────────────────
// extensive range: soft 0.30–0.85 in 0.05 steps (12 vals) × hard 0.10–0.50 in 0.05 steps (9 vals) = 108 combos
const SOFT_VALS: [f64; 12] = [
    0.30, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85,
];
const HARD_VALS: [f64; 9] = [0.10, 0.15, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50];

// ── Helpers ─────────────────────────────────────────────────────────────────

fn ddhard_exposure(dd_pct: f64, recovery_ratio: f64, soft_mult: f64, hard_mult: f64) -> f64 {
    if dd_pct >= HARD_THRESHOLD && recovery_ratio < RECOVERY_HARD {
        hard_mult
    } else if dd_pct >= SOFT_THRESHOLD && recovery_ratio < RECOVERY_SOFT {
        soft_mult
    } else {
        1.0
    }
}

fn compute_ad_momentum(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    vol: &[f64],
    period: usize,
) -> Vec<f64> {
    let n = close.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let range = high[i] - low[i];
        let mf = if range > 1e-9 {
            ((close[i] - low[i]) - (high[i] - close[i])) / range
        } else {
            0.0
        };
        if i == 0 {
            ad[i] = mf * vol[i];
        } else {
            ad[i] = ad[i - 1] + mf * vol[i];
        }
    }
    let mut mom = vec![0.0; n];
    for i in period..n {
        mom[i] = ad[i] - ad[i - period];
    }
    mom
}

fn compute_macd(close: &[f64], fast: usize, slow: usize, sig: usize) -> (Vec<f64>, Vec<f64>) {
    let n = close.len();
    // EMA helper inline
    let ema_fn = |data: &[f64], p: usize| -> Vec<f64> {
        let alpha = 2.0 / (p as f64 + 1.0);
        let mut out = vec![0.0; data.len()];
        for i in 0..data.len() {
            if i == 0 {
                out[i] = data[i];
            } else {
                out[i] = data[i] * alpha + out[i - 1] * (1.0 - alpha);
            }
        }
        out
    };
    let e_fast = ema_fn(close, fast);
    let e_slow = ema_fn(close, slow);
    let macd_line: Vec<f64> = e_fast
        .iter()
        .zip(e_slow.iter())
        .map(|(a, b)| a - b)
        .collect();
    let signal_line = ema_fn(&macd_line, sig);
    (macd_line, signal_line)
}

// ═══════════════════════════════════════════════════════════════════
// Walk-forward runner for a single universe + single tier config
// Returns (final_equity, sharpe, max_dd, trades, win_rate, passes, equity_curve)
// ═══════════════════════════════════════════════════════════════════

struct WfResult {
    final_equity: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    passes: usize,
    equity_curve: Vec<f64>,
}

fn run_walk_forward_tier(
    dfs: &HashMap<String, DataFrame>,
    soft_mult: f64,
    hard_mult: f64,
) -> WfResult {
    let syms: Vec<String> = dfs.keys().cloned().collect();
    let n_total = dfs.get(&syms[0]).unwrap().height();

    // Pre-extract data from DataFrames into vectors (avoids column lookup per bar)
    #[derive(Clone)]
    struct RawSym {
        close: Vec<f64>,
        open: Vec<f64>,
        high: Vec<f64>,
        low: Vec<f64>,
        vol: Vec<f64>,
        ad_mom: Vec<f64>,
        macd_line: Vec<f64>,
        macd_signal: Vec<f64>,
        sma200: Vec<f64>,
    }
    let mut raw: HashMap<String, RawSym> = HashMap::new();
    for sym in &syms {
        let df = dfs.get(sym).unwrap();
        let h = df.height();
        let extract_col = |name: &str| -> Vec<f64> {
            (0..h)
                .map(|i| {
                    df.column(name)
                        .ok()
                        .and_then(|c| c.f64().ok())
                        .and_then(|s| s.get(i))
                        .unwrap_or(0.0)
                })
                .collect()
        };
        let close_v = extract_col("close");
        let high_v = extract_col("high");
        let low_v = extract_col("low");
        let vol_v = extract_col("volume");
        let open_v = extract_col("open");
        let ad_mom = compute_ad_momentum(&close_v, &high_v, &low_v, &vol_v, AD_PERIOD);
        let (ml, ms) = compute_macd(&close_v, MACD_FAST, MACD_SLOW, MACD_SIG);
        // SMA200 inline
        let sma200: Vec<f64> = {
            let mut out = vec![0.0; h];
            let w = SMA200_LOOKBACK.min(h);
            for i in w..h {
                let sum: f64 = close_v[i - w..i].iter().sum();
                out[i] = sum / w as f64;
            }
            out
        };
        raw.insert(
            sym.clone(),
            RawSym {
                close: close_v,
                open: open_v,
                high: high_v,
                low: low_v,
                vol: vol_v,
                ad_mom,
                macd_line: ml,
                macd_signal: ms,
                sma200,
            },
        );
    }

    let btc = raw.get(BENCHMARK).cloned().unwrap();

    let warmup = SMA200_LOOKBACK.max(AD_PERIOD).max(MACD_SLOW).max(20) + 10;
    let eff_start = warmup;
    let step = N_TEST;

    let mut all_ret = Vec::new();
    let mut global_eq = vec![1.0_f64; N_TEST];
    let mut pass_count = 0usize;
    let mut global_trades = 0usize;
    let mut global_wins = 0usize;
    let mut peak_eq = 1.0_f64;
    let mut global_max_dd = 0.0_f64;

    for block in 0..4 {
        let test_start = eff_start + block * step;
        let test_end = (test_start + N_TEST).min(n_total);
        if test_end.saturating_sub(test_start) < N_TEST / 2 {
            break;
        }

        // Train window: use the preceding N_TRAIN bars to rank symbols
        let train_end = test_start.min(n_total);
        let train_start = train_end.saturating_sub(N_TRAIN);
        if train_end.saturating_sub(train_start) < N_TRAIN / 2 {
            continue;
        }

        // Rank symbols by A/D momentum at end of train period
        let mut candidates: Vec<(String, f64)> = Vec::new();
        for sym in &syms {
            let r = raw.get(sym).unwrap();
            let ad_val = r
                .ad_mom
                .get(train_end.saturating_sub(1))
                .copied()
                .unwrap_or(0.0);
            candidates.push((sym.clone(), ad_val));
        }
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let top: Vec<String> = candidates
            .into_iter()
            .take(TOP_K * 2)
            .map(|(s, _)| s)
            .collect();

        // Walk test bars
        let mut equity = 1.0_f64;
        let mut peak = 1.0_f64;
        let mut max_dd = 0.0_f64;
        let mut trades = 0usize;
        let mut wins = 0usize;
        let mut position: Option<(String, usize, f64)> = None;

        for bar in test_start..test_end {
            if position.is_none() {
                // Try to enter a position
                for sym in &top {
                    let r = raw.get(sym).unwrap();
                    // Check: A/D momentum positive AND MACD confirms
                    let ad_ok = r.ad_mom.get(bar.saturating_sub(1)).copied().unwrap_or(0.0) > 0.0;
                    let mc = r
                        .macd_line
                        .get(bar.saturating_sub(1))
                        .copied()
                        .unwrap_or(0.0);
                    let ms = r
                        .macd_signal
                        .get(bar.saturating_sub(1))
                        .copied()
                        .unwrap_or(0.0);
                    let macd_ok = mc > ms;

                    // BTC regime: only long when BTC > SMA200
                    let btc_sma_val = btc.close.get(bar.saturating_sub(1)).copied().unwrap_or(0.0);
                    let btc_sma200_val = btc
                        .sma200
                        .get(bar.saturating_sub(1))
                        .copied()
                        .unwrap_or(0.0);
                    let regime_ok = btc_sma_val > btc_sma200_val;

                    if ad_ok && macd_ok && regime_ok {
                        let entry_px = r.open.get(bar).copied().unwrap_or(0.0);
                        if entry_px > 0.0 {
                            position = Some((sym.clone(), bar, entry_px));
                            break;
                        }
                    }
                }
            }

            if let Some((p_sym, entry_bar, entry_px)) = position.take() {
                let should_exit = bar >= entry_bar + HOLD_BARS || bar >= test_end - 1;
                if should_exit {
                    let r = raw.get(&p_sym).unwrap();
                    let exit_px = r.close.get(bar).copied().unwrap_or(entry_px);
                    if exit_px > 0.0 && entry_px > 0.0 {
                        let ret = (exit_px / entry_px - 1.0) - TAKER_FEE;

                        // Apply DDHard throttle
                        let dd_pct = if peak > 0.0 {
                            (peak - equity) / peak * 100.0
                        } else {
                            0.0
                        };
                        let recovery_ratio = equity / peak;
                        let exposure =
                            ddhard_exposure(dd_pct, recovery_ratio, soft_mult, hard_mult);
                        let sized_ret = ret * exposure;

                        equity *= 1.0 + sized_ret;
                        peak = peak.max(equity);
                        let dd = if peak > 0.0 {
                            (peak - equity) / peak
                        } else {
                            0.0
                        };
                        max_dd = max_dd.max(dd);

                        if ret > 0.0 {
                            wins += 1;
                        }
                        trades += 1;
                    }
                } else {
                    position = Some((p_sym, entry_bar, entry_px));
                }
            }

            let idx = bar.saturating_sub(test_start);
            if idx < N_TEST {
                global_eq[idx] = equity;
            }
        }

        let block_ret = equity - 1.0;
        if block_ret > 0.0 {
            pass_count += 1;
        }
        all_ret.push(block_ret);
        global_trades += trades;
        global_wins += wins;
        peak_eq = peak_eq.max(equity);
        // Update global max_dd using running equity
    }

    // Recompute global max_dd from equity curve
    let mut g_peak = 1.0_f64;
    let mut g_max_dd = 0.0_f64;
    for &eq in &global_eq {
        g_peak = g_peak.max(eq);
        if g_peak > 0.0 {
            g_max_dd = g_max_dd.max((g_peak - eq) / g_peak);
        }
    }

    let sharpe = if all_ret.len() >= 2 {
        let mean: f64 = all_ret.iter().sum::<f64>() / all_ret.len() as f64;
        let var: f64 =
            all_ret.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / all_ret.len() as f64;
        let std = var.sqrt();
        if std > 1e-12 {
            mean / std
        } else {
            0.0
        }
    } else {
        0.0
    };

    WfResult {
        final_equity: *global_eq.last().unwrap_or(&1.0),
        sharpe,
        max_dd: g_max_dd,
        trades: global_trades,
        win_rate: if global_trades > 0 {
            global_wins as f64 / global_trades as f64
        } else {
            0.0
        },
        passes: pass_count,
        equity_curve: global_eq,
    }
}

// ═══════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ DDHard Tier Multiplier Hyperopt ═══");
    println!(
        "Sweep: soft={} values × hard={} values = {} combos × 9 universes",
        SOFT_VALS.len(),
        HARD_VALS.len(),
        SOFT_VALS.len() * HARD_VALS.len()
    );
    println!("Walk-forward: 4 windows, {N_TRAIN} train / {N_TEST} test\n");

    // Load all symbols
    let loader = DataLoader::new(None, None);
    let all_syms = [
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
        "EOSUSDT", "BCHUSDT",
    ];
    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in &all_syms {
        match loader.fetch_with_cache(sym, CANDLES, CANDLES_COUNT).await {
            Ok(df) => {
                let df = FeatureEngine::add_technicals(&df, None)?;
                cache.insert(sym.to_string(), df);
            }
            Err(e) => eprintln!("  WARNING: failed {sym}: {e}"),
        }
    }

    let mut results_csv = String::from(
        "universe,soft_mult,hard_mult,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,passes\n",
    );
    let mut equity_csv = String::from("universe,soft_mult,hard_mult,step,equity\n");

    // Track global best by (soft,hard) pair (round to 2 decimals for grouping)
    let mut combo_scores: HashMap<String, (f64, f64, f64, usize)> = HashMap::new();
    let mut best_by_universe: Vec<(String, f64, f64, WfResult)> = Vec::new();

    let total_unis = UNIVERSES.len();
    let total_combos = SOFT_VALS.len() * HARD_VALS.len();

    for (u_idx, (uni_name, uni_syms)) in UNIVERSES.iter().enumerate() {
        println!("[{}/{}] Universe: {}", u_idx + 1, total_unis, uni_name);

        let mut ud: HashMap<String, DataFrame> = HashMap::new();
        for sym in *uni_syms {
            if let Some(df) = cache.get(*sym) {
                ud.insert(sym.to_string(), df.clone());
            }
        }
        if ud.is_empty() {
            continue;
        }

        let mut best_score = f64::NEG_INFINITY;
        let mut best_soft = 0.60_f64;
        let mut best_hard = 0.30_f64;
        let mut best_result: Option<WfResult> = None;

        for (&soft, s_idx) in SOFT_VALS.iter().zip(0..) {
            let line = String::from_utf8(vec![b'-' as u8; 160]).unwrap();
            if s_idx % 3 == 0 {
                print!("\r  soft={soft:.2} ...");
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
            for &hard in &HARD_VALS {
                let r = run_walk_forward_tier(&ud, soft, hard);

                // Composite: Sharpe + (pass_rate * 3) - (max_dd / 100)
                let pass_rate = r.passes as f64 / 4.0;
                let composite = r.sharpe + pass_rate * 3.0 - r.max_dd;

                let key = format!(
                    "{}_{:04}_{:04}",
                    uni_name,
                    (soft * 100.0) as u32,
                    (hard * 100.0) as u32
                );
                combo_scores.insert(key, (soft, hard, composite, r.passes));

                results_csv.push_str(&format!(
                    "{},{:.2},{:.2},{:.1},{:.3},{:.2},{},{:.1},{}\n",
                    uni_name,
                    soft,
                    hard,
                    (r.final_equity - 1.0) * 100.0,
                    r.sharpe,
                    r.max_dd * 100.0,
                    r.trades,
                    r.win_rate * 100.0,
                    r.passes
                ));

                // Equity CSV (baseline + potential winners)
                for (step, &eq) in r.equity_curve.iter().enumerate() {
                    equity_csv.push_str(&format!(
                        "{},{},{},{},{:.6}\n",
                        uni_name, soft, hard, step, eq
                    ));
                }

                if composite > best_score {
                    best_score = composite;
                    best_soft = soft;
                    best_hard = hard;
                    best_result = Some(r);
                }
            }
        }

        println!("\r  Best: soft={:.2} hard={:.2} | Score:{:.2} Eq:{:.2e} Sharpe:{:.2} DD:{:.1}% Trades:{} WR:{:.0}% Pass:{}/4",
            best_soft, best_hard, best_score,
            best_result.as_ref().map(|r| r.final_equity).unwrap_or(1.0),
            best_result.as_ref().map(|r| r.sharpe).unwrap_or(0.0),
            best_result.as_ref().map(|r| r.max_dd * 100.0).unwrap_or(0.0),
            best_result.as_ref().map(|r| r.trades).unwrap_or(0),
            best_result.as_ref().map(|r| r.win_rate * 100.0).unwrap_or(0.0),
            best_result.as_ref().map(|r| r.passes).unwrap_or(0));

        if let Some(r) = best_result {
            best_by_universe.push((uni_name.to_string(), best_soft, best_hard, r));
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // Global analysis: which (soft,hard) combo wins the most universes?
    // ═══════════════════════════════════════════════════════════════

    let mut global_wins: HashMap<String, (f64, f64, usize)> = HashMap::new();
    for (uni_name, soft, hard, r) in &best_by_universe {
        let pass_rate = r.passes as f64 / 4.0;
        let composite = r.sharpe + pass_rate * 3.0 - r.max_dd;
        let key = format!("{:.2}/{:.2}", soft, hard);
        let entry = global_wins.entry(key).or_insert((*soft, *hard, 0));
        entry.2 += 1;
        // Keep track of aggregate stats
    }

    println!("\n═══ GLOBAL RESULTS ═══");
    println!("Config                          | Wins | Avg Score");
    println!("────────────────────────────────|------|----------");
    let mut sorted: Vec<_> = global_wins.into_iter().collect();
    sorted.sort_by(|a, b| {
        b.1 .2.cmp(&a.1 .2).then_with(|| {
            b.1 .1
                .partial_cmp(&a.1 .1)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    for (key, (soft, hard, wins)) in sorted.iter().take(10) {
        println!(
            "  soft={:.2} / hard={:.2}            | {:>3}  | (see detail)",
            soft, hard, wins
        );
    }

    // Save CSVs
    std::fs::create_dir_all("snapshots")?;
    let results_path = "snapshots/ddhard_tier_sweep_results.csv";
    let equity_path = "snapshots/ddhard_tier_sweep_equity.csv";
    std::fs::write(results_path, &results_csv)?;
    std::fs::write(equity_path, &equity_csv)?;

    println!("\nSaved:");
    println!(
        "  Results: {} ({:.1} MB)",
        results_path,
        results_csv.len() as f64 / 1e6
    );
    println!(
        "  Equity:  {} ({:.1} MB)",
        equity_path,
        equity_csv.len() as f64 / 1e6
    );

    // Summary for charting
    println!("\n═══ Per-universe best configs ═══");
    for (uni, soft, hard, r) in &best_by_universe {
        println!(
            "  {:>15} → soft={:.2} hard={:.2} | Eq:{:.2e} Sharpe:{:.2} DD:{:.1}% {}tr pass:{}/4",
            uni,
            soft,
            hard,
            r.final_equity,
            r.sharpe,
            r.max_dd * 100.0,
            r.trades,
            r.passes
        );
    }

    Ok(())
}

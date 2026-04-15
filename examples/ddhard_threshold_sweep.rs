//! =========================================================
//! HYPERPARAMETER OPTIMIZATION: DD-Hard Tier Thresholds
//! =========================================================
//!
//! PURPOSE: Systematically test DD-hard exposure tier thresholds.
//!
//! CURRENT (undocumented magic numbers):
//!   DD >= 30% → 30% exposure (hard cut)
//!   DD >= 15% → 60% exposure (soft cut)
//!   else        → 100% exposure
//!
//! THEORY: The 30/15 split was never justified. It was chosen arbitrarily.
//! In a 252-bar test window, a 30% drawdown is CATASTROPHIC for a crypto trader.
//! Tighter thresholds (e.g., 20%/10%) would reduce exposure earlier but also
//! reduce compounding in benign periods.
//!
//! SWEEP:
//!   HARD threshold: 15% to 50% in steps of 2.5% (14 values)
//!   SOFT threshold: 5% to 30% in steps of 2.5% (11 values)
//!   Constraint: SOFT < HARD always
//!   Total: 154 tier configurations
//!
//! METHOD:
//!   - Walk-forward 252/252 on 9 universes
//!   - Uses A/D Momentum (period=47) as the underlying signal
//!   - Full DDBudget-style sleeve weighting across A/D + MACD+Regime + SmallVol
//!   - Exports equity curves for top configs
//!   - 0.1% taker fee per side
//!
//! METRIC: Composite of (quarter passes × avg Sharpe × robustness)
//!
//! Kira — 2026-04-09

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_AD: usize = 54;
const HOLD_OTHER: usize = 21;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const WARMUP: usize = 200;

const UNIVERSES: &[(&str, &[&str])] = &[
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
    (
        "Legacy4",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"],
    ),
    (
        "Legacy5BNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
        ],
    ),
    (
        "OldGuardNoBNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
    ),
    (
        "LargeCaps5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT",
        ],
    ),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    (
        "OldGuard4",
        &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
];

// ── Threshold sweep grid ─────────────────────────────────────────────────────
// Hard: 15% to 50% in 2.5% steps
// Soft: 5% to 30% in 2.5% steps
// Only keep (soft, hard) where soft < hard
fn build_threshold_grid() -> Vec<(f64, f64)> {
    let mut combos = Vec::new();
    for &hard in &[
        15.0, 17.5, 20.0, 22.5, 25.0, 27.5, 30.0, 32.5, 35.0, 37.5, 40.0, 42.5, 45.0, 47.5, 50.0,
    ] {
        for &soft in &[
            5.0, 7.5, 10.0, 12.5, 15.0, 17.5, 20.0, 22.5, 25.0, 27.5, 30.0,
        ] {
            if soft < hard {
                combos.push((soft, hard));
            }
        }
    }
    combos
}

// ── DD-hard exposure function (parametrized) ─────────────────────────────────
fn ddhard_exposure(dd_pct: f64, soft: f64, hard: f64) -> f64 {
    if dd_pct >= hard {
        0.30
    } else if dd_pct >= soft {
        0.60
    } else {
        1.0
    }
}

// ── Data structures ───────────────────────────────────────────────────────────
struct SymData {
    close: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

#[derive(Clone)]
struct ThreshResult {
    soft: f64,
    hard: f64,
    quarter_passes: usize,
    windows: usize,
    avg_ret: f64,
    avg_sharpe: f64,
    worst_dd: f64,
    avg_trades: f64,
    equity_curve: Vec<f64>,
}

struct WfWindow {
    ret: f64,
    sh: f64,
    dd: f64,
    trades: usize,
    passed: bool,
    equity: Vec<f64>,
}

// ── A/D signal (train-anchored) ───────────────────────────────────────────────
fn ad_signal(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    vol: &[f64],
    period: usize,
    train_end: usize,
    bar: usize,
) -> Option<f64> {
    let warmup = WARMUP.max(period);
    if bar < warmup || bar > train_end {
        return None;
    }
    let alpha1 = 2.0 / (period as f64 + 1.0);
    let alpha2 = 2.0 / (period as f64 * 2.0 + 1.0);
    let mut ema1 = 0.0_f64;
    let mut ema2 = 0.0_f64;
    for j in warmup..=bar.min(train_end) {
        let range = high[j] - low[j];
        let mf = if range > 1e-9 {
            ((close[j] - low[j]) - (high[j] - close[j])) / range
        } else {
            0.0
        };
        ema1 = alpha1 * mf * vol[j] + (1.0 - alpha1) * ema1;
        ema2 = alpha2 * mf * vol[j] + (1.0 - alpha2) * ema2;
    }
    let ad_now = ema1 - ema2;
    let mut sum = 0.0_f64;
    let mut cnt = 0usize;
    for j in warmup..train_end {
        let range = high[j] - low[j];
        let mf = if range > 1e-9 {
            ((close[j] - low[j]) - (high[j] - close[j])) / range
        } else {
            0.0
        };
        sum += mf * vol[j];
        cnt += 1;
    }
    let ad_mean = if cnt > 0 { sum / cnt as f64 } else { 0.0 };
    Some(ad_now - ad_mean)
}

fn macd_signal(close: &[f64], train_end: usize, bar: usize) -> Option<f64> {
    let warmup = WARMUP.max(26).max(9);
    if bar < warmup || bar > train_end {
        return None;
    }
    let ef_alp = 2.0 / (12.0 + 1.0);
    let es_alp = 2.0 / (26.0 + 1.0);
    let em_alp = 2.0 / (9.0 + 1.0);
    let mut ef = 0.0_f64;
    let mut es = 0.0_f64;
    let mut sig_line = 0.0_f64;
    for j in warmup..=bar.min(train_end) {
        ef = ef_alp * close[j] + (1.0 - ef_alp) * ef;
        es = es_alp * close[j] + (1.0 - es_alp) * es;
        let ml = ef - es;
        sig_line = em_alp * ml + (1.0 - em_alp) * sig_line;
    }
    let sma_start = bar.saturating_sub(200).max(warmup);
    let mut sma_sum = 0.0_f64;
    let mut sma_cnt = 0usize;
    for j in sma_start..=bar.min(train_end) {
        sma_sum += close[j];
        sma_cnt += 1;
    }
    let sma200 = if sma_cnt > 0 {
        sma_sum / sma_cnt as f64
    } else {
        close[bar]
    };
    let regime = if close[bar] > sma200 { 1.0 } else { -1.0 };
    Some(regime * (ef - es - sig_line))
}

fn smallvol_rank(
    all_close: &HashMap<&str, &[f64]>,
    all_vol: &HashMap<&str, &[f64]>,
    sym: &str,
    bar: usize,
) -> Option<f64> {
    let my_c = all_close.get(sym)?;
    let my_v = all_vol.get(sym)?;
    if bar >= my_c.len() || bar >= my_v.len() {
        return None;
    }
    let my_dv = my_c[bar] * my_v[bar];
    let mut dvs: Vec<f64> = Vec::new();
    for (s, c) in all_close {
        if let Some(v) = all_vol.get(s) {
            if bar < c.len() && bar < v.len() {
                dvs.push(c[bar] * v[bar]);
            }
        }
    }
    if dvs.is_empty() {
        return Some(0.0);
    }
    dvs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pos = dvs
        .iter()
        .position(|&x| x >= my_dv)
        .unwrap_or(dvs.len() - 1);
    Some((dvs.len() - pos) as f64)
}

// ── Run one threshold config on one universe's walk-forward ──────────────────
fn run_thresh_wf(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    train_end: usize,
    test_start: usize,
    test_end: usize,
    soft_thresh: f64,
    hard_thresh: f64,
) -> WfWindow {
    let all_close: HashMap<&str, &[f64]> = sym_data
        .iter()
        .map(|(k, v)| (k.as_str(), v.close.as_slice()))
        .collect();
    let all_vol: HashMap<&str, &[f64]> = sym_data
        .iter()
        .map(|(k, v)| (k.as_str(), v.vol.as_slice()))
        .collect();

    let mut ad_eq = 1.0_f64;
    let mut mc_eq = 1.0_f64;
    let mut sm_eq = 1.0_f64;
    let mut ad_pk = 1.0_f64;
    let mut mc_pk = 1.0_f64;
    let mut sm_pk = 1.0_f64;
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut total_trades = 0usize;
    let mut wins = 0usize;
    let mut equity_curve = vec![1.0_f64];

    let mut bar = test_start;
    while bar + HOLD_AD.max(HOLD_OTHER) + 2 < test_end {
        let mut ad_scores: Vec<(&str, f64)> = Vec::new();
        let mut mc_scores: Vec<(&str, f64)> = Vec::new();
        let mut sm_scores: Vec<(&str, f64)> = Vec::new();

        for sym in symbols {
            let Some(sd) = sym_data.get(sym) else {
                continue;
            };
            if bar >= sd.close.len() {
                continue;
            }

            if let Some(score) = ad_signal(
                &sd.close, &sd.high, &sd.low, &sd.vol, AD_PERIOD, train_end, bar,
            ) {
                ad_scores.push((sym.as_str(), score));
            }
            if let Some(score) = macd_signal(&sd.close, train_end, bar) {
                mc_scores.push((sym.as_str(), score));
            }
            if let Some(score) = smallvol_rank(&all_close, &all_vol, sym.as_str(), bar) {
                sm_scores.push((sym.as_str(), score));
            }
        }

        ad_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        mc_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        sm_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        if ad_scores.is_empty() && mc_scores.is_empty() && sm_scores.is_empty() {
            bar += 1;
            continue;
        }

        // ── DD-hard sleeve weights ─────────────────────────────────────────
        let ad_dd = if ad_pk > 0.0 {
            (1.0 - ad_eq / ad_pk) * 100.0
        } else {
            0.0
        };
        let mc_dd = if mc_pk > 0.0 {
            (1.0 - mc_eq / mc_pk) * 100.0
        } else {
            0.0
        };
        let sm_dd = if sm_pk > 0.0 {
            (1.0 - sm_eq / sm_pk) * 100.0
        } else {
            0.0
        };

        let ad_w = ddhard_exposure(ad_dd, soft_thresh, hard_thresh);
        let mc_w = ddhard_exposure(mc_dd, soft_thresh, hard_thresh);
        let sm_w = ddhard_exposure(sm_dd, soft_thresh, hard_thresh);
        let tot_w = ad_w + mc_w + sm_w;
        let ad_w = ad_w / tot_w.max(1e-9);
        let mc_w = mc_w / tot_w.max(1e-9);
        let sm_w = sm_w / tot_w.max(1e-9);

        let mut port_ret = 0.0_f64;

        // AD sleeve
        let ad_top: Vec<_> = ad_scores.iter().take(2).map(|(s, v)| (*s, *v)).collect();
        let mut ad_rets = Vec::new();
        for &(sym, _) in &ad_top {
            let Some(sd) = sym_data.get(sym) else {
                continue;
            };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd
                .close
                .get((bar + 1 + HOLD_AD).min(sd.close.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                ad_rets.push(r);
                ad_pk = ad_pk.max(ad_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 {
                    wins += 1;
                }
            }
        }
        if !ad_rets.is_empty() {
            let avg = ad_rets.iter().sum::<f64>() / ad_rets.len() as f64;
            ad_eq *= 1.0 + avg;
            port_ret += ad_w * avg;
        }

        // MACD sleeve
        let mc_top: Vec<_> = mc_scores.iter().take(2).map(|(s, v)| (*s, *v)).collect();
        let mut mc_rets = Vec::new();
        for &(sym, _) in &mc_top {
            let Some(sd) = sym_data.get(sym) else {
                continue;
            };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd
                .close
                .get((bar + 1 + HOLD_OTHER).min(sd.close.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                mc_rets.push(r);
                mc_pk = mc_pk.max(mc_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 {
                    wins += 1;
                }
            }
        }
        if !mc_rets.is_empty() {
            let avg = mc_rets.iter().sum::<f64>() / mc_rets.len() as f64;
            mc_eq *= 1.0 + avg;
            port_ret += mc_w * avg;
        }

        // Small vol sleeve
        let sm_top: Vec<_> = sm_scores.iter().take(2).map(|(s, v)| (*s, *v)).collect();
        let mut sm_rets = Vec::new();
        for &(sym, _) in &sm_top {
            let Some(sd) = sym_data.get(sym) else {
                continue;
            };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd
                .close
                .get((bar + 1 + HOLD_OTHER).min(sd.close.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                sm_rets.push(r);
                sm_pk = sm_pk.max(sm_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 {
                    wins += 1;
                }
            }
        }
        if !sm_rets.is_empty() {
            let avg = sm_rets.iter().sum::<f64>() / sm_rets.len() as f64;
            sm_eq *= 1.0 + avg;
            port_ret += sm_w * avg;
        }

        equity *= 1.0 + port_ret;
        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);
        equity_curve.push(equity);
        bar += HOLD_AD + 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let test_days = (TEST_BARS) as f64;
    let ann_return = ret / 100.0 / (test_days / 365.0);
    let sharpe = if max_dd.abs() > 0.001 {
        ann_return / (max_dd.abs())
    } else {
        0.0
    };
    let passed = total_trades >= MIN_TRADES && ret > 0.0;

    WfWindow {
        ret,
        sh: sharpe,
        dd: max_dd * 100.0,
        trades: total_trades,
        passed,
        equity: equity_curve,
    }
}

// ── Run one threshold config across all universes ────────────────────────────
fn run_config(
    sym_data_map: &HashMap<String, SymData>,
    n: usize,
    soft_thresh: f64,
    hard_thresh: f64,
    config_idx: usize,
    total_configs: usize,
) -> ThreshResult {
    let mut total_ret = 0.0_f64;
    let mut total_sh = 0.0_f64;
    let mut worst_dd = 0.0_f64;
    let mut total_trades = 0usize;
    let mut total_passes = 0usize;
    let mut n_windows = 0usize;
    let mut merged_eq = vec![1.0_f64];

    for &(uni_name, symbols) in UNIVERSES {
        let syms: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = syms.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            continue;
        }

        let n_sym = syms
            .iter()
            .filter_map(|s| sym_data_map.get(s))
            .map(|sd| sd.close.len())
            .min()
            .unwrap_or(0);
        let nw = n_sym.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

        for wi in 0..nw {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n_sym);

            if test_end.saturating_sub(test_start) < HOLD_AD + 5 {
                continue;
            }

            let wf = run_thresh_wf(
                sym_data_map,
                &syms,
                train_end,
                test_start,
                test_end,
                soft_thresh,
                hard_thresh,
            );

            total_ret += wf.ret;
            total_sh += wf.sh;
            worst_dd = worst_dd.min(wf.dd);
            total_trades += wf.trades;
            if wf.passed {
                total_passes += 1;
            }
            n_windows += 1;

            // Extend merged equity
            if !merged_eq.is_empty() && !wf.equity.is_empty() {
                let base = *merged_eq.last().unwrap();
                merged_eq.extend(
                    wf.equity
                        .iter()
                        .skip(1)
                        .map(|&v| base * (v / wf.equity[0].max(1e-9))),
                );
            }
        }
    }

    if config_idx % 20 == 0 || config_idx == total_configs - 1 {
        let pct = config_idx as f64 / total_configs as f64 * 100.0;
        println!(
            "  [{:5.1}%] soft={:5.1}% hard={:5.1}%: QP={}/{} sh={:.3} ret={:+.1}%",
            pct,
            soft_thresh,
            hard_thresh,
            total_passes,
            n_windows,
            if n_windows > 0 {
                total_sh / n_windows as f64
            } else {
                0.0
            },
            if n_windows > 0 {
                total_ret / n_windows as f64
            } else {
                0.0
            }
        );
    }

    ThreshResult {
        soft: soft_thresh,
        hard: hard_thresh,
        quarter_passes: total_passes,
        windows: n_windows,
        avg_ret: if n_windows > 0 {
            total_ret / n_windows as f64
        } else {
            0.0
        },
        avg_sharpe: if n_windows > 0 {
            total_sh / n_windows as f64
        } else {
            0.0
        },
        worst_dd,
        avg_trades: if n_windows > 0 {
            total_trades as f64 / n_windows as f64
        } else {
            0.0
        },
        equity_curve: merged_eq,
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────
#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();
    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    println!("\n{}", "=".repeat(72));
    println!("  HYPEROPT: DD-Hard Tier Thresholds");
    println!("  Magic numbers under test: HARD=30%, SOFT=15%");
    println!("  Sweep: HARD ∈ [15–50%], SOFT ∈ [5–30%], 2.5% steps");
    println!("{}", "=".repeat(72));

    // ── Load data ──────────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                let n = df.height();
                min_len = min_len.min(n);
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => {
                eprintln!("  WARNING: {} load failed: {}", sym, e);
            }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let chunk = |name: &str| -> Vec<f64> {
                df.column(name)
                    .ok()
                    .and_then(|c| c.f64().ok())
                    .map(|c| c.into_iter().filter_map(|x| x).take(n).collect())
                    .unwrap_or_default()
            };
            sym_data_map.insert(
                sym.clone(),
                SymData {
                    close: chunk("close"),
                    open: chunk("open"),
                    high: chunk("high"),
                    low: chunk("low"),
                    vol: chunk("volume"),
                },
            );
        }
    }
    println!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // ── Build threshold grid ───────────────────────────────────────────────
    let grid = build_threshold_grid();
    println!(
        "Testing {} (soft, hard) threshold configurations\n",
        grid.len()
    );

    // ── Baseline: current magic numbers ──────────────────────────────────────
    let baseline_soft = 15.0;
    let baseline_hard = 30.0;
    println!("Baseline: SOFT={}% HARD={}%", baseline_soft, baseline_hard);
    let baseline_result = run_config(
        &sym_data_map,
        n,
        baseline_soft,
        baseline_hard,
        0,
        grid.len(),
    );
    println!(
        "  Baseline: QP={}/{} sh={:.3} ret={:+.1}% DD={:.1}%\n",
        baseline_result.quarter_passes,
        baseline_result.windows,
        baseline_result.avg_sharpe,
        baseline_result.avg_ret,
        baseline_result.worst_dd
    );

    // ── Run sweep ──────────────────────────────────────────────────────────
    let mut all_results: Vec<ThreshResult> = Vec::with_capacity(grid.len());
    for (idx, (soft, hard)) in grid.iter().enumerate() {
        let result = run_config(&sym_data_map, n, *soft, *hard, idx, grid.len());
        all_results.push(result);
    }

    // ── Sort by composite ──────────────────────────────────────────────────
    let max_qp = UNIVERSES.len() * 4; // 9 × 4 windows
    let mut sorted = all_results.clone();
    sorted.sort_by(|a, b| {
        b.quarter_passes
            .cmp(&a.quarter_passes)
            .then_with(|| {
                let ra = a.avg_ret / a.worst_dd.abs().max(0.01);
                let rb = b.avg_ret / b.worst_dd.abs().max(0.01);
                rb.partial_cmp(&ra).unwrap()
            })
            .then_with(|| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap())
    });

    let winner = &sorted[0];
    let runner1 = sorted.get(1).unwrap_or(winner);
    let runner2 = sorted.get(2).unwrap_or(winner);

    println!("\n{}", "=".repeat(72));
    println!("  TOP 10 THRESHOLD CONFIGURATIONS");
    println!(
        "  {:>6} | {:>6} | {:>5} | {:>10} | {:>8} | {:>7}",
        "Soft%", "Hard%", "QP/max", "AvgRet%", "Sharpe", "WorstDD%"
    );
    println!("{}", "-".repeat(60));
    for (i, r) in sorted.iter().take(10).enumerate() {
        let qp_max = format!("{}/{}", r.quarter_passes, max_qp);
        let marker = if i == 0 { " ★WINNER" } else { "" };
        let base_marker =
            if (r.soft - baseline_soft).abs() < 0.1 && (r.hard - baseline_hard).abs() < 0.1 {
                " (BASELINE)"
            } else {
                ""
            };
        println!(
            "  {:>6.1} | {:>6.1} | {:>5} | {:>+10.1}% | {:>+8.3} | {:>+7.1}%{}{}",
            r.soft, r.hard, qp_max, r.avg_ret, r.avg_sharpe, r.worst_dd, marker, base_marker
        );
    }

    let baseline_score = (baseline_result.quarter_passes as f64 / max_qp as f64)
        * (baseline_result.avg_ret / baseline_result.worst_dd.abs().max(0.01));
    let winner_score = (winner.quarter_passes as f64 / max_qp as f64)
        * (winner.avg_ret / winner.worst_dd.abs().max(0.01));
    let improvement = (winner_score / baseline_score - 1.0) * 100.0;

    println!("\n{}", "=".repeat(72));
    println!("  WINNER: SOFT={}% HARD={}%", winner.soft, winner.hard);
    println!(
        "  QP={}/{} | Sharpe={:.3} | AvgRet={:+.1}% | WorstDD={:.1}%",
        winner.quarter_passes, max_qp, winner.avg_sharpe, winner.avg_ret, winner.worst_dd
    );
    println!(
        "  vs Baseline: ΔQP={} ΔSharpe={:+.3} ΔDD={:+.1}%",
        winner.quarter_passes as i32 - baseline_result.quarter_passes as i32,
        winner.avg_sharpe - baseline_result.avg_sharpe,
        winner.worst_dd - baseline_result.worst_dd
    );
    println!("{}", "=".repeat(72));

    // ── Write CSV ────────────────────────────────────────────────────────────
    let csv_path = "snapshots/ddhard_threshold_sweep.csv";
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(csv_path)?;
    writeln!(f, "soft_pct,hard_pct,quarter_passes,windows,max_qp,avg_ret_pct,avg_sharpe,worst_dd_pct,avg_trades,is_baseline,is_winner")?;
    for r in &all_results {
        let is_base = (r.soft - baseline_soft).abs() < 0.1 && (r.hard - baseline_hard).abs() < 0.1;
        let is_win = (r.soft - winner.soft).abs() < 0.1 && (r.hard - winner.hard).abs() < 0.1;
        writeln!(
            f,
            "{:.1},{:.1},{},{},{},{:.2},{:.4},{:.2},{:.1},{},{}",
            r.soft,
            r.hard,
            r.quarter_passes,
            r.windows,
            max_qp,
            r.avg_ret,
            r.avg_sharpe,
            r.worst_dd,
            r.avg_trades,
            if is_base { "BASELINE" } else { "" },
            if is_win { "WINNER" } else { "" }
        )?;
    }
    println!("\nCSV: {}", csv_path);

    // ── Export equity curves for baseline + winner + runners ───────────────
    for label in &["baseline", "winner", "runner1", "runner2"] {
        let curve = match *label {
            "baseline" => &baseline_result.equity_curve,
            "winner" => &winner.equity_curve,
            "runner1" => &runner1.equity_curve,
            "runner2" => &runner2.equity_curve,
            _ => continue,
        };
        let soft = match *label {
            "baseline" => baseline_soft,
            "winner" => winner.soft,
            "runner1" => runner1.soft,
            "runner2" => runner2.soft,
            _ => 0.0,
        };
        let hard = match *label {
            "baseline" => baseline_hard,
            "winner" => winner.hard,
            "runner1" => runner1.hard,
            "runner2" => runner2.hard,
            _ => 0.0,
        };

        let eq_path = format!("snapshots/eqcurve_ddhard_{}.csv", label);
        let mut ef = File::create(&eq_path)?;
        writeln!(ef, "bar,equity")?;
        for (i, &v) in curve.iter().enumerate() {
            writeln!(ef, "{},{:.6}", i, v)?;
        }
        println!("  Saved equity curve: {}", eq_path);
    }

    // ── Generate Python chart ───────────────────────────────────────────────
    // Build script with explicit float values to avoid f-string brace issues
    let script = build_chart_script(
        winner.soft,
        winner.hard,
        baseline_soft,
        baseline_hard,
        improvement,
    );
    let script_path = "scripts/plot_ddhard_threshold.py";
    std::fs::write(script_path, &script)?;
    println!("\nRunning chart script...");
    let status = std::process::Command::new("python3")
        .current_dir(".")
        .arg(script_path)
        .output();

    match status {
        Ok(out) => {
            println!("{}", String::from_utf8_lossy(&out.stdout));
            if !out.status.success() {
                eprintln!("Chart stderr: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
        Err(e) => {
            eprintln!("Chart script failed: {}", e);
        }
    }

    println!("\nTotal runtime: {:.1}s\n", t0.elapsed().as_secs_f64());
    Ok(())
}

// ── Chart script builder ────────────────────────────────────────────────────
// Avoids f-string brace issues by building Python code as a plain string
fn build_chart_script(
    win_soft: f64,
    win_hard: f64,
    base_soft: f64,
    base_hard: f64,
    improvement: f64,
) -> String {
    let ws = format!("{:.1}", win_soft);
    let wh = format!("{:.1}", win_hard);
    let bs = format!("{:.1}", base_soft);
    let bh = format!("{:.1}", base_hard);
    let imp = format!("{:.1}", improvement);
    let mut script = r##"#!/usr/bin/env python3
"""
DD-Hard Threshold Sweep Chart Generator
"""
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import pandas as pd
import numpy as np
import os

os.makedirs('charts', exist_ok=True)

WIN_SOFT = {ws_placeholder}
WIN_HARD = {wh_placeholder}
BASE_SOFT = {bs_placeholder}
BASE_HARD = {bh_placeholder}
IMPROVEMENT = {imp_placeholder}

# Load sweep results
df = pd.read_csv('snapshots/ddhard_threshold_sweep.csv')
df.columns = df.columns.str.strip()
df_sorted = df.sort_values(['quarter_passes', 'avg_sharpe'], ascending=[False, False])

print("Top 10 threshold configs:")
print(df_sorted[['soft_pct','hard_pct','quarter_passes','avg_sharpe','avg_ret_pct','worst_dd_pct']].head(10).to_string(index=False))

# Figure 1: Heatmaps
pivot_qp = df.pivot_table(index='soft_pct', columns='hard_pct', values='quarter_passes', aggfunc='first')
pivot_sh = df.pivot_table(index='soft_pct', columns='hard_pct', values='avg_sharpe', aggfunc='first')

fig, axes = plt.subplots(1, 2, figsize=(18, 8))
fig.suptitle(
    'DD-Hard Threshold Optimization'
    '\nWinner: Soft=' + str(WIN_SOFT) + '% Hard=' + str(WIN_HARD)
    + '% | Baseline: Soft=' + str(BASE_SOFT) + '% Hard=' + str(BASE_HARD)
    + '%\nWinner improvement: ' + str(IMPROVEMENT) + '%',
    fontsize=13, fontweight='bold')

ax = axes[0]
im = ax.imshow(pivot_qp.values, cmap='RdYlGn', aspect='auto', origin='lower',
               vmin=df['quarter_passes'].min(), vmax=df['quarter_passes'].max())
xt = [format(v, '.1f') for v in pivot_qp.columns]
ax.set_xticks(range(len(pivot_qp.columns)))
ax.set_xticklabels(xt, fontsize=8, rotation=45)
yt = [format(v, '.1f') for v in pivot_qp.index]
ax.set_yticks(range(len(pivot_qp.index)))
ax.set_yticklabels(yt, fontsize=8)
ax.set_xlabel('Hard Threshold %', fontsize=12)
ax.set_ylabel('Soft Threshold %', fontsize=12)
ax.set_title('Walk-Forward Quarter Passes (Higher = Better)', fontsize=13)
plt.colorbar(im, ax=ax, label='Quarter Passes')
for i, soft in enumerate(pivot_qp.index):
    for j, hard in enumerate(pivot_qp.columns):
        if abs(soft - WIN_SOFT) < 0.1 and abs(hard - WIN_HARD) < 0.1:
            ax.add_patch(plt.Rectangle((j-0.5, i-0.5), 1, 1, fill=False, edgecolor='white', linewidth=3))
        if abs(soft - BASE_SOFT) < 0.1 and abs(hard - BASE_HARD) < 0.1:
            ax.add_patch(plt.Rectangle((j-0.5, i-0.5), 1, 1, fill=False, edgecolor='black', linewidth=2, linestyle='--'))

ax = axes[1]
im = ax.imshow(pivot_sh.values, cmap='RdYlGn', aspect='auto', origin='lower',
               vmin=df['avg_sharpe'].min(), vmax=df['avg_sharpe'].max())
xt = [format(v, '.1f') for v in pivot_sh.columns]
ax.set_xticks(range(len(pivot_sh.columns)))
ax.set_xticklabels(xt, fontsize=8, rotation=45)
yt = [format(v, '.1f') for v in pivot_sh.index]
ax.set_yticks(range(len(pivot_sh.index)))
ax.set_yticklabels(yt, fontsize=8)
ax.set_xlabel('Hard Threshold %', fontsize=12)
ax.set_ylabel('Soft Threshold %', fontsize=12)
ax.set_title('Average Sharpe Ratio (Higher = Better)', fontsize=13)
plt.colorbar(im, ax=ax, label='Avg Sharpe')
for i, soft in enumerate(pivot_sh.index):
    for j, hard in enumerate(pivot_sh.columns):
        if abs(soft - WIN_SOFT) < 0.1 and abs(hard - WIN_HARD) < 0.1:
            ax.add_patch(plt.Rectangle((j-0.5, i-0.5), 1, 1, fill=False, edgecolor='white', linewidth=3))
        if abs(soft - BASE_SOFT) < 0.1 and abs(hard - BASE_HARD) < 0.1:
            ax.add_patch(plt.Rectangle((j-0.5, i-0.5), 1, 1, fill=False, edgecolor='black', linewidth=2, linestyle='--'))

plt.tight_layout()
fig.savefig('charts/ddhard_threshold_sweep.png', dpi=150, bbox_inches='tight')
plt.close(fig)
print('Saved: charts/ddhard_threshold_sweep.png')

# Figure 2: Equity curves
fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10), gridspec_kw={'height_ratios': [3, 1]})
fig.suptitle(
    'DD-Hard Threshold Equity Curves'
    '\nWinner: Soft=' + str(WIN_SOFT) + '% Hard=' + str(WIN_HARD)
    + '% | Baseline: Soft=' + str(BASE_SOFT) + '% Hard=' + str(BASE_HARD),
    fontsize=13, fontweight='bold')

curve_info = [
    ('Baseline (S=' + str(BASE_SOFT) + '% H=' + str(BASE_HARD) + '%)', 'snapshots/eqcurve_ddhard_baseline.csv', '#888888', 1.5, '--'),
    ('Winner (S=' + str(WIN_SOFT) + '% H=' + str(WIN_HARD) + '%)', 'snapshots/eqcurve_ddhard_winner.csv', '#1f77b4', 3.0, '-'),
    ('Runner-up 1', 'snapshots/eqcurve_ddhard_runner1.csv', '#ff7f0e', 1.5, '--'),
    ('Runner-up 2', 'snapshots/eqcurve_ddhard_runner2.csv', '#2ca02c', 1.5, '--'),
]

for label, path, color, lw, ls in curve_info:
    if os.path.exists(path):
        eq = pd.read_csv(path)
        ax1.plot(eq['bar'], eq['equity'], label=label, color=color, linewidth=lw, linestyle=ls)
        peak = np.maximum.accumulate(eq['equity'].values)
        dd = (eq['equity'].values - peak) / peak * 100.0
        ax2.plot(eq['bar'], dd, label=label, color=color, linewidth=lw, linestyle=ls)

all_vals = []
for _, path, _, _, _ in curve_info:
    if os.path.exists(path):
        eq = pd.read_csv(path)
        all_vals.extend(eq['equity'].values.tolist())

if all_vals:
    y_min = min(all_vals)
    y_max = max(all_vals)
    pad = (y_max - y_min) * 0.05
    ax1.set_ylim(max(y_min - pad, 0.01), y_max + pad)
    ax1.set_yscale('log')
    ax1.set_ylabel('Equity (log scale)', fontsize=11)
    ax1.legend(loc='upper left', fontsize=9)
    ax1.grid(True, alpha=0.3, which='both')
    ax2.set_ylabel('Drawdown %', fontsize=11)
    ax2.set_xlabel('Trading Day', fontsize=11)
    ax2.legend(loc='lower left', fontsize=8)
    ax2.grid(True, alpha=0.3)
    ax2.set_ylim(bottom=-100)
    plt.tight_layout()
    fig.savefig('charts/ddhard_threshold_equity_comparison.png', dpi=150, bbox_inches='tight')
    plt.close(fig)
    print('Saved: charts/ddhard_threshold_equity_comparison.png')

print('Charts generated successfully.')
"##.to_string();
    script = script.replace("{ws_placeholder}", &ws);
    script = script.replace("{wh_placeholder}", &wh);
    script = script.replace("{bs_placeholder}", &bs);
    script = script.replace("{bh_placeholder}", &bh);
    script = script.replace("{imp_placeholder}", &imp);
    script
}

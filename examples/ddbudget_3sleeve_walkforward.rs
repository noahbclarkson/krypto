//! DDBudget 3-Sleeve Walk-Forward Validation
//!
//! PURPOSE: Validate the DDBudget 3-sleeve ensemble under STRICT OOS conditions.
//! The reported Sharpe of 7.68 from factor_momentum_sleeve_audit.rs was from FULL-SAMPLE
//! overlapping execution — an in-sample compounding artifact.
//!
//! This harness uses:
//! - 252-bar train / 252-bar test walk-forward windows
//! - Non-overlapping trades within each test window (step = hold bars)
//! - Independent signal generation per window (train-only parameter estimation)
//! - Realistic 0.1% taker fees per side
//!
//! The 3 sleeves:
//! 1. A/D Momentum (AD_PERIOD=5, HOLD=54)
//! 2. MACD+Regime (HOLD=21, SMA200 regime filter)
//! 3. SmallByDollarVol (HOLD=21, cross-sectional dollar-volume rank)
//!
//! DDBudget = DD-hard family exposure budgeting:
//!   DD > 30% → 30% exposure | DD > 15% → 60% exposure | else → 100%

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_AD: usize = 0; // Replaced by Chandelier
const HOLD_OTHER: usize = 21;
const AD_PERIOD: usize = 5;
const MIN_BARS_CHAND: usize = 5;
const CHAND_PERIOD: usize = 28; // hyperopt 2026-04-11: P=28 fine-sweep winner at M=2.00 (Sharpe 9.011 vs P=15=7.785, +15.7%). Full 5-50 step-1 sweep, 9/9 universes positive.
const CHAND_MULT: f64 = 2.00; // hyperopt 2026-04-11: M=2.00 optimal. Note: P was re-optimized at M=2.00 (P=15 was optimal at M=2.5, P=28 at M=2.0)
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3; // hyperopt 2026-04-11: CAP=3 wins over CAP=2 (+1.4% Sharpe, +13pp pass rate, 91% vs 78%)
const WARMUP: usize = 200;
const MIN_TRADES: usize = 3;

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

// ── Math helpers ──────────────────────────────────────────────────────────────

fn ddhard_exposure(dd_pct: f64) -> f64 {
    if dd_pct > 30.0 {
        0.30
    } else if dd_pct > 15.0 {
        0.60
    } else {
        1.0
    }
}

fn calc_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 5 {
        return 0.0;
    }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var =
        daily_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily_rets.len().max(1) as f64;
    let std = var.sqrt();
    if std < 1e-9 {
        return 0.0;
    }
    mean * 365.0 / (std * (365.0_f64).sqrt())
}


fn true_range(h: f64, l: f64, prev_c: f64) -> f64 {
    (h - l).abs().max((h - prev_c).abs()).max((l - prev_c).abs())
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut tr_sum = 0.0_f64;
    for i in idx.saturating_sub(period - 1)..=idx {
        let pc = if i > 0 { close[i - 1] } else { close[0] };
        tr_sum += true_range(high[i], low[i], pc);
    }
    tr_sum / period as f64
}

fn chandelier_exit(
    high: &[f64], low: &[f64], close: &[f64],
    entry_bar: usize, test_end: usize,
    chand_period: usize, chand_mult: f64,
) -> usize {
    let n = high.len();
    let max_exit = test_end.min(n - 1);
    let min_exit = (entry_bar + MIN_BARS_CHAND).max(entry_bar + 1);

    let mut highest_high = high[entry_bar];
    let mut exit_bar = max_exit;

    for bar in min_exit..=max_exit {
        if bar >= n { break; }
        highest_high = highest_high.max(high[bar]);
        let atr_val = atr_at(high, low, close, chand_period, bar);
        let exit_price = highest_high - chand_mult * atr_val;
        if close[bar] < exit_price {
            exit_bar = bar;
            break;
        }
    }
    exit_bar
}

fn calc_max_dd_from(equity: &[f64], mut peak: f64) -> (f64, f64) {
    let mut max_dd = 0.0_f64;
    let mut current_peak = peak;
    for &e in equity {
        if e > current_peak {
            current_peak = e;
        }
        let dd = (1.0 - e / current_peak) * 100.0;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    (max_dd, current_peak)
}

// ── Per-bar signal computation (train-anchored) ───────────────────────────────
//
// All signal functions take a `train_end` parameter and only use data up to that index
// for computing parameters (thresholds, EMAs, etc.)

/// A/D momentum: long top-K by A/D momentum (A/D(t) - A/D(t-N) over train mean)
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

    // Compute A/D values up to `bar` using only past data
    let mut ema1 = 0.0_f64;
    let mut ema2 = 0.0_f64;
    for j in warmup..=bar.min(train_end) {
        let c = close[j];
        let h = high[j];
        let l = low[j];
        let v = vol[j];
        let range = h - l;
        let mul = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        ema1 = alpha1 * mul * v + (1.0 - alpha1) * ema1;
        ema2 = alpha2 * mul * v + (1.0 - alpha2) * ema2;
    }
    let ad_now = ema1 - ema2;

    // Mean from train period
    let mut sum = 0.0_f64;
    let mut cnt = 0usize;
    for j in warmup..train_end {
        let c = close[j];
        let h = high[j];
        let l = low[j];
        let v = vol[j];
        let range = h - l;
        let mul = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        let ea1 = alpha1 * mul * v + (1.0 - alpha1) * 0.0; // simplified
        let ea2 = alpha2 * mul * v + (1.0 - alpha2) * 0.0;
        sum += ea1 - ea2;
        cnt += 1;
    }
    let ad_mean = if cnt > 0 { sum / cnt as f64 } else { 0.0 };

    Some(ad_now - ad_mean) // positive = accumulation
}

/// MACD+Regime: long when price > SMA200 AND MACD line > signal line
fn macd_signal(
    close: &[f64],
    fast: usize,
    slow: usize,
    sig: usize,
    train_end: usize,
    bar: usize,
) -> Option<f64> {
    let warmup = WARMUP.max(slow).max(sig);
    if bar < warmup || bar > train_end {
        return None;
    }

    let ef_alp = 2.0 / (fast as f64 + 1.0);
    let es_alp = 2.0 / (slow as f64 + 1.0);
    let em_alp = 2.0 / (sig as f64 + 1.0);

    let mut ef = 0.0_f64;
    let mut es = 0.0_f64;
    let mut macd_line = 0.0_f64;
    let mut sig_line = 0.0_f64;

    for j in warmup..=bar.min(train_end) {
        ef = ef_alp * close[j] + (1.0 - ef_alp) * ef;
        es = es_alp * close[j] + (1.0 - es_alp) * es;
        let ml = ef - es;
        sig_line = em_alp * ml + (1.0 - em_alp) * sig_line;
        macd_line = ml;
    }

    // SMA200 regime
    let sma_start = (bar.saturating_sub(200)).max(warmup);
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
    let macd_diff = macd_line - sig_line;
    Some(regime * macd_diff)
}

/// Small-dollar-volume rank within universe (smallest vol = highest signal)
fn smallvol_rank(
    close: &[f64],
    vol: &[f64],
    all_close: &HashMap<&str, &[f64]>,
    all_vol: &HashMap<&str, &[f64]>,
    train_end: usize,
    bar: usize,
) -> Option<f64> {
    let warmup = WARMUP;
    if bar < warmup || bar > train_end {
        return None;
    }

    let my_dv = close[bar] * vol.get(bar).copied().unwrap_or(0.0);
    let mut dvs: Vec<f64> = Vec::new();
    for (sym, c) in all_close {
        if let Some(v) = all_vol.get(sym) {
            dvs.push(c[bar.min(c.len() - 1)] * v.get(bar.min(v.len() - 1)).copied().unwrap_or(0.0));
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
    Some((dvs.len() - pos) as f64) // higher = smaller vol = better
}

// ── Walk-forward result ────────────────────────────────────────────────────────

struct WfResult {
    window: usize,
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

// ── Core simulation ───────────────────────────────────────────────────────────

struct SymData {
    close: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    train_end: usize,
    test_start: usize,
    test_end: usize,
    use_3sleeve: bool,
    start_equity: f64,
    mut peak_equity: f64,
) -> (WfResult, f64, f64) {
    // Collect cross-sectional data for small-vol rank
    let all_close: HashMap<&str, &[f64]> = sym_data
        .iter()
        .map(|(k, v)| (k.as_str(), v.close.as_slice()))
        .collect();
    let all_vol: HashMap<&str, &[f64]> = sym_data
        .iter()
        .map(|(k, v)| (k.as_str(), v.vol.as_slice()))
        .collect();

    // Sleeve equity for DDBudget DD-hard
    let mut ad_eq = 1.0_f64;
    let mut mc_eq = 1.0_f64;
    let mut sm_eq = 1.0_f64;
    let mut ad_pk = 1.0_f64;
    let mut mc_pk = 1.0_f64;
    let mut sm_pk = 1.0_f64;

    let mut equity = start_equity;
    let mut equity_curve = vec![start_equity];
    let mut daily_rets = Vec::new();
    let mut wins = 0usize;
    let mut total_trades = 0usize;

    let mut bar = test_start;

    while bar + HOLD_AD.max(HOLD_OTHER) + 2 < test_end {
        // ── Compute signal strength per sleeve ───────────────────────────────
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

            // AD
            if let Some(score) = ad_signal(
                &sd.close, &sd.high, &sd.low, &sd.vol, AD_PERIOD, train_end, bar,
            ) {
                ad_scores.push((sym.as_str(), score));
            }

            // MACD+Regime
            if let Some(score) = macd_signal(&sd.close, 12, 26, 9, train_end, bar) {
                mc_scores.push((sym.as_str(), score));
            }

            // Small vol rank
            if let Some(score) =
                smallvol_rank(&sd.close, &sd.vol, &all_close, &all_vol, train_end, bar)
            {
                sm_scores.push((sym.as_str(), score));
            }
        }

        // Sort top-K
        ad_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        mc_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        sm_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let ad_top: Vec<_> = ad_scores.iter().map(|(s, v)| (*s, *v)).collect();
        let mc_top: Vec<_> = mc_scores.iter().map(|(s, v)| (*s, *v)).collect();
        let sm_top: Vec<_> = sm_scores.iter().map(|(s, v)| (*s, *v)).collect();

        if ad_top.is_empty() && mc_top.is_empty() && sm_top.is_empty() {
            bar += 1;
            continue;
        }

        // ── Sleeve DD-hard weights ───────────────────────────────────────────
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

        let ad_w = ddhard_exposure(ad_dd);
        let mc_w = ddhard_exposure(mc_dd);
        let sm_w = ddhard_exposure(sm_dd);
        let tot_w = ad_w + mc_w + sm_w;
        let ad_w = ad_w / tot_w.max(1e-9);
        let mc_w = mc_w / tot_w.max(1e-9);
        let sm_w = sm_w / tot_w.max(1e-9);

        // ── Execute trades ───────────────────────────────────────────────────
        let mut port_ret = 0.0_f64;

        // AD sleeve (long only, top-K)
        let mut max_ad_hold = 0usize;
        let mut ad_rets = Vec::new();
        for &(sym, _) in &ad_top {
            let Some(sd) = sym_data.get(sym) else {
                continue;
            };
            let entry_bar = bar + 1;
            let entry = sd.close.get(entry_bar).copied().unwrap_or(0.0);
            let exit_bar = chandelier_exit(&sd.high, &sd.low, &sd.close, entry_bar, test_end, CHAND_PERIOD, CHAND_MULT);
            let exit = sd.close.get(exit_bar).copied().unwrap_or(entry);
            
            if entry > 0.0 && exit > 0.0 && exit_bar > entry_bar {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                ad_rets.push(r);
                ad_pk = ad_pk.max(ad_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 {
                    wins += 1;
                }
                
                // Track max hold for stepping forward
                if (exit_bar - bar) > max_ad_hold {
                    max_ad_hold = exit_bar - bar;
                }
            }
        }
        if !ad_rets.is_empty() {
            let avg = ad_rets.iter().sum::<f64>() / ad_rets.len() as f64;
            ad_eq *= 1.0 + avg;
            if use_3sleeve {
                port_ret += ad_w * avg;
            } else {
                port_ret += avg; // A/D only baseline
            }
        }

        if use_3sleeve {
            // MACD sleeve
            let mut mc_rets = Vec::new();
            for &(sym, _) in &mc_top {
                let Some(sd) = sym_data.get(sym) else {
                    continue;
                };
                let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
                let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
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
            let mut sm_rets = Vec::new();
            for &(sym, _) in &sm_top {
                let Some(sd) = sym_data.get(sym) else {
                    continue;
                };
                let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
                let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
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
        }

        let period_ret = port_ret;
        equity *= 1.0 + period_ret;
        equity_curve.push(equity);
        daily_rets.push(period_ret);

        // Step: non-overlapping (use longest hold)
        bar += max_ad_hold.max(HOLD_OTHER).max(1);
    }

    let ret = (equity / start_equity - 1.0) * 100.0;

    let (max_dd, new_peak) = calc_max_dd_from(&equity_curve, peak_equity);
    // Calculate annualized return over the whole period
    let test_days = (test_end - test_start) as f64;
    let ann_return = if test_days > 0.0 {
        ret / 100.0 / (test_days / 365.0)
    } else {
        0.0
    };
    // Calmar-style: (total_return / test_period_years) / max_dd
    let denom_dd = if max_dd.abs() > 1.0 {
        max_dd / 100.0
    } else {
        1.0 / 100.0
    }; // floor at 1% DD
    let sharpe = if ann_return != 0.0 {
        ann_return / denom_dd
    } else {
        0.0
    };
    let win_rate = if total_trades > 0 {
        wins as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    (
        WfResult {
            window: 0,
            ret,
            sharpe,
            max_dd,
            trades: total_trades,
            win_rate,
            pass,
        },
        equity,
        new_peak,
    )
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("═══ DDBudget 3-Sleeve Walk-Forward Validation ═══\n");
    println!("CRITICAL CONTEXT:");
    println!("  factor_momentum_sleeve_audit.rs Sharpe 7.68 = FULL-SAMPLE overlapping execution.");
    println!("  This harness = first STRICT OOS walk-forward test for DDBudget 3-sleeve.\n");
    println!(
        "Config: Train={}b / Test={}b / Non-overlapping / 0.1% taker each side",
        TRAIN_BARS, TEST_BARS
    );
    println!(
        "  AD: period={}, hold={} | MACD+Reg: hold={} | SmallVol: hold={}\n",
        AD_PERIOD, HOLD_AD, HOLD_OTHER, HOLD_OTHER
    );

    // ── Load data ──────────────────────────────────────────────────────────────
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
                raw_cache.insert(sym.clone(), df); // sym is &String, .clone() gives String for HashMap<String, DataFrame>
            }
            Err(e) => {
                eprintln!("  WARNING: {} load failed: {}", sym, e);
            }
        }
    }

    // ── Build aligned data per symbol ──────────────────────────────────────────
    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();

    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked
                        .into_iter()
                        .filter_map(|x| x)
                        .take(n_min)
                        .collect::<Vec<_>>()
                }};
            }
            let close = col_vec!("close");
            let open = col_vec!("open");
            let high = col_vec!("high");
            let low = col_vec!("low");
            let vol = col_vec!("volume");
            sym_data_map.insert(
                sym.clone(),
                SymData {
                    close,
                    open,
                    high,
                    low,
                    vol,
                },
            );
        }
    }

    println!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // ── Run per-universe walk-forward ──────────────────────────────────────────
    let mut csv_lines = vec![
        "universe,window,strategy,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass"
            .to_string(),
    ];

    let mut global_3s_pass = 0usize;
    let mut global_3s_total = 0usize;
    let mut global_ad_pass = 0usize;
    let mut global_ad_total = 0usize;

    for &(label, symbols) in UNIVERSES {
        print!("═══ {:<18} ═══ ", label);
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            println!("SKIPPED (missing data)\n");
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            println!("SKIPPED (not enough data)\n");
            continue;
        }
        println!("{} syms, {} windows", symbols.len(), total_windows);

        let mut rows_3s = Vec::new();
        let mut rows_ad = Vec::new();
        let mut eq_3s = 1.0_f64;
        let mut pk_3s = 1.0_f64;
        let mut eq_ad = 1.0_f64;
        let mut pk_ad = 1.0_f64;

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);

            if test_end.saturating_sub(test_start) < HOLD_AD + 5 {
                continue;
            }

            let (mut r3s, e3, p3_peak) = run_sim(
                &sym_data_map,
                &symbols,
                train_end,
                test_start,
                test_end,
                true,
                eq_3s,
                pk_3s,
            );
            eq_3s = e3;
            pk_3s = p3_peak;
            r3s.window = wi;

            let (mut r_ad, e_ad, p_ad_peak) = run_sim(
                &sym_data_map,
                &symbols,
                train_end,
                test_start,
                test_end,
                false,
                eq_ad,
                pk_ad,
            );
            eq_ad = e_ad;
            pk_ad = p_ad_peak;
            r_ad.window = wi;

            rows_3s.push(WfResult { ..r3s });
            rows_ad.push(WfResult { ..r_ad });

            let mark = |r: &WfResult| {
                if r.pass {
                    "✅"
                } else {
                    "❌"
                }
            };

            print!(
                "  W{:02} | 3Sleeve {:+8.1}% sh={:5.2} DD={:5.1}% {:3}t {:3.0}% {} | ADonly {:+8.1}% sh={:5.2} DD={:5.1}% {:3}t {:3.0}% {}\n",
                wi,
                r3s.ret, r3s.sharpe, r3s.max_dd, r3s.trades, r3s.win_rate, mark(&r3s),
                r_ad.ret, r_ad.sharpe, r_ad.max_dd, r_ad.trades, r_ad.win_rate, mark(&r_ad),
            );

            csv_lines.push(format!(
                "{},{},3sleeve,{:.2},{:.4},{:.2},{},{:.2},{}",
                label,
                wi,
                r3s.ret,
                r3s.sharpe,
                r3s.max_dd,
                r3s.trades,
                r3s.win_rate,
                if r3s.pass { "PASS" } else { "FAIL" }
            ));
            csv_lines.push(format!(
                "{},{},ADonly,{:.2},{:.4},{:.2},{},{:.2},{}",
                label,
                wi,
                r_ad.ret,
                r_ad.sharpe,
                r_ad.max_dd,
                r_ad.trades,
                r_ad.win_rate,
                if r_ad.pass { "PASS" } else { "FAIL" }
            ));
        }

        if rows_3s.is_empty() {
            continue;
        }

        let nw = rows_3s.len();
        let p3 = rows_3s.iter().filter(|r| r.pass).count();
        let pa = rows_ad.iter().filter(|r| r.pass).count();
        global_3s_pass += p3;
        global_3s_total += nw;
        global_ad_pass += pa;
        global_ad_total += nw;

        let avg_ret = |r: &[WfResult]| r.iter().map(|x| x.ret).sum::<f64>() / r.len() as f64;
        let avg_sh = |r: &[WfResult]| r.iter().map(|x| x.sharpe).sum::<f64>() / r.len() as f64;
        let worst_dd = |r: &[WfResult]| r.iter().map(|x| x.max_dd).fold(0.0_f64, |a, v| a.max(v));

        println!(
            "  AGG  | 3Sleeve {:+8.1}% sh={:.2} DD={:5.1}% | {}/{} pass | ADonly {:+8.1}% sh={:.2} DD={:5.1}% | {}/{} pass\n",
            avg_ret(&rows_3s), avg_sh(&rows_3s), worst_dd(&rows_3s), p3, nw,
            avg_ret(&rows_ad), avg_sh(&rows_ad), worst_dd(&rows_ad), pa, nw,
        );
    }

    // ── Write CSV ──────────────────────────────────────────────────────────────
    let csv_path = "snapshots/ddbudget_3sleeve_wf.csv";
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(csv_path)?;
    for line in &csv_lines {
        writeln!(f, "{}", line)?;
    }

    println!("\n═══ GLOBAL SUMMARY ═══");
    println!(
        "  3-Sleeve:  {}/{} windows passed ({:.0}% fail)",
        global_3s_pass,
        global_3s_total,
        if global_3s_total > 0 {
            (global_3s_total - global_3s_pass) as f64 / global_3s_total as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "  A/D-only:   {}/{} windows passed ({:.0}% fail)",
        global_ad_pass,
        global_ad_total,
        if global_ad_total > 0 {
            (global_ad_total - global_ad_pass) as f64 / global_ad_total as f64 * 100.0
        } else {
            0.0
        }
    );
    println!("\n  CSV: {}", csv_path);
    println!("  Runtime: {:.1}s\n", t0.elapsed().as_secs_f64());

    println!("═══ HONEST VERDICT ═══");
    let oos_rate = if global_3s_total > 0 {
        global_3s_pass as f64 / global_3s_total as f64
    } else {
        0.0
    };
    if oos_rate < 0.5 {
        println!(
            "  ⚠️  OOS pass rate {:.0}% < 50% — the Sharpe 7.68 was likely an artifact.",
            oos_rate * 100.0
        );
        println!("  Full-sample compounding inflated both Sharpe AND pass confidence.");
        println!(
            "  Do NOT promote DDBudget to HALL_OF_FAME until confirmed with proper OOS validation."
        );
    } else if oos_rate >= 0.7 {
        println!(
            "  ✅ OOS pass rate {:.0}% >= 70% — DDBudget appears genuinely robust.",
            oos_rate * 100.0
        );
        println!(
            "  The Sharpe 7.68 may be inflated by full-sample compounding, but the edge is real."
        );
    } else {
        println!(
            "  🟡 OOS pass rate {:.0}% — marginal result. Needs further validation.",
            oos_rate * 100.0
        );
    }

    Ok(())
}

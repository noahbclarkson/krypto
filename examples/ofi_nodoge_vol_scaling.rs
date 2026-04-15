//! OFI Momentum on NoDOGE with Vol-Scaling — Kira Research 2026-04-01
//!
//! Follow-up to ofi_momentum_vol_scaling_benchmark.rs
//!
//! Problem identified: DOGE's shorter FDUSD history truncates BTC's equity curve,
//! corrupting portfolio rankings. Fix: exclude DOGE entirely.
//!
//! Universe: BTC, ETH, SOL, XRP, BNB (1d USDT, same date range, no DOGE).
//! Vol-regime sizing overlay:
//!   - 21-bar realized vol %-rank vs prior 252-bar history
//!   - low (< 0.25): 1.5× | high (> 0.75): 0.5×

use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::Path;

const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 10;
const N_POSITIONS: usize = 2;
const VOL_WINDOW: usize = 21;
const VOL_HIST: usize = 252;
const LOOKBACK_BARS: usize = 252;
const TEST_BARS: usize = 252;

// ─── Data ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    ofi_10: Vec<f64>,
    ofi_21: Vec<f64>,
    vol_mult: Vec<f64>,
}

impl SymData {
    fn from_df(df: &DataFrame) -> Option<Self> {
        let n = df.height();
        let close = Self::col_f64(df, "close")?;
        let high = Self::col_f64(df, "high")?;
        let low = Self::col_f64(df, "low")?;

        let mut ofi = vec![0.0; n];
        for i in 0..n {
            let range = high[i] - low[i];
            if range > 1e-9 {
                ofi[i] = (close[i] - (high[i] + low[i]) / 2.0) / range;
            }
        }

        let ofi_10 = Self::rolling_sum(&ofi, 10);
        let ofi_21 = Self::rolling_sum(&ofi, 21);

        let mut returns = vec![0.0; n];
        for i in 1..n {
            if close[i - 1] > 1e-9 {
                returns[i] = (close[i] - close[i - 1]) / close[i - 1];
            }
        }
        let vol_mult = Self::compute_vol_mult(&returns);

        Some(Self {
            close,
            ofi_10,
            ofi_21,
            vol_mult,
        })
    }

    fn col_f64(df: &DataFrame, name: &str) -> Option<Vec<f64>> {
        let ca = df.column(name).ok()?.f64().ok()?;
        Some(ca.into_iter().map(|v| v.unwrap_or(0.0)).collect())
    }

    fn rolling_sum(v: &[f64], w: usize) -> Vec<f64> {
        let n = v.len();
        let mut out = vec![0.0; n];
        for i in w..n {
            out[i] = v[(i - w)..i].iter().sum();
        }
        out
    }

    fn compute_vol_mult(returns: &[f64]) -> Vec<f64> {
        let n = returns.len();
        let mut mult = vec![1.0; n];
        for i in VOL_HIST..n {
            let mut cur_sq = 0.0;
            for j in (i.saturating_sub(VOL_WINDOW))..i {
                cur_sq += returns[j] * returns[j];
            }
            let cur_vol = (cur_sq / VOL_WINDOW as f64).sqrt();

            let mut hist_vols: Vec<f64> = Vec::with_capacity(VOL_HIST);
            for w in (i.saturating_sub(VOL_HIST))..(i.saturating_sub(VOL_WINDOW)) {
                let mut sq = 0.0;
                for j in w..(w + VOL_WINDOW) {
                    if j < returns.len() {
                        sq += returns[j] * returns[j];
                    }
                }
                hist_vols.push((sq / VOL_WINDOW as f64).sqrt());
            }

            if !hist_vols.is_empty() {
                let p25 = Self::percentile(&hist_vols, 0.25);
                let p75 = Self::percentile(&hist_vols, 0.75);
                mult[i] = if cur_vol < p25 {
                    1.5
                } else if cur_vol > p75 {
                    0.5
                } else {
                    1.0
                };
            }
        }
        mult
    }

    fn percentile(v: &[f64], p: f64) -> f64 {
        if v.is_empty() {
            return 0.0;
        }
        let mut s = v.to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = (p * (s.len() - 1) as f64).round() as usize;
        s[idx.min(s.len() - 1)]
    }
}

// ─── Core strategy ─────────────────────────────────────────────────────────────

fn run_strategy(
    data: &HashMap<String, SymData>,
    symbols: &[String],
    start: usize,
    end: usize,
) -> Vec<(f64, f64)> {
    // Returns (bar_return, vol_mult) pairs
    let mut results: Vec<(f64, f64)> = Vec::new();
    let n_active = N_POSITIONS as f64;

    let mut bar = start;
    while bar + HOLD_BARS < end {
        let mut ranked: Vec<(String, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = data.get(sym) {
                if bar < sd.ofi_10.len() && sd.ofi_10[bar].is_finite() {
                    ranked.push((sym.clone(), sd.ofi_10[bar]));
                }
            }
        }

        if ranked.len() >= N_POSITIONS * 2 {
            ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

            let long_syms: Vec<String> = ranked
                .iter()
                .rev()
                .take(N_POSITIONS)
                .map(|(s, _)| s.clone())
                .collect();
            let short_syms: Vec<String> = ranked
                .iter()
                .take(N_POSITIONS)
                .map(|(s, _)| s.clone())
                .collect();

            // Vol mult
            let mut vol_sum = 0.0;
            let mut vol_cnt = 0usize;
            for sym in long_syms.iter().chain(short_syms.iter()) {
                if let Some(sd) = data.get(sym) {
                    if bar < sd.vol_mult.len() {
                        vol_sum += sd.vol_mult[bar];
                        vol_cnt += 1;
                    }
                }
            }
            let avg_vm = if vol_cnt > 0 {
                vol_sum / vol_cnt as f64
            } else {
                1.0
            };

            let mut rets: Vec<f64> = Vec::new();
            for sym in &long_syms {
                if let Some(sd) = data.get(sym) {
                    if bar + 1 < sd.close.len() && bar + HOLD_BARS < sd.close.len() {
                        let gross = sd.close[bar + HOLD_BARS] / sd.close[bar + 1] - 1.0;
                        rets.push(gross - 2.0 * TAKER_FEE);
                    }
                }
            }
            for sym in &short_syms {
                if let Some(sd) = data.get(sym) {
                    if bar + 1 < sd.close.len() && bar + HOLD_BARS < sd.close.len() {
                        let gross = sd.close[bar + HOLD_BARS] / sd.close[bar + 1] - 1.0;
                        rets.push(-(gross + 2.0 * TAKER_FEE));
                    }
                }
            }

            if !rets.is_empty() {
                let avg: f64 = rets.iter().sum::<f64>() / n_active;
                results.push((avg, avg_vm));
            } else {
                results.push((0.0, 1.0));
            }
        } else {
            results.push((0.0, 1.0));
        }

        bar += 1;
    }
    results
}

fn compute_stats(results: &[(f64, f64)], vol_scaled: bool) -> (f64, f64, f64, usize, usize) {
    // Returns (ret%, sharpe, max_dd%, n_trades, n_positive)
    if results.is_empty() {
        return (0.0, 0.0, 0.0, 0, 0);
    }

    let rets: Vec<f64> = if vol_scaled {
        results.iter().map(|(r, vm)| r * vm).collect()
    } else {
        results.iter().map(|(r, _)| *r).collect()
    };

    let mut equity: f64 = 1.0;
    let mut peak: f64 = 1.0;
    let mut max_dd: f64 = 0.0;
    let mut wins = 0usize;
    for &r in &rets {
        equity *= 1.0 + r;
        peak = peak.max(equity);
        let dd = (peak - equity) / peak;
        max_dd = max_dd.max(dd);
        if r > 0.0 {
            wins += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let n = rets.len() as f64;
    let mean: f64 = rets.iter().sum::<f64>() / n;
    let var: f64 = rets
        .iter()
        .map(|&r| {
            let d = r - mean;
            d * d
        })
        .sum::<f64>()
        / n;
    let std = var.sqrt();
    let sharpe = if std > 1e-9 {
        mean / std * 15.8745
    } else {
        0.0
    }; // sqrt(252)

    (ret, sharpe, max_dd * 100.0, rets.len(), wins)
}

fn run_cpcv(
    data: &HashMap<String, SymData>,
    symbols: &[String],
    tstart: usize,
    tend: usize,
) -> (usize, usize) {
    // 6-block chronological CPCV, 15 resamples each for base and vol-scaled
    let block_len = (tend - tstart) / 6;
    let mut base_pass = 0usize;
    let mut vol_pass = 0usize;

    for mask in 1u32..64 {
        let mut br: Vec<(f64, f64)> = Vec::new();
        let mut vr: Vec<(f64, f64)> = Vec::new();

        for bi in 0..6 {
            if mask & (1 << bi) != 0 {
                let bs = tstart + bi * block_len;
                let be = if bi == 5 {
                    tend
                } else {
                    tstart + (bi + 1) * block_len
                };
                br.extend(run_strategy(data, symbols, bs, be));
            }
        }

        if br.len() >= MIN_TRADES {
            let (ret, _, _, _, _) = compute_stats(&br, false);
            if ret > 0.0 {
                base_pass += 1;
            }
        }
        if vr.len() >= MIN_TRADES || !br.is_empty() {
            // For vol-scaled, use the same trades but with vol multipliers
            let (ret, _, _, _, _) = compute_stats(&br, true);
            if ret > 0.0 {
                vol_pass += 1;
            }
        }
    }
    (base_pass, vol_pass)
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    println!("=== OFI Momentum + Vol-Scaling on NoDOGE ===\n");

    let cache_dir = Path::new("data/cache");

    // NoDOGE universe: BTC, ETH, SOL, XRP, BNB (USDT 1d, comparable histories)
    let syms: Vec<String> = vec![
        "BTCUSDT".to_string(),
        "ETHUSDT".to_string(),
        "SOLUSDT".to_string(),
        "XRPUSDT".to_string(),
        "BNBUSDT".to_string(),
    ];

    let mut data: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    for sym in &syms {
        let path = cache_dir.join(format!("{}_1d.parquet", sym.to_lowercase()));
        if !path.exists() {
            eprintln!("[WARN] {} not found", path.display());
            continue;
        }
        match DataLoader::load_parquet(&path) {
            Ok(df) => {
                if let Some(sd) = SymData::from_df(&df) {
                    min_len = min_len.min(sd.close.len());
                    data.insert(sym.clone(), sd);
                    println!("  {}: {} bars", sym, data.get(sym).unwrap().close.len());
                }
            }
            Err(e) => eprintln!("[WARN] {} load error: {}", sym, e),
        }
    }

    let n = min_len;
    println!("\nCommon history: {} bars\n", n);

    if data.len() < syms.len() {
        eprintln!(
            "[ERROR] Only {}/{} loaded — need all symbols",
            data.len(),
            syms.len()
        );
        return;
    }

    // ── Walk-forward ──────────────────────────────────────────────────────────
    let total_windows = n.saturating_sub(LOOKBACK_BARS) / TEST_BARS;
    println!("{} walk-forward windows\n", total_windows);

    println!(
        "{:<6} {:>7} | {:>9} {:>6} {:>6} {:>6} | {:>9} {:>6} {:>6} {:>6} | {:>5} {:>5}",
        "W",
        "tstart",
        "BASE ret%",
        "Sh",
        "DD%",
        "Trd",
        "VOL ret%",
        "Sh",
        "DD%",
        "Trd",
        "QP",
        "CPCV"
    );
    println!("{}", "-".repeat(100));

    let mut total_base_pass = 0usize;
    let mut total_vol_pass = 0usize;
    let mut base_total_ret = 0.0f64;
    let mut vol_total_ret = 0.0f64;

    for wi in 0..total_windows {
        let tstart = LOOKBACK_BARS + wi * TEST_BARS;
        let tend = (tstart + TEST_BARS).min(n);
        if tend - tstart < HOLD_BARS + 5 {
            break;
        }

        let results = run_strategy(&data, &syms, tstart, tend);

        // Quarter-level pass
        let q_len = (tend - tstart) / 4;
        let mut base_q_pass = 0usize;
        let mut vol_q_pass = 0usize;
        for qi in 0..4 {
            let qs = tstart + qi * q_len;
            let qe = if qi == 3 {
                tend
            } else {
                tstart + (qi + 1) * q_len
            };
            let q_idx_start = qs.saturating_sub(tstart);
            let q_idx_end = (qe - tstart).min(results.len());

            let q_base: Vec<(f64, f64)> = results[q_idx_start..q_idx_end].to_vec();
            if q_base.len() >= 3 {
                let (ret, _, _, _, _) = compute_stats(&q_base, false);
                if ret > 0.0 {
                    base_q_pass += 1;
                }
            }
            if q_base.len() >= 3 {
                let (ret, _, _, _, _) = compute_stats(&q_base, true);
                if ret > 0.0 {
                    vol_q_pass += 1;
                }
            }
        }

        let (b_ret, b_sh, b_dd, b_trd, _) = compute_stats(&results, false);
        let (v_ret, v_sh, v_dd, v_trd, _) = compute_stats(&results, true);

        let base_pass = if b_ret > 0.0 && b_trd >= MIN_TRADES {
            1
        } else {
            0
        };
        let vol_pass = if v_ret > 0.0 && v_trd >= MIN_TRADES {
            1
        } else {
            0
        };
        total_base_pass += base_pass;
        total_vol_pass += vol_pass;
        base_total_ret += b_ret;
        vol_total_ret += v_ret;

        println!("{:<6} {:>7} | {:>+9.1} {:>6.2} {:>6.1} {:>6} | {:>+9.1} {:>6.2} {:>6.1} {:>6} | {:>5} {:>5}",
            format!("W{:02}", wi),
            format!("b{}", tstart),
            b_ret, b_sh, -b_dd, b_trd,
            v_ret, v_sh, -v_dd, v_trd,
            format!("{}/4", vol_q_pass),
            format!("{}/4", base_q_pass),
        );
    }

    let nw = total_windows;
    if nw == 0 {
        return;
    }

    println!("\n=== SUMMARY ===");
    let b_avg_ret = base_total_ret / nw as f64;
    let v_avg_ret = vol_total_ret / nw as f64;

    let base_avg_sh = {
        let mut total = 0.0f64;
        let mut count = 0usize;
        for wi in 0..nw {
            let tstart = LOOKBACK_BARS + wi * TEST_BARS;
            let tend = (tstart + TEST_BARS).min(n);
            if tend - tstart < HOLD_BARS + 5 {
                break;
            }
            let r = run_strategy(&data, &syms, tstart, tend);
            let (_, sh, _, _, _) = compute_stats(&r, false);
            total += sh;
            count += 1;
        }
        if count > 0 {
            total / count as f64
        } else {
            0.0
        }
    };
    let vol_avg_sh = {
        let mut total = 0.0f64;
        let mut count = 0usize;
        for wi in 0..nw {
            let tstart = LOOKBACK_BARS + wi * TEST_BARS;
            let tend = (tstart + TEST_BARS).min(n);
            if tend - tstart < HOLD_BARS + 5 {
                break;
            }
            let r = run_strategy(&data, &syms, tstart, tend);
            let (_, sh, _, _, _) = compute_stats(&r, true);
            total += sh;
            count += 1;
        }
        if count > 0 {
            total / count as f64
        } else {
            0.0
        }
    };

    println!(
        "BASE: {}/{} pass | avg ret {:+.1}% | avg sharpe {:.2}",
        total_base_pass, nw, b_avg_ret, base_avg_sh
    );
    println!(
        "VOL:   {}/{} pass | avg ret {:+.1}% | avg sharpe {:.2}",
        total_vol_pass, nw, v_avg_ret, vol_avg_sh
    );

    // ── OFI 21-bar variant (quick test on last window) ───────────────────────
    println!("\n=== OFI 21-bar variant (last window) ===");
    let last_wi = nw - 1;
    let tstart = LOOKBACK_BARS + last_wi * TEST_BARS;
    let tend = (tstart + TEST_BARS).min(n);

    let mut results21: Vec<(f64, f64)> = Vec::new();
    let mut bar = tstart;
    while bar + HOLD_BARS < tend {
        let mut ranked: Vec<(String, f64)> = Vec::new();
        for sym in &syms {
            if let Some(sd) = data.get(sym) {
                if bar < sd.ofi_21.len() && sd.ofi_21[bar].is_finite() {
                    ranked.push((sym.clone(), sd.ofi_21[bar]));
                }
            }
        }
        if ranked.len() >= N_POSITIONS * 2 {
            ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            let long_syms: Vec<String> = ranked
                .iter()
                .rev()
                .take(N_POSITIONS)
                .map(|(s, _)| s.clone())
                .collect();
            let short_syms: Vec<String> = ranked
                .iter()
                .take(N_POSITIONS)
                .map(|(s, _)| s.clone())
                .collect();
            let mut vol_sum = 0.0f64;
            let mut vol_cnt = 0usize;
            for sym in long_syms.iter().chain(short_syms.iter()) {
                if let Some(sd) = data.get(sym) {
                    if bar < sd.vol_mult.len() {
                        vol_sum += sd.vol_mult[bar];
                        vol_cnt += 1;
                    }
                }
            }
            let vm = if vol_cnt > 0 {
                vol_sum / vol_cnt as f64
            } else {
                1.0
            };

            let mut rets: Vec<f64> = Vec::new();
            for sym in &long_syms {
                if let Some(sd) = data.get(sym) {
                    if bar + 1 < sd.close.len() && bar + HOLD_BARS < sd.close.len() {
                        let g = sd.close[bar + HOLD_BARS] / sd.close[bar + 1] - 1.0;
                        rets.push(g - 2.0 * TAKER_FEE);
                    }
                }
            }
            for sym in &short_syms {
                if let Some(sd) = data.get(sym) {
                    if bar + 1 < sd.close.len() && bar + HOLD_BARS < sd.close.len() {
                        let g = sd.close[bar + HOLD_BARS] / sd.close[bar + 1] - 1.0;
                        rets.push(-(g + 2.0 * TAKER_FEE));
                    }
                }
            }
            if !rets.is_empty() {
                let avg: f64 = rets.iter().sum::<f64>() / (N_POSITIONS * 2) as f64;
                results21.push((avg, vm));
            } else {
                results21.push((0.0, 1.0));
            }
        } else {
            results21.push((0.0, 1.0));
        }
        bar += 1;
    }
    let (b21, s21, d21, t21, _) = compute_stats(&results21, false);
    let (v21, sv21, dv21, tv21, _) = compute_stats(&results21, true);
    println!(
        "OFI-21 BASE: {:+.1}%  Sharpe={:.2}  DD={:.1}%  {} trades",
        b21, s21, -d21, t21
    );
    println!(
        "OFI-21 VOL:  {:+.1}%  Sharpe={:.2}  DD={:.1}%  {} trades",
        v21, sv21, -dv21, tv21
    );
}

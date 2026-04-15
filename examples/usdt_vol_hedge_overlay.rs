//! USDT Vol-Hedge Overlay vs Vol-Scaling vs Baseline — Kira Research 2026-04-01
//!
//! Question: does a USDT cash buffer (30% in USDT when vol > 75th pct) reduce
//! MaxDD more than vol-scaling (position 0.5x when high-vol) for Turtle+MACD?
//!
//! Key difference from vol-scaling:
//! - Vol scaling: multiplier within crypto (0.5x in high-vol → less exposure, still in crypto)
//! - USDT overlay: 30% in USDT (safe, earns 0%), 70% in crypto during high-vol periods
//! - USDT is a HARD PROTECTION — 30% of capital cannot lose during crash
//!
//! Design: same 7-window walk-forward (252 train / 252 test / 21-bar hold)
//! Turtle+MACD signal, BTC SMA200 regime filter
//! Top-3 ranked long/short book, 0.1% taker each side
//! Three modes: BASE (1.0x), VOL_SCALE (0.5x/1.5x), USDT_OVERLAY (0.7 crypto + USDT buffer)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;
const VOL_WINDOW: usize = 21;
const VOL_HIST: usize = 252;
const VOL_PCT_THRESHOLD: f64 = 0.75; // top quartile = high vol

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ USDT Vol-Hedge Overlay vs Vol-Scaling ═══\n");

    let loader = DataLoader::new(None, None);
    let syms = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in &syms {
        let raw = loader.fetch_with_cache(s, "1d", CANDLES).await?;
        let df = krypto::features::indicators::FeatureEngine::add_technicals(&raw, None)?;
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

    println!("{:<6} {:>7} | {:>9} {:>6} {:>7} {:>5} | {:>9} {:>6} {:>7} {:>5} | {:>9} {:>6} {:>7} {:>5} | {:>6}",
        "W", "tstart",
        "BASE ret%", "Sh", "DD%", "Trd",
        "VOL ret%", "Sh", "DD%", "Trd",
        "USDT ret%", "Sh", "DD%", "Trd",
        "Best");
    println!("{}", "-".repeat(125));

    let mut base_pass = 0usize;
    let mut vol_pass = 0usize;
    let mut usdt_pass = 0usize;
    let mut vol_better_dd = 0usize;
    let mut usdt_better_dd = 0usize;
    let mut base_total_dd = 0.0f64;
    let mut vol_total_dd = 0.0f64;
    let mut usdt_total_dd = 0.0f64;
    let mut worst_base_dd = 0.0f64;
    let mut worst_vol_dd = 0.0f64;
    let mut worst_usdt_dd = 0.0f64;
    let mut base_total_ret = 0.0f64;
    let mut vol_total_ret = 0.0f64;
    let mut usdt_total_ret = 0.0f64;

    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        if tend - tstart < HOLD_BARS + 2 {
            continue;
        }

        let t0 = Instant::now();

        // ── Build per-symbol data ─────────────────────────────────────────────
        let mut close: HashMap<String, Vec<f64>> = HashMap::new();
        let mut macd_line: HashMap<String, Vec<f64>> = HashMap::new();
        let mut macd_signal: HashMap<String, Vec<f64>> = HashMap::new();
        let mut dc_hi: HashMap<String, Vec<f64>> = HashMap::new();
        let mut dc_lo: HashMap<String, Vec<f64>> = HashMap::new();
        // Vol percentile (for USDT overlay) and vol multiplier (for vol-scaling)
        let mut vol_pct: HashMap<String, Vec<f64>> = HashMap::new();
        let mut vol_mult: HashMap<String, Vec<f64>> = HashMap::new();

        for sym in &syms {
            let df = cache.get(*sym).unwrap();
            let c = col_f64(df, "close")?;
            close.insert(sym.to_string(), c.clone());
            macd_line.insert(sym.to_string(), col_f64(df, "macd")?);
            macd_signal.insert(sym.to_string(), col_f64(df, "macd_signal")?);

            let h = col_f64(df, "high")?;
            let l = col_f64(df, "low")?;
            dc_hi.insert(sym.to_string(), rolling_max(&h, 20));
            dc_lo.insert(sym.to_string(), rolling_min(&l, 20));

            // Returns series for vol calculations
            let mut rets = vec![0.0; c.len()];
            for i in 1..c.len() {
                if c[i - 1] > 0.0 {
                    rets[i] = (c[i] - c[i - 1]) / c[i - 1];
                }
            }
            vol_mult.insert(sym.to_string(), compute_vol_mult(&rets));
            vol_pct.insert(sym.to_string(), compute_vol_pct(&rets));
        }

        let (b_ret, b_sh, b_dd, b_trd) = run_backtest(
            &close,
            &macd_line,
            &macd_signal,
            &dc_hi,
            &dc_lo,
            &vol_mult,
            &vol_pct,
            &syms,
            tstart,
            tend,
            Mode::Base,
        );
        let (v_ret, v_sh, v_dd, v_trd) = run_backtest(
            &close,
            &macd_line,
            &macd_signal,
            &dc_hi,
            &dc_lo,
            &vol_mult,
            &vol_pct,
            &syms,
            tstart,
            tend,
            Mode::VolScale,
        );
        let (u_ret, u_sh, u_dd, u_trd) = run_backtest(
            &close,
            &macd_line,
            &macd_signal,
            &dc_hi,
            &dc_lo,
            &vol_mult,
            &vol_pct,
            &syms,
            tstart,
            tend,
            Mode::UsdtHedge,
        );

        if b_trd >= MIN_TRADES && b_ret > 0.0 {
            base_pass += 1;
        }
        if v_trd >= MIN_TRADES && v_ret > 0.0 {
            vol_pass += 1;
        }
        if u_trd >= MIN_TRADES && u_ret > 0.0 {
            usdt_pass += 1;
        }
        if v_dd < b_dd {
            vol_better_dd += 1;
        }
        if u_dd < b_dd {
            usdt_better_dd += 1;
        }
        base_total_ret += b_ret;
        base_total_dd += b_dd;
        vol_total_ret += v_ret;
        vol_total_dd += v_dd;
        usdt_total_ret += u_ret;
        usdt_total_dd += u_dd;
        worst_base_dd = worst_base_dd.min(b_dd);
        worst_vol_dd = worst_vol_dd.min(v_dd);
        worst_usdt_dd = worst_usdt_dd.min(u_dd);

        let best = if u_dd < v_dd && u_dd < b_dd {
            "USDT"
        } else if v_dd < b_dd {
            "VOL"
        } else {
            "BASE"
        };

        println!("{:<6} {:>7} | {:>+9.1} {:>6.2} {:>7.1} {:>5} | {:>+9.1} {:>6.2} {:>7.1} {:>5} | {:>+9.1} {:>6.2} {:>7.1} {:>5} | {:>6} ({:.1}s)",
            format!("W{:02}", wi), format!("b{}", tstart),
            b_ret, b_sh, -b_dd, b_trd,
            v_ret, v_sh, -v_dd, v_trd,
            u_ret, u_sh, -u_dd, u_trd,
            best, t0.elapsed().as_secs_f64());

        drop(close);
        drop(macd_line);
        drop(macd_signal);
        drop(dc_hi);
        drop(dc_lo);
        drop(vol_mult);
        drop(vol_pct);
    }

    let nw = total_windows;
    if nw == 0 {
        return Ok(());
    }

    println!("\n═══════════════════════════════════════════════════════════");
    println!("SUMMARY: {} windows", nw);
    println!(
        "BASE:   {}/{} pass | avg {:+.1}% | avg DD {:.1}% | worst DD {:.1}%",
        base_pass,
        nw,
        base_total_ret / nw as f64,
        -base_total_dd / nw as f64,
        -worst_base_dd
    );
    println!(
        "VOL:    {}/{} pass | avg {:+.1}% | avg DD {:.1}% | worst DD {:.1}%",
        vol_pass,
        nw,
        vol_total_ret / nw as f64,
        -vol_total_dd / nw as f64,
        -worst_vol_dd
    );
    println!(
        "USDT:   {}/{} pass | avg {:+.1}% | avg DD {:.1}% | worst DD {:.1}%",
        usdt_pass,
        nw,
        usdt_total_ret / nw as f64,
        -usdt_total_dd / nw as f64,
        -worst_usdt_dd
    );

    println!("\n─── DD improvement vs BASE ───");
    println!("VOL reduces DD in {}/{} windows", vol_better_dd, nw);
    println!("USDT reduces DD in {}/{} windows", usdt_better_dd, nw);
    println!(
        "Worst-case DD: BASE {:.1}% | VOL {:.1}% | USDT {:.1}%",
        -worst_base_dd, -worst_vol_dd, -worst_usdt_dd
    );

    // Pass rate comparison
    let base_rate = base_pass as f64 / nw as f64;
    let vol_rate = vol_pass as f64 / nw as f64;
    let usdt_rate = usdt_pass as f64 / nw as f64;
    println!("\n─── Pass rate ───");
    println!(
        "BASE: {:.0}% | VOL: {:.0}% | USDT: {:.0}%",
        base_rate * 100.0,
        vol_rate * 100.0,
        usdt_rate * 100.0
    );

    // Honest assessment
    let usdt_wins_dd = usdt_total_dd < vol_total_dd && usdt_total_dd < base_total_dd;
    let usdt_acceptable = usdt_pass >= base_pass.saturating_sub(1); // allow 1 fewer pass
    println!("\n─── Honest assessment ───");
    if usdt_wins_dd && usdt_acceptable {
        println!("✅ USDT overlay reduces MaxDD without meaningfully hurting pass rate");
    } else if usdt_wins_dd {
        println!(
            "⚠️ USDT reduces DD but hurts pass rate ({}/{} → {}/{})",
            base_pass, nw, usdt_pass, nw
        );
    } else {
        println!("❌ USDT overlay does NOT improve drawdown meaningfully");
    }

    println!("\n─── Verdict ───");
    if usdt_wins_dd && usdt_acceptable {
        println!("USDT overlay is VIABLE for MaxDD control — candidate for HOF");
    } else if usdt_total_dd < base_total_dd {
        println!("USDT helps DD but not enough — borderline, needs risk sizing review");
    } else {
        println!("Neither vol-scaling nor USDT overlay solves the MaxDD problem structurally");
    }

    Ok(())
}

// ─── Modes ────────────────────────────────────────────────────────────────────

enum Mode {
    Base,
    VolScale,
    UsdtHedge,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn col_f64(df: &DataFrame, name: &str) -> Result<Vec<f64>> {
    let ca = df.column(name)?.f64()?;
    Ok(ca.into_iter().map(|v| v.unwrap_or(0.0)).collect())
}

fn compute_vol_mult(returns: &[f64]) -> Vec<f64> {
    // Returns multiplier: 0.5 (high vol) / 1.0 (mid) / 1.5 (low vol)
    let n = returns.len();
    let mut mult = vec![1.0; n];
    for i in VOL_HIST..n {
        let mut cur_sq = 0.0;
        for j in (i.saturating_sub(VOL_WINDOW))..i {
            cur_sq += returns[j] * returns[j];
        }
        let cur_vol = (cur_sq / VOL_WINDOW as f64).sqrt();

        let mut hist: Vec<f64> = Vec::with_capacity(VOL_HIST);
        for w in (i.saturating_sub(VOL_HIST))..(i.saturating_sub(VOL_WINDOW)) {
            let mut sq = 0.0;
            for j in w..(w + VOL_WINDOW) {
                if j < returns.len() {
                    sq += returns[j] * returns[j];
                }
            }
            hist.push((sq / VOL_WINDOW as f64).sqrt());
        }

        if !hist.is_empty() {
            let p25 = percentile(&hist, 0.25);
            let p75 = percentile(&hist, 0.75);
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

fn compute_vol_pct(returns: &[f64]) -> Vec<f64> {
    // Returns percentile rank of current vol vs 252-bar history (0.0 = lowest, 1.0 = highest)
    let n = returns.len();
    let mut pct = vec![0.5; n]; // default mid
    for i in VOL_HIST..n {
        let mut cur_sq = 0.0;
        for j in (i.saturating_sub(VOL_WINDOW))..i {
            cur_sq += returns[j] * returns[j];
        }
        let cur_vol = (cur_sq / VOL_WINDOW as f64).sqrt();

        let mut hist: Vec<f64> = Vec::with_capacity(VOL_HIST);
        for w in (i.saturating_sub(VOL_HIST))..(i.saturating_sub(VOL_WINDOW)) {
            let mut sq = 0.0;
            for j in w..(w + VOL_WINDOW) {
                if j < returns.len() {
                    sq += returns[j] * returns[j];
                }
            }
            hist.push((sq / VOL_WINDOW as f64).sqrt());
        }

        if !hist.is_empty() {
            let count_above = hist.iter().filter(|&&v| v <= cur_vol).count();
            pct[i] = count_above as f64 / hist.len() as f64;
        }
    }
    pct
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

fn rolling_max(high: &[f64], window: usize) -> Vec<f64> {
    let n = high.len();
    let mut out = vec![0.0; n];
    for i in window..n {
        out[i] = high[(i - window)..i]
            .iter()
            .fold(high[i - window], |a, &b| a.max(b));
    }
    out
}

fn rolling_min(low: &[f64], window: usize) -> Vec<f64> {
    let n = low.len();
    let mut out = vec![0.0; n];
    for i in window..n {
        out[i] = low[(i - window)..i]
            .iter()
            .fold(low[i - window], |a, &b| a.min(b));
    }
    out
}

fn sma(close: &[f64], window: usize) -> f64 {
    if close.len() < window {
        return close.iter().sum::<f64>() / close.len().max(1) as f64;
    }
    close[close.len() - window..].iter().sum::<f64>() / window as f64
}

// ─── Backtest ─────────────────────────────────────────────────────────────────

fn run_backtest(
    close: &HashMap<String, Vec<f64>>,
    macd_line: &HashMap<String, Vec<f64>>,
    macd_signal: &HashMap<String, Vec<f64>>,
    dc_hi: &HashMap<String, Vec<f64>>,
    dc_lo: &HashMap<String, Vec<f64>>,
    vol_mult: &HashMap<String, Vec<f64>>,
    vol_pct: &HashMap<String, Vec<f64>>,
    syms: &[&str],
    tstart: usize,
    tend: usize,
    mode: Mode,
) -> (f64, f64, f64, usize) {
    let mut rets: Vec<f64> = Vec::new();

    let mut bar = tstart;
    while bar + HOLD_BARS < tend {
        let btc_close = match close.get("BTCUSDT") {
            Some(c) if c.len() > bar => &c[..],
            _ => {
                bar += 1;
                continue;
            }
        };

        // BTC SMA200 regime
        let btc_sma200 = sma(btc_close, 200);
        let btc_above_sma = btc_close[bar] > btc_sma200;

        #[derive(Clone)]
        struct Entry {
            sym: String,
            direction: i8,
        }
        let mut longs: Vec<Entry> = Vec::new();
        let mut shorts: Vec<Entry> = Vec::new();

        for sym in syms {
            let cl = match close.get(*sym) {
                Some(c) if c.len() > bar + HOLD_BARS => c,
                _ => continue,
            };
            let price = cl[bar];
            let ml = *macd_line.get(*sym).and_then(|v| v.get(bar)).unwrap_or(&0.0);
            let ms = *macd_signal
                .get(*sym)
                .and_then(|v| v.get(bar))
                .unwrap_or(&0.0);
            let dhi = *dc_hi.get(*sym).and_then(|v| v.get(bar)).unwrap_or(&0.0);
            let dlo = *dc_lo.get(*sym).and_then(|v| v.get(bar)).unwrap_or(&0.0);

            if price > dhi && ml > ms && btc_above_sma {
                longs.push(Entry {
                    sym: (*sym).to_string(),
                    direction: 1,
                });
            } else if price < dlo && ml < ms && !btc_above_sma {
                shorts.push(Entry {
                    sym: (*sym).to_string(),
                    direction: -1,
                });
            }
        }

        // Strength rank
        let mut long_entries: Vec<(String, i8)> =
            longs.into_iter().map(|e| (e.sym, e.direction)).collect();
        let mut short_entries: Vec<(String, i8)> =
            shorts.into_iter().map(|e| (e.sym, e.direction)).collect();

        long_entries.sort_by(|a, b| {
            let sa = close
                .get(&a.0)
                .and_then(|c| c.get(bar))
                .copied()
                .unwrap_or(0.0)
                - close
                    .get(&a.0)
                    .and_then(|c| c.get(bar.saturating_sub(1)))
                    .copied()
                    .unwrap_or(0.0);
            let sb = close
                .get(&b.0)
                .and_then(|c| c.get(bar))
                .copied()
                .unwrap_or(0.0)
                - close
                    .get(&b.0)
                    .and_then(|c| c.get(bar.saturating_sub(1)))
                    .copied()
                    .unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap()
        });
        short_entries.sort_by(|a, b| {
            let sa = close
                .get(&a.0)
                .and_then(|c| c.get(bar))
                .copied()
                .unwrap_or(0.0)
                - close
                    .get(&a.0)
                    .and_then(|c| c.get(bar.saturating_sub(1)))
                    .copied()
                    .unwrap_or(0.0);
            let sb = close
                .get(&b.0)
                .and_then(|c| c.get(bar))
                .copied()
                .unwrap_or(0.0)
                - close
                    .get(&b.0)
                    .and_then(|c| c.get(bar.saturating_sub(1)))
                    .copied()
                    .unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap()
        });

        let top_longs: Vec<_> = long_entries.into_iter().take(3).collect();
        let top_shorts: Vec<_> = short_entries.into_iter().take(3).collect();

        if !top_longs.is_empty() || !top_shorts.is_empty() {
            for (sym, dir) in top_longs.into_iter().chain(top_shorts.into_iter()) {
                if let (Some(cl), Some(_ml), Some(_ms)) =
                    (close.get(&sym), macd_line.get(&sym), macd_signal.get(&sym))
                {
                    if bar + 1 < cl.len() && bar + HOLD_BARS < cl.len() {
                        let entry = cl[bar + 1];
                        let exit = cl[bar + HOLD_BARS];
                        let gross = exit / entry - 1.0;
                        let net = if dir > 0 {
                            gross - 2.0 * TAKER_FEE
                        } else {
                            -(gross + 2.0 * TAKER_FEE)
                        };

                        let pos_mult = match mode {
                            Mode::Base => 1.0,
                            Mode::VolScale => {
                                let vm =
                                    *vol_mult.get(&sym).and_then(|v| v.get(bar)).unwrap_or(&1.0);
                                vm
                            }
                            Mode::UsdtHedge => {
                                let vp =
                                    *vol_pct.get(&sym).and_then(|v| v.get(bar)).unwrap_or(&0.5);
                                if vp >= VOL_PCT_THRESHOLD {
                                    0.7
                                } else {
                                    1.0
                                }
                            }
                        };

                        // Apply position multiplier
                        let sized_net = net * pos_mult;

                        // For USDT overlay in high-vol: 70% crypto + 30% USDT (0% return)
                        // Portfolio return = 0.7 * crypto_return + 0.3 * 0
                        // So we use 0.7 * net and note the USDT portion is captured by not compounding the full amount
                        // Actually: the crypto portion gets 0.7 allocation, USDT gets 0.3
                        // Net effect on $1: 0.7 * (1 + net) - 0.3 loss from USDT not being allocated
                        // = 0.7 + 0.7*net - 0.3 = 0.4 + 0.7*net
                        // vs full allocation = 1 + net
                        // Equivalent multiplier = 0.7*net / net (when net != 0) PLUS the USDT floor
                        // This simplifies to: 0.7 for the allocated portion
                        // BUT we need to think about this per-dollar:
                        // If we put $0.70 in crypto and $0.30 in USDT:
                        // Final = 0.70 * (1 + net) + 0.30 * 1.0 = 0.70 + 0.70*net + 0.30 = 1.0 + 0.70*net
                        // vs without hedge = 1.0 + net
                        // Hedge return = 0.70 * net
                        // Which means: the net return IS 0.70 * net when high-vol
                        let final_ret = match mode {
                            Mode::UsdtHedge => {
                                let vp =
                                    *vol_pct.get(&sym).and_then(|v| v.get(bar)).unwrap_or(&0.5);
                                if vp >= VOL_PCT_THRESHOLD {
                                    sized_net * 0.70 / pos_mult.max(0.01)
                                } else {
                                    sized_net
                                }
                            }
                            _ => sized_net,
                        };

                        rets.push(final_ret);
                    }
                }
            }
        }

        bar += 1;
    }

    if rets.is_empty() {
        return (0.0, 0.0, 0.0, 0);
    }

    // Compound equity
    let mut equity: f64 = 1.0;
    let mut peak: f64 = 1.0;
    let mut max_dd: f64 = 0.0;
    for &r in &rets {
        equity *= 1.0 + r;
        peak = peak.max(equity);
        let dd = (peak - equity) / peak;
        max_dd = max_dd.max(dd);
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
    };

    (ret, sharpe, max_dd * 100.0, rets.len())
}

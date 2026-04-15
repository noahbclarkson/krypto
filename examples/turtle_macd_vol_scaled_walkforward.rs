//! Turtle+MACD + Vol-Scaling Walk-Forward — Kira Research 2026-04-01
//!
//! Previous vol-regime benchmark showed vol-scaling improves avg Q-return 3/4 universes
//! for Turtle+MACD. This fills the gap: per-window MaxDD and per-window pass/fail.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
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
const TURTLE_ENTRY: usize = 20;

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ Turtle+MACD + Vol-Scaling Walk-Forward ═══\n");

    let loader = DataLoader::new(None, None);
    let syms = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in &syms {
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

    let total_windows = n.saturating_sub(TRAIN_BARS) / TEST_BARS;
    println!(
        "{} syms, {} bars, {} windows\n",
        syms.len(),
        n,
        total_windows
    );

    println!(
        "{:<6} {:>7} | {:>9} {:>6} {:>7} {:>5} | {:>9} {:>6} {:>7} {:>5} | {:>6}",
        "W", "tstart", "BASE ret%", "Sh", "DD%", "Trd", "VOL ret%", "Sh", "DD%", "Trd", "Result"
    );
    println!("{}", "-".repeat(100));

    let mut base_pass_cnt = 0usize;
    let mut vol_pass_cnt = 0usize;
    let mut vol_better_ret = 0usize;
    let mut vol_better_dd = 0usize;
    let mut base_total_ret = 0.0f64;
    let mut vol_total_ret = 0.0f64;
    let mut base_total_sh = 0.0f64;
    let mut vol_total_sh = 0.0f64;
    let mut base_total_dd = 0.0f64;
    let mut vol_total_dd = 0.0f64;
    let mut worst_base_dd = 0.0f64;
    let mut worst_vol_dd = 0.0f64;

    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        if tend - tstart < HOLD_BARS + 2 {
            continue;
        }

        let t0 = Instant::now();

        // Build per-symbol data
        let mut close: HashMap<String, Vec<f64>> = HashMap::new();
        let mut macd_line: HashMap<String, Vec<f64>> = HashMap::new();
        let mut macd_signal: HashMap<String, Vec<f64>> = HashMap::new();
        let mut dc_hi: HashMap<String, Vec<f64>> = HashMap::new();
        let mut dc_lo: HashMap<String, Vec<f64>> = HashMap::new();
        let mut vol_mult: HashMap<String, Vec<f64>> = HashMap::new();

        for sym in &syms {
            let df = cache.get(*sym).unwrap();
            let c = col_f64(df, "close")?;
            let h = col_f64(df, "high")?;
            let l = col_f64(df, "low")?;
            close.insert(sym.to_string(), c.clone());
            macd_line.insert(sym.to_string(), col_f64(df, "macd")?);
            macd_signal.insert(sym.to_string(), col_f64(df, "macd_signal")?);
            // Donchian 20-bar channels computed manually
            dc_hi.insert(sym.to_string(), rolling_max(&h, 20));
            dc_lo.insert(sym.to_string(), rolling_min(&l, 20));

            // Vol mult for this symbol
            let cl = close.get(*sym).unwrap();
            let mut rets = vec![0.0; cl.len()];
            for i in 1..cl.len() {
                if cl[i - 1] > 0.0 {
                    rets[i] = (cl[i] - cl[i - 1]) / cl[i - 1];
                }
            }
            vol_mult.insert(sym.to_string(), compute_vol_mult(&rets));
        }

        let (b_ret, b_sh, b_dd, b_trd) = run_backtest(
            &close,
            &macd_line,
            &macd_signal,
            &dc_hi,
            &dc_lo,
            &vol_mult,
            &syms,
            tstart,
            tend,
            false,
        );
        let (v_ret, v_sh, v_dd, v_trd) = run_backtest(
            &close,
            &macd_line,
            &macd_signal,
            &dc_hi,
            &dc_lo,
            &vol_mult,
            &syms,
            tstart,
            tend,
            true,
        );

        let b_pass = b_trd >= MIN_TRADES && b_ret > 0.0;
        let v_pass = v_trd >= MIN_TRADES && v_ret > 0.0;
        if b_pass {
            base_pass_cnt += 1;
        }
        if v_pass {
            vol_pass_cnt += 1;
        }
        if v_ret > b_ret {
            vol_better_ret += 1;
        }
        if v_dd < b_dd {
            vol_better_dd += 1;
        }
        base_total_ret += b_ret;
        vol_total_ret += v_ret;
        base_total_sh += b_sh;
        vol_total_sh += v_sh;
        base_total_dd += b_dd;
        vol_total_dd += v_dd;
        worst_base_dd = worst_base_dd.min(b_dd);
        worst_vol_dd = worst_vol_dd.min(v_dd);

        let improvement = if v_pass && !b_pass {
            "FAIL→PASS"
        } else if !v_pass && b_pass {
            "PASS→FAIL"
        } else if v_ret > b_ret {
            "▲"
        } else {
            "▼"
        };

        println!("{:<6} {:>7} | {:>+9.1} {:>6.2} {:>7.1} {:>5} | {:>+9.1} {:>6.2} {:>7.1} {:>5} | {:>6} ({:.1}s)",
            format!("W{:02}", wi), format!("b{}", tstart),
            b_ret, b_sh, -b_dd, b_trd,
            v_ret, v_sh, -v_dd, v_trd,
            improvement, t0.elapsed().as_secs_f64());

        // Clear HashMaps to free memory
        drop(close);
        drop(macd_line);
        drop(macd_signal);
        drop(dc_hi);
        drop(dc_lo);
        drop(vol_mult);
    }

    let nw = total_windows;
    if nw == 0 {
        return Ok(());
    }

    println!("\n══════════════════════════════════════════");
    println!("SUMMARY: {} windows", nw);
    println!(
        "BASE:  {}/{} pass | avg {:+.1}% | avg sh {:.2} | avg DD {:.1}% | worst DD {:.1}%",
        base_pass_cnt,
        nw,
        base_total_ret / nw as f64,
        base_total_sh / nw as f64,
        -base_total_dd / nw as f64,
        -worst_base_dd
    );
    println!(
        "VOL:   {}/{} pass | avg {:+.1}% | avg sh {:.2} | avg DD {:.1}% | worst DD {:.1}%",
        vol_pass_cnt,
        nw,
        vol_total_ret / nw as f64,
        vol_total_sh / nw as f64,
        -vol_total_dd / nw as f64,
        -worst_vol_dd
    );
    println!(
        "\nVol wins RET: {}/{} | wins DD: {}/{}",
        vol_better_ret, nw, vol_better_dd, nw
    );

    // Honest assessment
    let vol_improves_ret = vol_better_ret >= nw / 2;
    let vol_reduces_dd = vol_total_dd < base_total_dd;
    let vol_improves_pass = vol_pass_cnt >= base_pass_cnt;
    println!("\n─── Honest assessment ───");
    println!(
        "Vol-scaling {} pass rate ({}/{} → {}/{})",
        if vol_improves_pass {
            "improves"
        } else {
            "does NOT improve"
        },
        base_pass_cnt,
        nw,
        vol_pass_cnt,
        nw
    );
    println!(
        "Vol-scaling {} avg return ({:+.1}% → {:+.1}%)",
        if vol_improves_ret {
            "improves"
        } else {
            "does NOT improve"
        },
        base_total_ret / nw as f64,
        vol_total_ret / nw as f64
    );
    println!(
        "Vol-scaling {} avg DD ({:.1}% → {:.1}%)",
        if vol_reduces_dd {
            "reduces"
        } else {
            "does NOT reduce"
        },
        -base_total_dd / nw as f64,
        -vol_total_dd / nw as f64
    );
    println!(
        "Worst-case DD: BASE {:.1}% | VOL {:.1}%",
        -worst_base_dd, -worst_vol_dd
    );

    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn col_f64(df: &DataFrame, name: &str) -> Result<Vec<f64>> {
    use polars::chunked_array::ChunkedArray;
    let ca = df.column(name)?.f64()?;
    Ok(ca.into_iter().map(|v| v.unwrap_or(0.0)).collect())
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
    syms: &[&str],
    tstart: usize,
    tend: usize,
    use_vol_scale: bool,
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
            vm: f64,
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
            let vm = if use_vol_scale {
                *vol_mult.get(*sym).and_then(|v| v.get(bar)).unwrap_or(&1.0)
            } else {
                1.0
            };

            if price > dhi && ml > ms && btc_above_sma {
                longs.push(Entry {
                    sym: (*sym).to_string(),
                    direction: 1,
                    vm,
                });
            } else if price < dlo && ml < ms && !btc_above_sma {
                shorts.push(Entry {
                    sym: (*sym).to_string(),
                    direction: -1,
                    vm,
                });
            }
        }

        // Strength: rank by |entry - prior_close| (more signal = stronger)
        let mut long_entries: Vec<(String, i8, f64)> = longs
            .into_iter()
            .map(|e| {
                let prior = close
                    .get(&e.sym)
                    .and_then(|c| c.get(bar.wrapping_sub(1).min(c.len().saturating_sub(1))))
                    .copied()
                    .unwrap_or(e.vm as f64);
                let strength = (close
                    .get(&e.sym)
                    .and_then(|c| c.get(bar))
                    .copied()
                    .unwrap_or(0.0)
                    - prior)
                    .abs();
                (e.sym, e.direction, e.vm)
            })
            .collect();
        let mut short_entries: Vec<(String, i8, f64)> = shorts
            .into_iter()
            .map(|e| {
                let prior = close
                    .get(&e.sym)
                    .and_then(|c| c.get(bar.wrapping_sub(1).min(c.len().saturating_sub(1))))
                    .copied()
                    .unwrap_or(e.vm as f64);
                let strength = (close
                    .get(&e.sym)
                    .and_then(|c| c.get(bar))
                    .copied()
                    .unwrap_or(0.0)
                    - prior)
                    .abs();
                (e.sym, e.direction, e.vm)
            })
            .collect();

        // Sort by strength (highest first) and take top-3
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
            let n_active = (top_longs.len() + top_shorts.len()) as f64;

            for (sym, dir, vm) in top_longs.into_iter().chain(top_shorts.into_iter()) {
                if let (Some(cl), Some(ml), Some(ms)) =
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
                        rets.push(net * vm);
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

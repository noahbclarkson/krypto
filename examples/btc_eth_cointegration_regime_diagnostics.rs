//! BTC-ETH Cointegration Regime Diagnostics
//!
//! Follow-up to btc_eth_cointegration_pair_benchmark.rs.
//! The first pass found the pair ALIVE but FRAGILE (1/4 quarter passes,
//! +53.9% best row, 60.9% win rate, 15.6% DD).
//!
//! This diagnostic asks:
//! 1. Is cointegration stability time-varying? (rolling correlation)
//! 2. Which vol/trend regimes does the pair work in?
//! 3. Does regime-conditional execution beat always-on?
//!
//! Data: daily BTCUSDT + ETHUSDT parquet from Binance cache.
//! Signal at close → next-open entry.
//! 0.1% taker per side (both legs).

use krypto::data::DataLoader;
use polars::prelude::*;
use std::path::Path;

fn load_f64_col(df: &DataFrame, name: &str) -> Vec<f64> {
    df.column(name)
        .unwrap()
        .f64()
        .unwrap()
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect()
}

fn load_i64_col(df: &DataFrame, name: &str) -> Vec<i64> {
    let col = df.column(name).unwrap();
    if let Ok(ca) = col.i64() {
        ca.into_iter().map(|v| v.unwrap_or(0_i64)).collect()
    } else if let Ok(dt) = col.datetime() {
        dt.into_iter().map(|v| v.unwrap_or(0_i64)).collect()
    } else {
        vec![0_i64; df.height()]
    }
}

fn rolling_corr(b: &[f64], e: &[f64], w: usize) -> Vec<f64> {
    // b[0], e[0] are dummy 0.0; real data from index 1
    // Window at index i uses [i-w+1..=i] (inclusive), all real if i >= w
    let n = b.len();
    let warmup = w;
    let mut out = vec![0.0; n];
    for i in warmup..n {
        let mut sb = 0.0_f64;
        let mut se = 0.0_f64;
        let mut sbb = 0.0_f64;
        let mut see = 0.0_f64;
        let mut sbe = 0.0_f64;
        for j in (i - w + 1)..=i {
            sb += b[j];
            se += e[j];
            sbb += b[j] * b[j];
            see += e[j] * e[j];
            sbe += b[j] * e[j];
        }
        let nw = w as f64;
        let cov = (sbe - sb * se / nw) / nw;
        let vb = (sbb - sb * sb / nw) / nw;
        let ve = (see - se * se / nw) / nw;
        let sd_b = vb.sqrt().max(1e-10);
        let sd_e = ve.sqrt().max(1e-10);
        out[i] = if sd_b > 1e-9 && sd_e > 1e-9 {
            (cov / (sd_b * sd_e)).clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }
    let fill = out[warmup..]
        .iter()
        .copied()
        .find(|x| x.is_finite())
        .unwrap_or(0.0);
    for i in 0..warmup {
        out[i] = fill;
    }
    out
}

fn rolling_beta(b: &[f64], e: &[f64], w: usize) -> Vec<f64> {
    let n = b.len();
    let warmup = w;
    let mut out = vec![1.0; n];
    for i in warmup..n {
        let mut sb = 0.0_f64;
        let mut se = 0.0_f64;
        let mut sbb = 0.0_f64;
        let mut sbe = 0.0_f64;
        for j in (i - w + 1)..=i {
            sb += b[j];
            se += e[j];
            sbb += b[j] * b[j];
            sbe += b[j] * e[j];
        }
        let nw = w as f64;
        let cov = (sbe - sb * se / nw) / nw;
        let vb = (sbb - sb * sb / nw) / nw;
        out[i] = if vb > 1e-10 { cov / vb } else { 1.0 };
    }
    let fill = out[warmup..]
        .iter()
        .copied()
        .find(|x| x.is_finite())
        .unwrap_or(1.0);
    for i in 0..warmup {
        out[i] = fill;
    }
    out
}

fn rolling_zscore(er: &[f64], br: &[f64], beta: &[f64], w: usize) -> Vec<f64> {
    let n = er.len();
    let warmup = w;
    let mut out = vec![0.0; n];
    for i in warmup..n {
        let b = beta[i];
        let mut sp = Vec::with_capacity(w);
        for j in (i - w + 1)..=i {
            sp.push(er[j] - b * br[j]);
        }
        let mean = sp.iter().sum::<f64>() / w as f64;
        let var = sp.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / w as f64;
        let std = var.sqrt().max(1e-10);
        out[i] = (er[i] - b * br[i] - mean) / std;
    }
    for i in 0..warmup {
        out[i] = 0.0;
    }
    out
}

fn vol_regime_idx(btc_ret: &[f64], w: usize, n: usize) -> Vec<&'static str> {
    // Returns[0] = dummy; regime from index w+1 (first w+1 are warmup)
    let warmup = w + 1;
    (0..n)
        .map(|i| {
            if i <= warmup {
                "Warmup"
            } else {
                let slice = &btc_ret[(i - w)..i];
                let vr: f64 = slice.iter().map(|r| r.powi(2)).sum::<f64>() / w as f64;
                let ann = vr.sqrt() * (365.0_f64).sqrt();
                if ann > 0.80 {
                    "HighVol"
                } else if ann < 0.40 {
                    "LowVol"
                } else {
                    "MidVol"
                }
            }
        })
        .collect()
}

fn trend_regime_idx(btc_ret: &[f64], w: usize, n: usize) -> Vec<&'static str> {
    let warmup = w + 1;
    (0..n)
        .map(|i| {
            if i <= warmup {
                "Warmup"
            } else {
                let slice = &btc_ret[(i - w)..i];
                let cr: f64 = slice.iter().sum();
                let ann = cr * (365.0 / w as f64);
                if ann > 0.30 {
                    "StrongTrend"
                } else if ann < -0.15 {
                    "BearTrend"
                } else {
                    "Chop"
                }
            }
        })
        .collect()
}

fn main() {
    println!("=== BTC-ETH Cointegration Regime Diagnostics ===\n");

    let cache = Path::new("data/cache");
    let btc_df = match DataLoader::load_parquet(&cache.join("btcusdt_1d.parquet")) {
        Ok(df) => df,
        Err(e) => {
            eprintln!("ERROR loading BTC: {}", e);
            return;
        }
    };
    let eth_df = match DataLoader::load_parquet(&cache.join("ethusdt_1d.parquet")) {
        Ok(df) => df,
        Err(e) => {
            eprintln!("ERROR loading ETH: {}", e);
            return;
        }
    };

    // Positional alignment: both cover the same period at same frequency
    let n_raw = btc_df.height().min(eth_df.height());
    let btc_close: Vec<f64> = load_f64_col(&btc_df, "close")
        .into_iter()
        .take(n_raw)
        .collect();
    let eth_close: Vec<f64> = load_f64_col(&eth_df, "close")
        .into_iter()
        .take(n_raw)
        .collect();
    let btc_ts = load_i64_col(&btc_df, "time");
    let first_ts = btc_ts.first().copied().unwrap_or(0_i64);
    let unit = if first_ts > 1_000_000_000_000_i64 {
        1_000_000_i64
    } else {
        1_000_i64
    };
    let dates: Vec<i64> = btc_ts.iter().take(n_raw).map(|t| t / unit).collect();

    println!("Bars: {}", n_raw);
    println!("BTC close[0..5]: {:?}", &btc_close[..5]);
    println!("dates[0..5]: {:?}", &dates[..5]);

    // Compute log returns: ret[0] = 0.0; ret[i] for i>=1 is the real return
    let n = n_raw;
    let mut btc_ret = vec![0.0; n];
    let mut eth_ret = vec![0.0; n];
    let mut finite_count = 0;
    for i in 1..n {
        let r_b = (btc_close[i] / btc_close[i - 1] - 1.0).ln();
        let r_e = (eth_close[i] / eth_close[i - 1] - 1.0).ln();
        if r_b.is_finite() {
            finite_count += 1;
        }
        btc_ret[i] = r_b;
        eth_ret[i] = r_e;
    }
    let finite_pct = finite_count as f64 / (n - 1) as f64 * 100.0;
    let btc_mean: f64 = btc_ret[1..]
        .iter()
        .filter(|r| r.is_finite())
        .copied()
        .sum::<f64>()
        / btc_ret[1..].iter().filter(|r| r.is_finite()).count().max(1) as f64;
    println!(
        "BTC returns: finite={:.0}% ({}/{}), mean={:.4}",
        finite_pct,
        finite_count,
        n - 1,
        btc_mean
    );

    // Quick manual correlation on first 126 finite returns
    let mut sb = 0.0_f64;
    let mut se = 0.0_f64;
    let mut sbb = 0.0_f64;
    let mut see = 0.0_f64;
    let mut sbe = 0.0_f64;
    let mut cnt = 0;
    for i in 1..n {
        if btc_ret[i].is_finite() && eth_ret[i].is_finite() && cnt < 126 {
            sb += btc_ret[i];
            se += eth_ret[i];
            sbb += btc_ret[i] * btc_ret[i];
            see += eth_ret[i] * eth_ret[i];
            sbe += btc_ret[i] * eth_ret[i];
            cnt += 1;
        }
    }
    if cnt > 0 {
        let cov = (sbe - sb * sb / cnt as f64) / cnt as f64;
        let vb = (sbb - sb * sb / cnt as f64) / cnt as f64;
        let ve = (see - se * se / cnt as f64) / cnt as f64;
        let qc = cov / (vb.sqrt() * ve.sqrt());
        println!("Quick 126-return corr: {:.3}", qc);
    }

    // === DIAGNOSTIC 1: Rolling correlation stability ===
    println!("\n--- Rolling Correlation (Cointegration Stability Proxy) ---");
    for (w, lbl) in [(63, "63d"), (126, "126d"), (252, "252d")] {
        let corr = rolling_corr(&btc_ret, &eth_ret, w);
        let data: Vec<f64> = corr[(w + 1)..]
            .iter()
            .filter(|x| x.is_finite())
            .copied()
            .collect();
        if !data.is_empty() {
            let mean = data.iter().sum::<f64>() / data.len() as f64;
            let mn = data.iter().copied().fold(f64::INFINITY, f64::min);
            let mx = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            println!(
                "  {}: mean={:.3}, min={:.3}, max={:.3}, n={}",
                lbl,
                mean,
                mn,
                mx,
                data.len()
            );
        }
    }

    // === DIAGNOSTIC 2: Vol regime distribution ===
    println!("\n--- Vol Regime Distribution (BTC 63d ann vol) ---");
    let vreg = vol_regime_idx(&btc_ret, 63, n);
    let mut cnt_map: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for r in &vreg {
        *cnt_map.entry(r).or_insert(0) += 1;
    }
    let mut keys: Vec<_> = cnt_map.keys().collect();
    keys.sort();
    for k in keys {
        println!(
            "  {}: {} ({:.1}%)",
            k,
            cnt_map[k],
            cnt_map[k] as f64 / n as f64 * 100.0
        );
    }

    // === DIAGNOSTIC 3: Trend regime distribution ===
    println!("\n--- Trend Regime Distribution (BTC 63d ann ret) ---");
    let treg = trend_regime_idx(&btc_ret, 63, n);
    let mut cnt_map: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for r in &treg {
        *cnt_map.entry(r).or_insert(0) += 1;
    }
    let mut keys: Vec<_> = cnt_map.keys().collect();
    keys.sort();
    for k in keys {
        println!(
            "  {}: {} ({:.1}%)",
            k,
            cnt_map[k],
            cnt_map[k] as f64 / n as f64 * 100.0
        );
    }

    // === DIAGNOSTIC 4: Quarter-by-quarter pair performance ===
    println!("\n--- Quarter-by-Quarter Pair Performance ---");
    let fee = 0.001;

    let param_sets: [(usize, usize, f64, f64, usize); 4] = [
        (126, 126, 2.5, 0.0, 5),
        (63, 63, 1.5, 0.0, 21),
        (126, 42, 1.5, 0.5, 21),
        (252, 63, 2.0, 0.0, 10),
    ];

    for (bw, zw, ez, xz, mh) in param_sets {
        let beta = rolling_beta(&btc_ret, &eth_ret, bw);
        let z = rolling_zscore(&eth_ret, &btc_ret, &beta, zw);

        let mut trades = Vec::new();
        let mut pos: Option<(usize, f64, f64)> = None;

        for i in (bw.max(zw) + 2)..n {
            let zi = z[i];
            if pos.is_none() {
                if zi < -ez {
                    pos = Some((i, eth_close[i], btc_close[i]));
                }
            } else {
                let (eidx, e_eth, e_btc) = pos.unwrap();
                let bars = (dates[i] - dates[eidx]) as usize;
                let zexit = z[i];
                if zexit > 0.0 || bars >= mh {
                    let re = eth_close[i] / e_eth - 1.0;
                    let rb = btc_close[i] / e_btc - 1.0;
                    let pnl = re - beta[eidx] * rb - 2.0 * fee;
                    trades.push((pnl, dates[eidx]));
                    pos = None;
                }
            }
        }

        if trades.is_empty() {
            println!(
                "  beta={}, z={}, entry={:.1}, hold={}: NO TRADES",
                bw, zw, ez, mh
            );
            continue;
        }

        // Quarter grouping: dates are in microseconds / unit
        // 1 quarter ≈ 91.25 days = 91.25 * 86.4e6 microseconds / unit
        let q_len = 91_i64 * 86400 / (unit / 1_000_000_i64) as i64; // rough approx
        let mut qmap: std::collections::BTreeMap<i64, Vec<f64>> = std::collections::BTreeMap::new();
        for (pnl, d) in &trades {
            let q = d / q_len;
            qmap.entry(q).or_default().push(*pnl);
        }

        let mut q_pass = 0;
        for (q, pnls) in &qmap {
            let tot: f64 = pnls.iter().sum();
            let wr = pnls.iter().filter(|p| **p > 0.0).count() as f64 / pnls.len() as f64;
            let pass = if tot > 0.0 { "PASS" } else { "FAIL" };
            if tot > 0.0 {
                q_pass += 1;
            }
            println!(
                "  beta={}, z={}, Q{}: n={}, ret={:+.1}%, wr={:.0}%, {}",
                bw,
                zw,
                q,
                pnls.len(),
                tot * 100.0,
                wr * 100.0,
                pass
            );
        }

        let total: f64 = trades.iter().map(|t| t.0).sum();
        let wins = trades.iter().filter(|t| t.0 > 0.0).count();
        println!(
            "  --> {} quarters passed, total={:+.1}%, avg={:+.2}%, wr={:.0}%",
            q_pass,
            total * 100.0,
            total / trades.len() as f64 * 100.0,
            wins as f64 / trades.len() as f64 * 100.0
        );
    }

    // === DIAGNOSTIC 5: Regime-conditional pair performance ===
    println!("\n--- Regime-Conditional Pair Performance (best params) ---");
    let (bw, zw, ez, _xz, mh) = (126, 126, 2.5, 0.0, 5);
    let beta = rolling_beta(&btc_ret, &eth_ret, bw);
    let z = rolling_zscore(&eth_ret, &btc_ret, &beta, zw);

    let mut trades = Vec::new();
    let mut pos: Option<(usize, f64, f64)> = None;
    for i in (bw.max(zw) + 2)..n {
        let zi = z[i];
        if pos.is_none() {
            if zi < -ez {
                pos = Some((i, eth_close[i], btc_close[i]));
            }
        } else {
            let (eidx, e_eth, e_btc) = pos.unwrap();
            let bars = (dates[i] - dates[eidx]) as usize;
            if z[i] > 0.0 || bars >= mh {
                let re = eth_close[i] / e_eth - 1.0;
                let rb = btc_close[i] / e_btc - 1.0;
                let pnl = re - beta[eidx] * rb - 2.0 * fee;
                trades.push((pnl, dates[eidx]));
                pos = None;
            }
        }
    }

    println!(
        "Pair: beta={}, z={}, entry={:.1}, max_hold={}",
        bw, zw, ez, mh
    );
    println!("Total pair trades: {}", trades.len());

    if !trades.is_empty() {
        let date_to_vreg: std::collections::HashMap<i64, &str> = dates
            .iter()
            .zip(vreg.iter())
            .map(|(d, r)| (*d, *r))
            .collect();
        let date_to_treg: std::collections::HashMap<i64, &str> = dates
            .iter()
            .zip(treg.iter())
            .map(|(d, r)| (*d, *r))
            .collect();

        let mut vstats: std::collections::HashMap<&str, (usize, f64)> =
            std::collections::HashMap::new();
        let mut tstats: std::collections::HashMap<&str, (usize, f64)> =
            std::collections::HashMap::new();

        for (pnl, d) in &trades {
            if let Some(vr) = date_to_vreg.get(d) {
                let e = vstats.entry(vr).or_insert((0, 0.0));
                e.0 += 1;
                e.1 += pnl;
            }
            if let Some(tr) = date_to_treg.get(d) {
                let e = tstats.entry(tr).or_insert((0, 0.0));
                e.0 += 1;
                e.1 += pnl;
            }
        }

        println!("\nBy vol regime:");
        let mut vk: Vec<_> = vstats.keys().copied().collect();
        vk.sort();
        for k in vk {
            let (cnt, sum) = vstats[k];
            println!(
                "  {}: n={}, total={:+.1}%, avg={:+.2}%",
                k,
                cnt,
                sum * 100.0,
                sum / cnt as f64 * 100.0
            );
        }

        println!("\nBy trend regime:");
        let mut tk: Vec<_> = tstats.keys().copied().collect();
        tk.sort();
        for k in tk {
            let (cnt, sum) = tstats[k];
            println!(
                "  {}: n={}, total={:+.1}%, avg={:+.2}%",
                k,
                cnt,
                sum * 100.0,
                sum / cnt as f64 * 100.0
            );
        }
    }

    // === DIAGNOSTIC 6: Stability-threshold filter ===
    println!("\n--- Stability-Threshold Filter Test ---");
    let corr126 = rolling_corr(&btc_ret, &eth_ret, 126);

    for thresh in [0.40, 0.45, 0.50, 0.55, 0.60] {
        let mut filtered = Vec::new();

        for i in (bw.max(zw) + 2)..n {
            if corr126[i] <= thresh {
                continue;
            }
            if z[i] >= -ez {
                continue;
            }

            let entry_eth = eth_close[i];
            let entry_btc = btc_close[i];
            let entry_date = dates[i];
            let b = beta[i];

            let mut pnl = None;
            for h in 1..=mh {
                if i + h >= n {
                    break;
                }
                let ret_e = eth_close[i + h] / entry_eth - 1.0;
                let ret_b = btc_close[i + h] / entry_btc - 1.0;
                let bars = (dates[i + h] - entry_date) as usize;
                if z[i + h] > 0.0 || bars >= mh {
                    pnl = Some(ret_e - b * ret_b - 2.0 * fee);
                    break;
                }
            }
            if let Some(p) = pnl {
                filtered.push(p);
            }
        }

        if filtered.is_empty() {
            println!("  corr>{:.2}: NO TRADES", thresh);
        } else {
            let total: f64 = filtered.iter().sum();
            let wins = filtered.iter().filter(|p| **p > 0.0).count();
            let avg = total / filtered.len() as f64;
            let mut peak = 0.0_f64;
            let mut max_dd = f64::NEG_INFINITY;
            for p in &filtered {
                peak += p;
                let dd = peak.min(0.0);
                if -dd > max_dd {
                    max_dd = -dd;
                }
                if peak < 0.0 {
                    peak = 0.0;
                }
            }
            println!(
                "  corr>{:.2}: n={}, total={:+.1}%, avg={:+.2}%, DD={:.1}%, wr={:.0}%",
                thresh,
                filtered.len(),
                total * 100.0,
                avg * 100.0,
                max_dd * 100.0,
                wins as f64 / filtered.len() as f64 * 100.0
            );
        }
    }

    println!("\n=== Done ===");
}

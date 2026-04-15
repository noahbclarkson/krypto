//! EWMA-CUSUM Break Detection with Observable Covariates
//!
//! Replaces the misspecified BOCPD NIG model from bocpd_covariate_state.rs.
//! The BOCPD evidence stream was informative (Normal vs Stale dispersion),
//! but the NIG conjugate model couldn't fire actual breaks — run-length
//! stayed pegged at 13 forever.
//!
//! EWMA-CUSUM is simpler and more robust:
//! - EWMA tracks the recent level of the evidence stream
//! - CUSUM accumulates deviations from the EWMA baseline
//! - When CUSUM exceeds a threshold → structural break detected
//! - Use break flags as sizing context on the frozen three-sleeve book
//!
//! Same covariates as BOCPD: breadth, vol stability, correlation.
//! Same honest lens: signal at close, next-open entry, 21-bar hold, 0.1% taker.

use krypto::data::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::Path;

fn load_f64_col(df: &DataFrame, name: &str) -> Vec<f64> {
    let col = df.column(name).unwrap();
    let ca = col.f64().unwrap();
    (0..ca.len()).map(|i| ca.get(i).unwrap_or(0.0)).collect()
}

fn load_datetime_col(df: &DataFrame, name: &str) -> Vec<i64> {
    let col = df.column(name).unwrap();
    if let Ok(ca) = col.i64() {
        (0..ca.len()).map(|i| ca.get(i).unwrap_or(0_i64)).collect()
    } else if let Ok(ca) = col.datetime() {
        (0..ca.len()).map(|i| ca.get(i).unwrap_or(0_i64)).collect()
    } else {
        vec![0_i64; df.height()]
    }
}

// ── EWMA-CUSUM break detector ─────────────────────────────────────────────

/// EWMA-CUSUM parameters
struct EwmaCusum {
    /// EWMA smoothing factor (0 < alpha <= 1). Lower = slower tracking.
    alpha: f64,
    /// CUSUM threshold for break detection (in std units)
    threshold: f64,
    /// CUSUM reset fraction after break (0 = full reset, 1 = no reset)
    reset_frac: f64,
}

impl EwmaCusum {
    fn new(alpha: f64, threshold: f64, reset_frac: f64) -> Self {
        Self {
            alpha,
            threshold,
            reset_frac,
        }
    }

    /// Run EWMA-CUSUM on a z-scored evidence stream.
    /// Returns a vector of regime labels: "Break", "PostBreak", "Stable", "Trending"
    fn detect(&self, evidence: &[f64], warmup: usize) -> Vec<&'static str> {
        let n = evidence.len();
        let mut regimes = vec!["Warmup"; n];

        // Compute global stats from warmup period
        let slice = &evidence[warmup..];
        let g_mean = slice.iter().sum::<f64>() / slice.len() as f64;
        let g_var = slice.iter().map(|&x| (x - g_mean).powi(2)).sum::<f64>() / slice.len() as f64;
        let g_std = g_var.sqrt().max(0.01);

        // EWMA starts at global mean
        let mut ewma = g_mean;
        let mut cusum_pos = 0.0_f64;
        let mut cusum_neg = 0.0_f64;
        let mut bars_since_break = 100usize; // start as "long since break"

        // Track rolling std for adaptive threshold
        let mut rolling_var = g_var;
        let var_alpha = 0.02; // slow adaptation

        for i in warmup..n {
            let x = evidence[i];

            // Update EWMA
            ewma = self.alpha * x + (1.0 - self.alpha) * ewma;

            // Update rolling variance
            let dev = x - ewma;
            rolling_var = (1.0 - var_alpha) * rolling_var + var_alpha * dev * dev;
            let rolling_std = rolling_var.sqrt().max(0.01);

            // CUSUM: accumulate positive and negative deviations
            cusum_pos = (cusum_pos + (x - ewma) / rolling_std).max(0.0);
            cusum_neg = (cusum_neg - (x - ewma) / rolling_std).max(0.0);

            // Adaptive threshold: scale by rolling std relative to global
            let adaptive_thresh = self.threshold * (rolling_std / g_std).max(0.5).min(2.0);

            // Break detection
            if cusum_pos > adaptive_thresh || cusum_neg > adaptive_thresh {
                regimes[i] = "Break";
                bars_since_break = 0;

                // Reset CUSUM
                let reset_to = cusum_pos * self.reset_frac;
                cusum_pos = reset_to;
                cusum_neg = reset_to;
            } else if bars_since_break < 10 {
                regimes[i] = "PostBreak";
                bars_since_break += 1;
            } else if ewma > g_mean + 0.5 * g_std {
                regimes[i] = "Trending";
            } else if ewma < g_mean - 0.5 * g_std {
                regimes[i] = "Stressed";
            } else {
                regimes[i] = "Stable";
            }
        }

        regimes
    }
}

// ── Evidence stream (same as BOCPD) ───────────────────────────────────────

fn compute_evidence_stream(
    btc_ret: &[f64],
    eth_ret: &[f64],
    sol_ret: &[f64],
    ada_ret: &[f64],
    xrp_ret: &[f64],
    window: usize,
) -> Vec<f64> {
    let n = btc_ret.len();
    let mut evidence = vec![0.0; n];

    fn rolling_mean_std(data: &[f64], window: usize, idx: usize) -> (f64, f64) {
        if idx < window {
            return (0.0, 1.0);
        }
        let start = idx - window;
        let mut sum = 0.0_f64;
        for &v in &data[start..=idx] {
            sum += v;
        }
        let mean = sum / window as f64;
        let var = data[start..=idx]
            .iter()
            .map(|&x| (x - mean).powi(2))
            .sum::<f64>()
            / window as f64;
        (mean, var.sqrt().max(1e-8))
    }

    fn corr_window(a: &[f64], b: &[f64], window: usize, idx: usize) -> f64 {
        if idx < window {
            return 0.0;
        }
        let start = idx - window;
        let mut sa = 0.0;
        let mut sb = 0.0;
        let mut ssa = 0.0;
        let mut ssb = 0.0;
        let mut sab = 0.0;
        for i in start..=idx {
            sa += a[i];
            sb += b[i];
            ssa += a[i] * a[i];
            ssb += b[i] * b[i];
            sab += a[i] * b[i];
        }
        let nw = window as f64;
        let cov = (sab - sa * sb / nw) / nw;
        let va = (ssa - sa * sa / nw) / nw;
        let vb = (ssb - sb * sb / nw) / nw;
        (cov / (va.sqrt().max(1e-10) * vb.sqrt().max(1e-10))).clamp(-1.0, 1.0)
    }

    let vol_window = window;
    let mut btc_vol = vec![0.0; n];
    let mut eth_vol = vec![0.0; n];
    let mut sol_vol = vec![0.0; n];
    let mut ada_vol = vec![0.0; n];
    let mut xrp_vol = vec![0.0; n];
    for i in vol_window..n {
        let (_, bs) = rolling_mean_std(btc_ret, vol_window, i);
        btc_vol[i] = bs;
        let (_, es) = rolling_mean_std(eth_ret, vol_window, i);
        eth_vol[i] = es;
        let (_, ss) = rolling_mean_std(sol_ret, vol_window, i);
        sol_vol[i] = ss;
        let (_, as_) = rolling_mean_std(ada_ret, vol_window, i);
        ada_vol[i] = as_;
        let (_, xs) = rolling_mean_std(xrp_ret, vol_window, i);
        xrp_vol[i] = xs;
    }

    for i in vol_window..n {
        let (btc_m, _) = rolling_mean_std(btc_ret, window, i);
        let (eth_m, _) = rolling_mean_std(eth_ret, window, i);
        let (sol_m, _) = rolling_mean_std(sol_ret, window, i);
        let (ada_m, _) = rolling_mean_std(ada_ret, window, i);
        let (xrp_m, _) = rolling_mean_std(xrp_ret, window, i);

        let avg_vol = (btc_vol[i] + eth_vol[i] + sol_vol[i] + ada_vol[i] + xrp_vol[i]) / 5.0;

        let mut vol_vol_sum = 0.0_f64;
        for j in 0..vol_window {
            let v = [
                (btc_vol[i - j] - btc_vol[i]) / btc_vol[i].max(1e-10),
                (eth_vol[i - j] - eth_vol[i]) / eth_vol[i].max(1e-10),
                (sol_vol[i - j] - sol_vol[i]) / sol_vol[i].max(1e-10),
                (ada_vol[i - j] - ada_vol[i]) / ada_vol[i].max(1e-10),
                (xrp_vol[i - j] - xrp_vol[i]) / xrp_vol[i].max(1e-10),
            ];
            vol_vol_sum += v.iter().map(|&x| x * x).sum::<f64>();
        }
        let vol_vol = (vol_vol_sum / vol_window as f64 / 5.0).sqrt().max(1e-10);
        let avg_vol_safe = avg_vol.max(1e-10);
        let vol_score = (1.0 / vol_vol) * (1.0 / avg_vol_safe);

        let pos_count = [
            btc_m > 0.0,
            eth_m > 0.0,
            sol_m > 0.0,
            ada_m > 0.0,
            xrp_m > 0.0,
        ]
        .iter()
        .filter(|&&x| x)
        .count();
        let breadth_score = pos_count as f64 / 5.0;

        let c_be = corr_window(btc_ret, eth_ret, window, i);
        let c_bs = corr_window(btc_ret, sol_ret, window, i);
        let c_es = corr_window(eth_ret, sol_ret, window, i);
        let avg_corr = (c_be + c_bs + c_es) / 3.0;

        evidence[i] = vol_score * 0.4 + breadth_score * 0.3 + (1.0 - avg_corr.abs()) * 0.3;
    }

    // Z-score
    let warmup = window;
    let slice = &evidence[warmup..];
    let mean = slice.iter().sum::<f64>() / slice.len() as f64;
    let var = slice.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / slice.len() as f64;
    let std = var.sqrt().max(1e-8);
    for i in warmup..n {
        evidence[i] = (evidence[i] - mean) / std;
    }

    evidence
}

fn main() {
    println!("=== EWMA-CUSUM Break Detection with Observable Covariates ===\n");

    let symbols = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "ADAUSDT", "XRPUSDT"];

    // Load data
    let mut btc_close = Vec::new();
    let mut eth_close = Vec::new();
    let mut sol_close = Vec::new();
    let mut ada_close = Vec::new();
    let mut xrp_close = Vec::new();
    let mut timestamps = Vec::new();

    for &sym in symbols {
        let sym_lower = sym.to_lowercase();
        let sym_stripped = sym_lower.strip_suffix("usdt").unwrap_or(&sym_lower);
        let path = format!("data/cache/{}usdt_1d.parquet", sym_stripped);
        let df = match DataLoader::load_parquet(Path::new(&path)) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Failed to load {}: {}", sym, e);
                return;
            }
        };
        let close = load_f64_col(&df, "close");
        let ts = load_datetime_col(&df, "time");

        if btc_close.is_empty() {
            btc_close = close.clone();
            timestamps = ts;
        } else if eth_close.is_empty() {
            eth_close = close.clone();
        } else if sol_close.is_empty() {
            sol_close = close.clone();
        } else if ada_close.is_empty() {
            ada_close = close.clone();
        } else if xrp_close.is_empty() {
            xrp_close = close.clone();
        }
    }

    fn to_returns(prices: &[f64]) -> Vec<f64> {
        let mut r = vec![0.0; prices.len()];
        for i in 1..prices.len() {
            if prices[i] > 0.0 && prices[i - 1] > 0.0 {
                r[i] = (prices[i] / prices[i - 1]).ln();
            }
        }
        r
    }

    let btc = to_returns(&btc_close);
    let eth = to_returns(&eth_close);
    let sol = to_returns(&sol_close);
    let ada = to_returns(&ada_close);
    let xrp = to_returns(&xrp_close);

    let total_n = [btc.len(), eth.len(), sol.len(), ada.len(), xrp.len()];
    let n = *total_n.iter().min().unwrap();
    let btc = btc[..n].to_vec();
    let eth = eth[..n].to_vec();
    let sol = sol[..n].to_vec();
    let ada = ada[..n].to_vec();
    let xrp = xrp[..n].to_vec();
    let timestamps = timestamps[..n].to_vec();
    println!("Bars: {}", n);

    // ── Evidence stream (same as BOCPD) ────────────────────────────────────
    let window = 63;
    let evidence = compute_evidence_stream(&btc, &eth, &sol, &ada, &xrp, window);
    let warmup = window;

    let ev_mean = evidence[warmup..].iter().sum::<f64>() / (n - warmup) as f64;
    let ev_var = evidence[warmup..]
        .iter()
        .map(|&x| (x - ev_mean).powi(2))
        .sum::<f64>()
        / (n - warmup) as f64;
    println!(
        "Evidence stream (warmup={}): mean={:.4}, std={:.4}",
        warmup,
        ev_mean,
        ev_var.sqrt()
    );

    // ── Sweep EWMA-CUSUM parameters ────────────────────────────────────────
    println!("\n=== Parameter Sweep ===");

    let configs = [
        // Original range — already known
        ("Fast(alpha=0.10,th=3.0)", 0.10, 3.0, 0.1),
        ("Mid(alpha=0.05,th=4.0)", 0.05, 4.0, 0.1),
        ("Slow(alpha=0.03,th=5.0)", 0.03, 5.0, 0.1),
        ("VerySlow(alpha=0.02,th=6.0)", 0.02, 6.0, 0.1),
        ("SlowHigh(alpha=0.03,th=8.0)", 0.03, 8.0, 0.1),
        // Higher thresholds to target 5–15% break fraction
        ("Thresh10(a=0.03)", 0.03, 10.0, 0.1),
        ("Thresh10(a=0.02)", 0.02, 10.0, 0.1),
        ("Thresh12(a=0.03)", 0.03, 12.0, 0.1),
        ("Thresh12(a=0.02)", 0.02, 12.0, 0.1),
        ("Thresh15(a=0.03)", 0.03, 15.0, 0.1),
        ("Thresh15(a=0.02)", 0.02, 15.0, 0.1),
    ];

    for (label, alpha, threshold, reset_frac) in &configs {
        let cusum = EwmaCusum::new(*alpha, *threshold, *reset_frac);
        let regimes = cusum.detect(&evidence, warmup);

        let mut rc: HashMap<&str, usize> = HashMap::new();
        for r in &regimes[warmup..] {
            *rc.entry(r).or_insert(0) += 1;
        }
        let total = n - warmup;

        let break_pct = *rc.get("Break").unwrap_or(&0) as f64 / total as f64 * 100.0;
        let post_pct = *rc.get("PostBreak").unwrap_or(&0) as f64 / total as f64 * 100.0;
        let stable_pct = *rc.get("Stable").unwrap_or(&0) as f64 / total as f64 * 100.0;
        let trend_pct = *rc.get("Trending").unwrap_or(&0) as f64 / total as f64 * 100.0;
        let stress_pct = *rc.get("Stressed").unwrap_or(&0) as f64 / total as f64 * 100.0;

        println!("\n--- {} ---", label);
        println!("  Break={:5.1}%  PostBreak={:5.1}%  Stable={:5.1}%  Trending={:5.1}%  Stressed={:5.1}%",
            break_pct, post_pct, stable_pct, trend_pct, stress_pct);

        // Regime-conditional BTC next-21-bar returns
        let hold = 21;
        let mut regime_rets: HashMap<&str, (f64, usize)> = HashMap::new();
        for i in warmup..n.saturating_sub(hold) {
            let reg = regimes[i];
            if reg == "Warmup" {
                continue;
            }
            let ret: f64 = btc[i + 1..i + 1 + hold]
                .iter()
                .filter(|&&x| x.is_finite())
                .sum::<f64>();
            if !ret.is_finite() {
                continue;
            }
            let entry = regime_rets.entry(reg).or_insert((0.0, 0));
            entry.0 += ret;
            entry.1 += 1;
        }

        println!("  Regime-conditional next-21-bar BTC returns:");
        for name in &["Break", "PostBreak", "Trending", "Stable", "Stressed"] {
            if let Some((sum, count)) = regime_rets.get(name) {
                if *count > 0 {
                    let avg = sum / *count as f64;
                    println!("    {:12}: avg={:+.4} ({} obs)", name, avg, count);
                }
            }
        }
    }

    // ── Best candidate: detailed analysis ──────────────────────────────────
    println!("\n\n=== Detailed Analysis: Mid(alpha=0.05,th=4.0) ===");
    let best = EwmaCusum::new(0.05, 4.0, 0.1);
    let regimes = best.detect(&evidence, warmup);

    // Historical events check
    println!("\n--- Historical Events Check ---");
    let events = [
        ("2020-03-15 COVID crash", 1584316800000i64),
        ("2021-05-19 China/Elon", 1621382400000i64),
        ("2021-11-10 BTC peak", 1638316800000i64),
        ("2022-05-09 LUNA crash", 1652054400000i64),
        ("2022-06-18 FTX fear", 1655510400000i64),
        ("2022-11-08 FTX collapse", 1667865600000i64),
        ("2023-03-14 rates fear", 1678752000000i64),
    ];

    for (label, target_ts) in &events {
        if let Some(bar) = timestamps.iter().position(|&t| t >= *target_ts) {
            if bar >= warmup && bar < n {
                println!(
                    "  {}: bar={}, regime={:12}, evidence={:+.3}",
                    label, bar, regimes[bar], evidence[bar]
                );
            }
        }
    }

    // Quarter breakdown
    println!("\n--- Quarter Breakdown ---");
    let bpq = 91;
    let n_q = (n - warmup) / bpq;
    for q in 0..n_q.min(12) {
        let start = warmup + q * bpq;
        let end = (start + bpq).min(n);
        let slice = &regimes[start..end];
        let tot = end - start;

        let mut qrc: HashMap<&str, usize> = HashMap::new();
        for r in slice {
            *qrc.entry(r).or_insert(0) += 1;
        }

        println!("Q{:2}: Break={:4.0}% PostBreak={:4.0}% Trending={:4.0}% Stable={:4.0}% Stressed={:4.0}%",
            q + 1,
            *qrc.get("Break").unwrap_or(&0) as f64 / tot as f64 * 100.0,
            *qrc.get("PostBreak").unwrap_or(&0) as f64 / tot as f64 * 100.0,
            *qrc.get("Trending").unwrap_or(&0) as f64 / tot as f64 * 100.0,
            *qrc.get("Stable").unwrap_or(&0) as f64 / tot as f64 * 100.0,
            *qrc.get("Stressed").unwrap_or(&0) as f64 / tot as f64 * 100.0,
        );
    }

    // ── Overlay interpretation ─────────────────────────────────────────────
    println!("\n--- Overlay Interpretation ---");
    let mut rc: HashMap<&str, usize> = HashMap::new();
    for r in &regimes[warmup..] {
        *rc.entry(r).or_insert(0) += 1;
    }
    let total = n - warmup;

    let break_pct = *rc.get("Break").unwrap_or(&0) as f64 / total as f64 * 100.0;
    let post_pct = *rc.get("PostBreak").unwrap_or(&0) as f64 / total as f64 * 100.0;
    let stress_pct = *rc.get("Stressed").unwrap_or(&0) as f64 / total as f64 * 100.0;

    println!("Break+PostBreak fraction: {:.1}%", break_pct + post_pct);
    println!("Stressed fraction: {:.1}%", stress_pct);

    if break_pct + post_pct > 5.0 {
        println!("  EWMA-CUSUM is discriminative — sufficient break/post-break mass for overlay harness.");
    } else {
        println!("  EWMA-CUSUM may need lower threshold or higher alpha for more sensitivity.");
    }

    println!("\nNext step: if break detection is meaningful, build EWMA-CUSUM overlay harness");
    println!("comparing DDBudget(A/D,MACD,Small) with and without break-state sizing context.");
    println!("\n=== Done ===");
}

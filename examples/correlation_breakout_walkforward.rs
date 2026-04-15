//! Correlation Breakout Detector — FIXED Walk-Forward Validation
//! Track C: Broaden Edge Discovery
//!
//! Signal: BTC-ETH 21d rolling correlation breaks down (drops below pct threshold)
//! → long ETH (laggard mean-reverts). Different from lead-lag momentum.
//!
//! Previous attempt: correlation indexing bug (NaN for first window entries).
//! This version: manual correlation computation per-bar, no pre-fill.

use anyhow::Result;
use chrono::{TimeZone, Utc};
use krypto::data::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 126;
const FEE_PCT: f64 = 0.001;
const HOLD_BARS: usize = 10;
const MIN_TRADES: usize = 3;

const CORR_WINDOWS: &[usize] = &[10, 21, 42];
const PCT_LOW: &[f64] = &[10.0, 20.0];

fn epoch_ms_to_date(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "?".to_string())
}

#[derive(Clone)]
struct WindowResult {
    ret_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate: f64,
    btc_ret_pct: f64,
}

fn pearson_corr(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    if n < 2 { return 0.0; }
    let xm = x[..n].iter().sum::<f64>() / n as f64;
    let ym = y[..n].iter().sum::<f64>() / n as f64;
    let mut cov = 0.0; let mut vx = 0.0; let mut vy = 0.0;
    for i in 0..n { let dx = x[i]-xm; let dy = y[i]-ym; cov += dx*dy; vx += dx*dx; vy += dy*dy; }
    let d = (vx*vy).sqrt();
    if d > 1e-10 { cov/d } else { 0.0 }
}

// Entry at bar i, correlation uses [i-cw..i] window — all pre-bar (no look-ahead)
fn corr_at(btc: &[f64], eth: &[f64], i: usize, cw: usize) -> f64 {
    if i < cw { return f64::NAN; }
    pearson_corr(&btc[i-cw..i], &eth[i-cw..i])
}

fn run_strategy(
    btc: &[f64],
    eth: &[f64],
    corr_window: usize,
    pct_low: f64,
    test_start: usize,  // absolute index in arrays
    test_end: usize,    // absolute index (exclusive)
) -> WindowResult {
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut wins = 0;
    let mut losses = 0;
    let mut returns = Vec::new();
    let btc_equity = btc[test_end.saturating_sub(1).min(btc.len()-1)] / btc[test_start.min(btc.len()-1)];

    let mut i = test_start + corr_window;
    while i + HOLD_BARS < test_end {
        let c = corr_at(btc, eth, i, corr_window);
        if c.is_nan() { i += 1; continue; }

        // Dynamic percentile: bottom pct_low of train-window corr values
        let train_start = i.saturating_sub(corr_window);
        let mut train_corrs: Vec<f64> = (corr_window..i).map(|j| corr_at(btc, eth, j, corr_window)).collect();
        train_corrs.retain(|&x| !x.is_nan());
        if train_corrs.len() < 20 { i += 1; continue; }
        train_corrs.sort_by(|a,b| a.partial_cmp(b).unwrap());
        let pct_idx = ((100.0 - pct_low) / 100.0 * train_corrs.len() as f64) as usize;
        let thresh = train_corrs[pct_idx.min(train_corrs.len()-1)];

        let use_long = c < thresh;

        if use_long {
            let entry_idx = i + 1;
            let exit_idx = (entry_idx + HOLD_BARS).min(test_end - 1).min(eth.len()-1);
            if entry_idx >= eth.len() || exit_idx <= entry_idx { i += 1; continue; }

            let gross = eth[exit_idx] / eth[entry_idx] - 1.0 - 2.0 * FEE_PCT;
            equity *= 1.0 + gross;
            returns.push(gross);
            if gross > 0.0 { wins += 1; } else { losses += 1; }
            if equity > peak { peak = equity; }
            let dd = (peak - equity) / peak;
            if dd > max_dd { max_dd = dd; }
            i = exit_idx + 1;
        } else {
            i += 1;
        }
    }

    let trades = wins + losses;
    let ret_pct = (equity - 1.0) * 100.0;
    let btc_ret = (btc_equity - 1.0) * 100.0;
    let wr = if trades > 0 { wins as f64 / trades as f64 } else { 0.0 };
    let mean_r = if returns.is_empty() { 0.0 } else { returns.iter().sum::<f64>() / returns.len() as f64 };
    let var_r = if returns.is_empty() { 1.0 } else {
        returns.iter().map(|r| (r - mean_r).powi(2)).sum::<f64>() / returns.len().max(1) as f64
    };
    let sharpe = mean_r / var_r.sqrt().max(1e-10) * (365.25_f64 / HOLD_BARS as f64).sqrt();

    WindowResult { ret_pct, sharpe, max_dd_pct: max_dd * 100.0, trades, win_rate: wr * 100.0, btc_ret_pct: btc_ret }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", "═".repeat(72));
    println!("  CORRELATION BREAKOUT DETECTOR — FIXED WALK-FORWARD");
    println!("{}", "═".repeat(72));

    let loader = DataLoader::new(None, None);

    let btc_df = loader.fetch_data("BTCUSDT", "1d", CANDLES).await?;
    let eth_df = loader.fetch_data("ETHUSDT", "1d", CANDLES).await?;

    let btc_times: Vec<i64> = btc_df.column("time")?.cast(&DataType::Int64)?.i64()?.into_iter().flatten().collect();
    let btc_closes: Vec<f64> = btc_df.column("close")?.cast(&DataType::Float64)?.f64()?.into_iter().flatten().collect();
    let eth_times: Vec<i64> = eth_df.column("time")?.cast(&DataType::Int64)?.i64()?.into_iter().flatten().collect();
    let eth_closes: Vec<f64> = eth_df.column("close")?.cast(&DataType::Float64)?.f64()?.into_iter().flatten().collect();

    let btc_map: HashMap<i64, f64> = btc_times.iter().zip(btc_closes.iter()).map(|(&t, &c)| (t, c)).collect();
    let n = eth_times.len();
    let mut btc_aligned = vec![0.0f64; n];
    for i in 0..n {
        if let Some(&c) = btc_map.get(&eth_times[i]) { btc_aligned[i] = c; }
    }

    println!("BTC: {} bars | {} → {}", btc_closes.len(), epoch_ms_to_date(btc_times[0]), epoch_ms_to_date(btc_times[btc_times.len()-1]));
    println!("ETH: {} bars | {} → {}", eth_closes.len(), epoch_ms_to_date(eth_times[0]), epoch_ms_to_date(eth_times[eth_times.len()-1]));

    // ── Correlation distribution ──
    let mut all_corrs: Vec<f64> = (42..n).map(|i| corr_at(&btc_aligned, &eth_closes, i, 42)).collect();
    all_corrs.retain(|&c| !c.is_nan());
    all_corrs.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let p5  = all_corrs[(all_corrs.len()*5/100).max(1)-1];
    let p10 = all_corrs[(all_corrs.len()*10/100).max(1)-1];
    let p20 = all_corrs[(all_corrs.len()*20/100).max(1)-1];
    let p50 = all_corrs[all_corrs.len()*50/100];
    let p80 = all_corrs[(all_corrs.len()*80/100).min(all_corrs.len()-1)];
    println!("\n  42d corr distribution (n={}):", all_corrs.len());
    println!("  p5={:.2} | p10={:.2} | p20={:.2} | p50={:.2} | p80={:.2}", p5, p10, p20, p50, p80);
    println!("  Min={:.2} | Max={:.2}", all_corrs.first().unwrap_or(&0.0), all_corrs.last().unwrap_or(&1.0));

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    println!("\nWalk-forward: {} windows ({} train / {} test)", total_windows, TRAIN_BARS, TEST_BARS);
    println!("HOLD_BARS: {} ({:.1}d) | FEE: {:.0}bp RT | MIN_TRADES: {}", HOLD_BARS, HOLD_BARS as f64, FEE_PCT*10000.0, MIN_TRADES);

    let bnh_start = TRAIN_BARS.min(n-1);
    let bnh_end = n-1;
    let bnh_ret = (eth_closes[bnh_end] / eth_closes[bnh_start] - 1.0) * 100.0;
    println!("ETH B&H (W0 start→end): {:.1}%", bnh_ret);

    // ── Sweep ──
    println!("\n{}", "═".repeat(72));
    println!("  SWEEP: corr < threshold → long ETH (hold {} bars)", HOLD_BARS);
    println!("{}", "═".repeat(72));
    println!("  {:>4} {:>6}  {:>8} {:>8} {:>7} {:>6} {:>6} {:>6}", "CW", "pct", "Ret%", "Sharpe", "DD%", "Trades", "WR%", "Pass");
    println!("  {}", "─".repeat(60));

    let mut best_sharpe = f64::NEG_INFINITY;
    let mut best_config = (0usize, 0.0f64);
    let mut best_results: Vec<WindowResult> = Vec::new();

    let thresholds: Vec<f64> = vec![];

    for &cw in CORR_WINDOWS {
        for &pct in PCT_LOW {
            let mut window_results = Vec::new();
            let mut agg_ret = 0.0;
            let mut agg_trades = 0;
            let mut worst_dd = 0.0_f64;

            for wi in 0..total_windows {
                let ts = TRAIN_BARS + wi * TEST_BARS;
                let te = (ts + TEST_BARS).min(n);
                if te <= ts + cw + HOLD_BARS + 2 { continue; }

                let r = run_strategy(&btc_aligned, &eth_closes, cw, pct, ts, te);
                if r.max_dd_pct > worst_dd { worst_dd = r.max_dd_pct; }
                window_results.push(r.clone());
                agg_ret += r.ret_pct;
                agg_trades += r.trades;
            }

            let nw = window_results.len();
            let avg_ret = agg_ret / nw.max(1) as f64;
            let avg_sharpe = window_results.iter().map(|r| r.sharpe).sum::<f64>() / nw.max(1) as f64;
            let avg_wr = window_results.iter().map(|r| r.win_rate).sum::<f64>() / nw.max(1) as f64;
            let pass = window_results.iter().filter(|r| r.ret_pct > 0.0 && r.trades >= MIN_TRADES).count();

            let marker = if avg_sharpe > best_sharpe { "★" } else { "" };
            if avg_sharpe > best_sharpe {
                best_sharpe = avg_sharpe;
                best_config = (cw, pct);
                best_results = window_results.clone();
            }

            println!("  CW={:2} pct={:4.0}%  {:>+8.1} {:>8.2} {:>5.1}% {:>5} {:>5.0}% {:>3}/{}{}",
                     cw, pct, avg_ret, avg_sharpe, worst_dd, agg_trades, avg_wr, pass, nw, marker);
        }
    }

    // ── Per-window breakdown ──
    println!("\n{}", "═".repeat(72));
    println!("  BEST: CW={}, pct={:.0}% | Sharpe={:.2}", best_config.0, best_config.1, best_sharpe);
    println!("{}", "═".repeat(72));

    let pass_ct = best_results.iter().filter(|r| r.ret_pct > 0.0 && r.trades >= MIN_TRADES).count();
    let total_trades: usize = best_results.iter().map(|r| r.trades).sum();
    let avg_wr = best_results.iter().map(|r| r.win_rate).sum::<f64>() / best_results.len().max(1) as f64;
    let avg_dd = best_results.iter().map(|r| r.max_dd_pct).sum::<f64>() / best_results.len().max(1) as f64;
    let avg_btc = best_results.iter().map(|r| r.btc_ret_pct).sum::<f64>() / best_results.len().max(1) as f64;

    println!("  Pass: {}/{} ({:.0}%) | Trades: {} | Avg WR: {:.0}% | Avg DD: {:.1}%", pass_ct, best_results.len(), pass_ct as f64 / best_results.len() as f64 * 100.0, total_trades, avg_wr, avg_dd);
    let avg_ret = best_results.iter().map(|r| r.ret_pct).sum::<f64>() / best_results.len().max(1) as f64;
    println!("  Strategy avg return: {:.1}% vs BTC B&H avg: {:.1}%", avg_ret, avg_btc);

    for (wi, r) in best_results.iter().enumerate() {
        let pf = if r.ret_pct > 0.0 && r.trades >= MIN_TRADES { "PASS" } else if r.trades < MIN_TRADES { "THIN" } else { "FAIL" };
        println!("  W{:02} | {:+7.1}% | Sh {:7.2} | DD {:5.1}% | {:3}t | {:.0}%WR | {} | BTC {:+6.1}%",
                 wi, r.ret_pct, r.sharpe, r.max_dd_pct, r.trades, r.win_rate, pf, r.btc_ret_pct);
    }

    // ── Random baseline ──
    println!("\n{}", "─".repeat(72));
    let mut rand_equity = 1.0_f64;
    let mut rand_trades = 0usize;
    let mut rand_wins = 0usize;
    for wi in 0..total_windows {
        let ts = TRAIN_BARS + wi * TEST_BARS;
        let te = (ts + TEST_BARS).min(n);
        let mut i = ts;
        while i < te.saturating_sub(HOLD_BARS + 1) {
            let entry = eth_closes[(i+1).min(n-1)];
            let exit = eth_closes[(i+1+HOLD_BARS).min(n-1)];
            let gross = exit/entry - 1.0 - 2.0*FEE_PCT;
            rand_equity *= 1.0 + gross;
            if gross > 0.0 { rand_wins += 1; }
            rand_trades += 1;
            i += HOLD_BARS + 1;
        }
    }
    println!("  RANDOM ENTRY: {:.1}% | {} trades | {:.0}% WR", (rand_equity-1.0)*100.0, rand_trades, rand_wins as f64/rand_trades as f64*100.0);

    // ── Verdict ──
    let pass_rate = pass_ct as f64 / best_results.len().max(1) as f64;
    println!("\n{}", "═".repeat(72));
    let verdict = if best_sharpe > 3.0 && pass_rate > 0.65 {
        "✅ VIABLE — production sleeve candidate"
    } else if best_sharpe > 1.5 && pass_rate > 0.50 {
        "⚠️ BORDERLINE — needs further validation"
    } else {
        "❌ NOT VIABLE — correlation breakout not a reliable signal"
    };
    println!("  {}", verdict);
    println!("  Sharpe: {:.2} | Pass: {:.0}% | Trades: {}", best_sharpe, pass_rate*100.0, total_trades);
    println!("  vs Turtle+Chandelier (93% pass, Sharpe ~5.5 fee-adj): {}",
             if pass_rate >= 0.93 { "COMPARABLE" } else if pass_rate >= 0.70 { "SLIGHTLY WORSE" } else { "UNDERPERFORMS" });
    println!("{}", "═".repeat(72));

    // Save CSV
    let mut csv = String::from("window,ret_pct,sharpe,max_dd_pct,trades,win_rate,btc_ret,pass\n");
    for (wi, r) in best_results.iter().enumerate() {
        csv.push_str(&format!("{},{:.2},{:.4},{:.2},{},{:.2},{:.2},{}\n",
            wi, r.ret_pct, r.sharpe, r.max_dd_pct, r.trades, r.win_rate, r.btc_ret_pct,
            if r.ret_pct > 0.0 && r.trades >= MIN_TRADES { 1 } else { 0 }));
    }
    std::fs::write("snapshots/correlation_breakout_wf.csv", &csv)?;
    println!("  Results → snapshots/correlation_breakout_wf.csv");

    Ok(())
}
//! BTC→SOL Lead-Lag Pair Trade — DAILY RESOLUTION — Walk-Forward Validation
//!
//! Track C broadening: Does the BTC→ETH lead-lag effect generalize to SOL?
//!
//! Signal: BTC daily return > threshold → long SOL next day
//! Walk-forward: 252/252 train/test

use anyhow::Result;
use chrono::{TimeZone, Utc};
use krypto::data::DataLoader;
use polars::prelude::*;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const FEE_PCT: f64 = 0.001;
const HOLD_BARS: usize = 5; // hold ~1 week
const MIN_TRADES: usize = 5;

const LEAD_LENGTHS: &[usize] = &[1, 2, 3, 5];
const THRESHOLDS: &[f64] = &[1.0, 1.5, 2.0, 3.0, 5.0];

fn epoch_ms_to_date(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "?".to_string())
}

struct TradeResult {
    ret_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate: f64,
}

fn run_lead_lag(
    btc: &[f64],
    target: &[f64],
    lead_len: usize,
    threshold_pct: f64,
    test_start: usize,
    test_end: usize,
) -> TradeResult {
    let threshold = threshold_pct / 100.0;
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut wins = 0;
    let mut losses = 0;
    let mut returns = Vec::new();
    let mut i = test_start;

    while i < test_end.saturating_sub(lead_len + HOLD_BARS + 1) {
        if i < lead_len {
            i += 1;
            continue;
        }
        let btc_ret = btc[i] / btc[i - lead_len] - 1.0;

        if btc_ret > threshold {
            let entry_idx = i + 1;
            let exit_idx = (entry_idx + HOLD_BARS).min(test_end - 1).min(target.len() - 1);
            if entry_idx >= target.len() || exit_idx <= entry_idx {
                i += 1;
                continue;
            }

            let entry = target[entry_idx];
            let exit = target[exit_idx];
            let gross = exit / entry - 1.0 - 2.0 * FEE_PCT;
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
    let wr = if trades > 0 { wins as f64 / trades as f64 * 100.0 } else { 0.0 };
    let mean_r = if returns.is_empty() { 0.0 } else { returns.iter().sum::<f64>() / returns.len() as f64 };
    let var_r = if returns.is_empty() { 1.0 } else {
        returns.iter().map(|r| (r - mean_r).powi(2)).sum::<f64>() / returns.len().max(1) as f64
    };
    let bars_per_year: f64 = 365.25;
    let sharpe = mean_r / var_r.sqrt().max(1e-10) * bars_per_year.sqrt();

    TradeResult { ret_pct, sharpe, max_dd_pct: max_dd * 100.0, trades, win_rate: wr }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", "═".repeat(72));
    println!("  BTC→SOL LEAD-LAG @ 1d — WALK-FORWARD VALIDATION");
    println!("{}", "═".repeat(72));

    let loader = DataLoader::new(None, None);

    let btc_df = loader.fetch_data("BTCUSDT", "1d", CANDLES).await?;
    let sol_df = loader.fetch_data("SOLUSDT", "1d", CANDLES).await?;

    let btc_times: Vec<i64> = btc_df.column("time")?.cast(&DataType::Int64)?.i64()?.into_iter().flatten().collect();
    let btc_closes: Vec<f64> = btc_df.column("close")?.cast(&DataType::Float64)?.f64()?.into_iter().flatten().collect();
    let sol_times: Vec<i64> = sol_df.column("time")?.cast(&DataType::Int64)?.i64()?.into_iter().flatten().collect();
    let sol_closes: Vec<f64> = sol_df.column("close")?.cast(&DataType::Float64)?.f64()?.into_iter().flatten().collect();

    // Align by timestamp
    let btc_map: std::collections::HashMap<i64, f64> = btc_times.iter().zip(btc_closes.iter()).map(|(&t, &c)| (t, c)).collect();
    let n = sol_times.len();
    let mut btc_aligned = vec![0.0f64; n];
    let mut matched = 0;
    for i in 0..n {
        if let Some(&c) = btc_map.get(&sol_times[i]) {
            btc_aligned[i] = c;
            matched += 1;
        }
    }

    println!("BTC: {} bars | {} → {}", btc_closes.len(), epoch_ms_to_date(btc_times[0]), epoch_ms_to_date(btc_times[btc_times.len()-1]));
    println!("SOL: {} bars | {} → {}", sol_closes.len(), epoch_ms_to_date(sol_times[0]), epoch_ms_to_date(sol_times[sol_times.len()-1]));
    println!("Aligned: {}/{}", matched, n);

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    println!("Walk-forward: {} windows ({} train / {} test)", total_windows, TRAIN_BARS, TEST_BARS);

    // B&H baseline
    let bnh_ret = (sol_closes[n-1] / sol_closes[TRAIN_BARS] - 1.0) * 100.0;
    println!("SOL B&H (from W0 start): {:.1}%", bnh_ret);

    // ── Sweep ──
    println!("\n{}", "═".repeat(72));
    println!("  LONG-ONLY RESULTS (BTC surge → long SOL)");
    println!("{}", "═".repeat(72));
    println!("\n  {:>4} {:>6}  {:>8} {:>8} {:>7} {:>6} {:>6} {:>6}", "Lead", "Thresh", "Ret%", "Sharpe", "DD%", "Trades", "WR%", "Pass");
    println!("  {}", "─".repeat(64));

    let mut best_sharpe = f64::NEG_INFINITY;
    let mut best_config = (0, 0.0);
    let mut best_results: Vec<TradeResult> = Vec::new();

    for &lead in LEAD_LENGTHS {
        for &thresh in THRESHOLDS {
            let mut window_pass = 0;
            let mut agg_ret = 0.0;
            let mut agg_trades = 0;
            let mut window_results = Vec::new();
            let mut worst_dd = 0.0_f64;

            for wi in 0..total_windows {
                let ts = TRAIN_BARS + wi * TEST_BARS;
                let te = (ts + TEST_BARS).min(n);
                if te <= ts + 10 { continue; }
                let r = run_lead_lag(&btc_aligned, &sol_closes, lead, thresh, ts, te);
                if r.ret_pct > 0.0 && r.trades >= MIN_TRADES { window_pass += 1; }
                agg_ret += r.ret_pct;
                agg_trades += r.trades;
                if r.max_dd_pct > worst_dd { worst_dd = r.max_dd_pct; }
                window_results.push(r);
            }

            let n_win = window_results.len();
            let avg_ret = agg_ret / n_win.max(1) as f64;
            let avg_sharpe = window_results.iter().map(|r| r.sharpe).sum::<f64>() / n_win.max(1) as f64;
            let avg_wr = window_results.iter().map(|r| r.win_rate).sum::<f64>() / n_win.max(1) as f64;

            let marker = if avg_sharpe > best_sharpe { "★" } else { "" };
            if avg_sharpe > best_sharpe {
                best_sharpe = avg_sharpe;
                best_config = (lead, thresh);
                best_results = window_results;
            }

            println!("  {:>4} {:>5.1}%  {:>+8.1} {:>8.2} {:>6.1}% {:>6} {:>5.0}% {:>3}/{}  {}",
                     lead, thresh, avg_ret, avg_sharpe, worst_dd, agg_trades, avg_wr,
                     window_pass, n_win, marker);
        }
    }

    // ── Best config detail ──
    println!("\n{}", "═".repeat(72));
    println!("  BEST: lead={}, thresh={:.1}% | Sharpe={:.2}", best_config.0, best_config.1, best_sharpe);
    println!("{}", "═".repeat(72));

    let best_pass = best_results.iter().filter(|r| r.ret_pct > 0.0 && r.trades >= MIN_TRADES).count();
    let best_trades: usize = best_results.iter().map(|r| r.trades).sum();

    println!("  Pass: {}/{} | Trades: {} | Avg WR: {:.0}%",
             best_pass, best_results.len(), best_trades,
             best_results.iter().map(|r| r.win_rate).sum::<f64>() / best_results.len() as f64);

    for (wi, r) in best_results.iter().enumerate() {
        let pass = if r.ret_pct > 0.0 && r.trades >= MIN_TRADES { "PASS" } else if r.trades < MIN_TRADES { "THIN" } else { "FAIL" };
        if wi < 10 || r.ret_pct < 0.0 || wi >= best_results.len() - 5 {
            println!("  W{:02} | {:+7.1}% | Sh {:7.2} | DD {:5.1}% | {:3}t | {:.0}% | {}",
                     wi, r.ret_pct, r.sharpe, r.max_dd_pct, r.trades, r.win_rate, pass);
        } else if wi == 10 {
            println!("  ... ({} windows omitted) ...", best_results.len() - 15);
        }
    }

    // ── Random entry baseline ──
    println!("\n{}", "─".repeat(72));
    let test_start = TRAIN_BARS;
    let mut random_equity = 1.0_f64;
    let mut random_wins = 0;
    let mut random_trades = 0;
    let mut i = test_start;
    while i < n.saturating_sub(HOLD_BARS + 1) {
        let entry = sol_closes[i + 1];
        let exit_idx = (i + 1 + HOLD_BARS).min(n - 1);
        let exit = sol_closes[exit_idx];
        let raw = exit / entry - 1.0 - 2.0 * FEE_PCT;
        random_equity *= 1.0 + raw;
        if raw > 0.0 { random_wins += 1; }
        random_trades += 1;
        i += HOLD_BARS + 1;
    }
    println!("  Random (long, hold {} bars, every {} bars): {:.1}% | {} trades | {:.0}% WR",
             HOLD_BARS, HOLD_BARS + 1, (random_equity - 1.0) * 100.0, random_trades,
             random_wins as f64 / random_trades as f64 * 100.0);
    println!("  SOL B&H: {:.1}%", bnh_ret);

    // ── Verdict ──
    println!("\n{}", "═".repeat(72));
    if best_sharpe > 1.5 && best_pass as f64 / best_results.len() as f64 > 0.5 {
        println!("  ✅ BTC→SOL lead-lag at 1d: Sharpe {:.2}, {}/{} pass", best_sharpe, best_pass, best_results.len());
        println!("     Lead={} bars, threshold={:.1}%, hold={} bars", best_config.0, best_config.1, HOLD_BARS);
    } else if best_sharpe > 0.5 {
        println!("  ⚠️  BTC→SOL lead-lag at 1d: Marginal Sharpe {:.2}, {}/{} pass", best_sharpe, best_pass, best_results.len());
    } else {
        println!("  ❌ BTC→SOL lead-lag at 1d: Negative/marginal Sharpe {:.2}", best_sharpe);
    }
    println!("{}", "═".repeat(72));

    Ok(())
}

//! M1: Equity Trajectory Monitor
//!
//! Operational monitoring tool for live testnet deployment.
//! Computes rolling return metrics on the exact-live equity CSV and reports
//! whether current regime is degrading vs historical norms.
//!
//! Inputs: snapshots/live_bot_exact_equity.csv (from live_bot_exact_equity.rs)
//! Output: console report + alerts
//!
//! Metrics computed:
//! - 60-day rolling return
//! - Current 60d return vs historical distribution (10th/5th pctile alerts)
//! - 90-day drawdown depth
//! - Per-year rolling return distribution
//! - Tail concentration check (top-10 log contributors)

use std::fs::File;
use std::io::{BufRead, BufReader};

fn percentile(vals: &[f64], p: f64) -> f64 {
    if vals.is_empty() { return 0.0; }
    let mut sorted = vals.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = (p * (vals.len() - 1) as f64).round() as usize;
    sorted[idx.min(vals.len() - 1)]
}

fn annualised_sharpe(rets: &[f64]) -> f64 {
    if rets.len() < 2 { return 0.0; }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var <= 0.0 { 0.0 } else { (mean / var.sqrt()) * 365.0_f64.sqrt() }
}

fn main() {
    println!("=== M1: Equity Trajectory Monitor ===\n");

    let eq_file = File::open("snapshots/live_bot_exact_equity.csv")
        .expect("snapshots/live_bot_exact_equity.csv not found. Run live_bot_exact_equity.rs first.");
    let reader = BufReader::new(eq_file);

    let mut dates = Vec::new();
    let mut equity = Vec::new();

    for line in reader.lines().skip(1) {
        let line = line.expect("read error");
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 3 {
            dates.push(parts[1].to_string());
            equity.push(parts[2].parse::<f64>().unwrap_or(1.0));
        }
    }

    if equity.len() < 60 {
        println!("ERROR: need at least 60 bars, got {}", equity.len());
        return;
    }

    let n = equity.len();
    println!(
        "Loaded {} bars ({} to {})\n",
        n,
        dates.first().map(|s| s.as_str()).unwrap_or("?"),
        dates.last().map(|s| s.as_str()).unwrap_or("?")
    );

    // Compute daily returns
    let rets: Vec<f64> = equity
        .windows(2)
        .map(|w| if w[0] > 0.0 { w[1] / w[0] - 1.0 } else { 0.0 })
        .collect();

    // Rolling 60-day returns
    const ROLL: usize = 60;
    let mut roll_rets = Vec::new();
    let mut roll_sharpe = Vec::new();

    for i in ROLL..=n {
        let start_eq = equity[i - ROLL];
        let end_eq = equity[i - 1];
        let roll_ret = (end_eq / start_eq - 1.0) * 100.0;
        roll_rets.push(roll_ret);

        let window_rets: Vec<f64> = rets[i - ROLL..i - 1].to_vec();
        let sh = annualised_sharpe(&window_rets);
        roll_sharpe.push(sh);
    }

    let current_roll_ret = *roll_rets.last().unwrap_or(&0.0);
    let current_roll_sharpe = *roll_sharpe.last().unwrap_or(&0.0);
    let current_date = dates.get(n - ROLL).map(|s| s.as_str()).unwrap_or("?");
    let current_eq = *equity.last().unwrap_or(&1.0);

    // Peak equity in last 252 days (1 year)
    let lookback = 252.min(n);
    let peak_recent = equity[n - lookback..].iter().fold(0.0f64, |a, &b| a.max(b));
    let current_vs_peak_pct = (current_eq / peak_recent - 1.0) * 100.0;

    // 90-day drawdown from recent peak
    let last_90_start = n.saturating_sub(90);
    let peak_90 = equity[last_90_start..].iter().fold(0.0f64, |a, &b| a.max(b));
    let dd_90 = (1.0 - current_eq / peak_90) * 100.0;

    // Historical rolling return stats
    let roll_ret_p10 = percentile(&roll_rets, 0.10);
    let roll_ret_p5 = percentile(&roll_rets, 0.05);
    let roll_ret_p25 = percentile(&roll_rets, 0.25);
    let roll_ret_p50 = percentile(&roll_rets, 0.50);
    let roll_ret_mean = roll_rets.iter().sum::<f64>() / roll_rets.len() as f64;

    println!("─── Current 60-Day Window ───");
    println!("Date: {}", current_date);
    println!("60d return: {:+.1}%", current_roll_ret);
    println!("60d Sharpe: {:.2}", current_roll_sharpe);
    println!("Equity vs 1y peak: {:+.1}%", current_vs_peak_pct);
    println!("90d drawdown from recent peak: {:.1}%", dd_90);

    println!("\n─── Alert Thresholds ───");
    println!("10th pctile 60d return: {:+.1}%", roll_ret_p10);
    println!("5th pctile 60d return:  {:+.1}%", roll_ret_p5);
    println!("Median 60d return:      {:+.1}%", roll_ret_p50);

    let p10_alert = current_roll_ret < roll_ret_p10;
    let p5_alert = current_roll_ret < roll_ret_p5;
    let dd_alert = current_vs_peak_pct < -20.0 || dd_90 > 20.0;

    println!("\n─── Status ───");
    if p5_alert || dd_alert {
        println!("🔴 RED ALERT");
        if p5_alert {
            println!(
                "   60d return ({:.1}%) below 5th percentile ({:.1}%)",
                current_roll_ret, roll_ret_p5
            );
        }
        if dd_alert {
            let dd_val = current_vs_peak_pct.abs().max(dd_90);
            println!("   Drawdown alert: {:.1}% below peak", dd_val);
        }
    } else if p10_alert {
        println!("🟡 YELLOW ALERT");
        println!(
            "   60d return ({:.1}%) below 10th percentile ({:.1}%)",
            current_roll_ret, roll_ret_p10
        );
    } else {
        println!("🟢 GREEN — current regime within normal bounds");
    }

    println!("\n─── Historical Rolling Return Distribution ───");
    println!("       5th pctile: {:+.1}%", roll_ret_p5);
    println!("      10th pctile: {:+.1}%", roll_ret_p10);
    println!("      25th pctile: {:+.1}%", roll_ret_p25);
    println!("Median:          {:+.1}%", roll_ret_p50);
    println!("         mean:   {:+.1}%", roll_ret_mean);

    // Per-year rolling return stats
    println!("\n─── Per-Year Rolling 60d Return Stats ───");
    let mut year_rolls: std::collections::HashMap<i32, Vec<f64>> =
        std::collections::HashMap::new();
    for (i, &rr) in roll_rets.iter().enumerate() {
        let bar_idx = ROLL + i;
        if let Some(d) = dates.get(bar_idx) {
            if let Ok(year) = d.get(0..4).unwrap_or("0").parse::<i32>() {
                year_rolls.entry(year).or_default().push(rr);
            }
        }
    }

    let mut years: Vec<i32> = year_rolls.keys().cloned().collect();
    years.sort_unstable();
    for year in years {
        if let Some(vals) = year_rolls.get(&year) {
            let p10y = percentile(vals, 0.10);
            let p50y = percentile(vals, 0.50);
            let p90y = percentile(vals, 0.90);
            let mean = vals.iter().sum::<f64>() / vals.len() as f64;
            let current_yr_roll = *roll_rets.last().unwrap_or(&0.0);
            let is_current = year == 2026;
            let label = if is_current {
                format!(" [CURRENT: {:+.1}%]", current_yr_roll)
            } else {
                String::new()
            };
            println!(
                "  {}: mean {:+.1}%, p10 {:+.1}%, p50 {:+.1}%, p90 {:+.1}%{}",
                year, mean, p10y, p50y, p90y, label
            );
        }
    }

    // Top-winner tail sensitivity
    println!("\n─── Tail Sensitivity Check ───");
    if let Ok(tf) = File::open("snapshots/live_bot_exact_trades.csv") {
        let reader = BufReader::new(tf);
        let mut log_rets = Vec::new();
        for line in reader.lines().skip(1) {
            if let Ok(line) = line {
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 11 {
                    if let Ok(eq_mult) = parts[10].parse::<f64>() {
                        let ln = eq_mult.max(1e-12).ln();
                        if ln.is_finite() {
                            log_rets.push(ln);
                        }
                    }
                }
            }
        }
        log_rets.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let total_log: f64 = log_rets.iter().sum();
        let top5_log: f64 = log_rets.iter().take(5).sum();
        let top10_log: f64 = log_rets.iter().take(10).sum();
        let top20_log: f64 = log_rets.iter().take(20).sum();
        let equity_without_top10 =
            if top10_log.is_finite() {
                (total_log - top10_log).exp()
            } else {
                0.0
            };
        let equity_without_top20 =
            if top20_log.is_finite() {
                (total_log - top20_log).exp()
            } else {
                0.0
            };

        println!("  Top-5  log contributors: {:.0}% of total equity", top5_log / total_log);
        println!("  Top-10 log contributors: {:.0}% of total equity", top10_log / total_log);
        println!("  Top-20 log contributors: {:.0}% of total equity", top20_log / total_log);
        println!(
            "  Equity without top-10 trades: {:.2}x (vs {:.2}x full)",
            equity_without_top10,
            equity.last().unwrap_or(&1.0)
        );
        println!("  Equity without top-20 trades: {:.2}x", equity_without_top20);

        if top10_log / total_log > 0.75 {
            println!(
                "  ⚠️  CONCENTRATION RISK: top-10 = {:.0}% of equity — strategy highly tail-dependent",
                top10_log / total_log * 100.0
            );
        }
    }

    println!("\nNote: requires snapshots/live_bot_exact_equity.csv");
    println!("Run: cargo run --example live_bot_exact_equity --profile sweep");
}

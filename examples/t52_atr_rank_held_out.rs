//! T52: ATR_RANK=24 Held-Out Validation
//!
//! PURPOSE: Validate ATR_RANK=24 is not a same-harness artifact (EP=24 pattern).
//!
//! EP=24 failed held-out: found as 3rd sequential optimization on live_compatible_wf.rs
//! (after LB=42 and AP=12), then failed pre-2021 validation.
//! ATR_RANK=24 has the SAME pattern: 3rd sequential optimization on live_compatible_wf.rs.
//!
//! METHOD: Use pre-2021 data ONLY for validation (never seen during T=24 optimization).
//! The T=24 winner was found using post-2021 OOS windows (7 windows spanning 2021-2026).
//!
//! Compare: T=0 (no filter) vs T=5 vs T=24 vs T=50 on pre-2021 held-out data.
//! If T=24 wins → promote to production default.
//! If T=24 loses or is indistinguishable from T=5 → revert to T=5.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

// Regime ATR params (confirmed in prior hyperopts)
const REGIME_ATR_PERIOD: usize = 12;
const REGIME_LOOKBACK: usize = 42;

// ATR rank thresholds to test
const ATR_RANK_VALUES: &[f64] = &[0.0, 5.0, 24.0, 50.0, 80.0];

const VOL_LOOKBACK: usize = 96;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
    "LTCUSDT", "EOSUSDT", "BNBUSDT", "BCHUSDT",
];

// Pre-2021 held-out periods
const HELD_OUT_PERIODS: &[(&str, i64, i64)] = &[
    ("P1-2019 (Pre-COVID bear/chop)",    1546300800, 1580515200), // 2019-01-01 to 2020-01-01
    ("P2-2020 (COVID crash/recovery)",    1577836800, 1609459200), // 2020-01-01 to 2021-01-01
];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
    times: Vec<i64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        if i == 0 { continue; }
        let h = high[i];
        let l = low[i];
        let c0 = close[i - 1];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    vals[idx.saturating_sub(window - 1)..=idx].iter().sum::<f64>() / window as f64
}

fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    let mut hist = Vec::with_capacity(lookback);
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = 1.0;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
    daily_rets: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_rank_t: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };

        if atr_rank_t > 0.0 && btc_pct < atr_rank_t {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Dollar-volume ranking
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                if price > 0.0 { scores.push((sym.as_str(), dv)); }
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let selected: Vec<&str> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect();

        // Entry signals
        for sym in &selected {
            if let Some(sd) = sym_data.get(*sym) {
                if bar < TURTLE_ENTRY || bar >= sd.close.len() { continue; }
                let start = bar - TURTLE_ENTRY;
                let max_close = sd.close[start..bar].iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if sd.close[bar] >= max_close {
                    let entry_px = sd.close[bar] * (1.0 + TAKER_FEE);
                    let atr = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar);
                    let stop = if atr > 0.0 { sd.high[bar] - TURTLE_ATR_MULT * atr } else { sd.close[bar] * 0.98 };

                    let mut pos_bar = bar;
                    let mut exited = false;
                    let mut exit_px = 0.0;

                    while pos_bar < test_end && pos_bar - bar < HOLD_MAX {
                        if sd.low[pos_bar] <= stop {
                            exit_px = stop;
                            exited = true;
                            break;
                        }
                        pos_bar += 1;
                    }

                    if !exited {
                        exit_px = sd.close[bar + (HOLD_MAX.min(test_end - bar - 1))];
                    }

                    let exit_fee = exit_px * (1.0 - TAKER_FEE);
                    let ret = exit_fee / entry_px - 1.0;
                    equity *= 1.0 + ret;
                    if equity > peak { peak = equity; }
                    total_trades += 1;

                    let daily_ret = ret / HOLD_MAX.max(1) as f64;
                    daily_rets.push(daily_ret);
                }
            }
        }

        equity_curve.push(equity);
        bar += 1;
    }

    WfResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
        daily_rets,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("T52: ATR_RANK Held-Out Validation");
    println!("==================================");
    println!("Testing: T ∈ {{0, 5, 24, 50, 80}} on PRE-2021 data only");
    println!("T=24 was found on post-2021 OOS windows. Pre-2021 is truly held-out.");
    println!("");

    println!("Loading data for {} symbols...", SYMBOLS.len());
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();

    for &sym in SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let time_col = df.column("time")?.datetime()?;
        let close_col = df.column("close")?.f64()?;
        let high_col = df.column("high")?.f64()?;
        let low_col = df.column("low")?.f64()?;
        let vol_col = df.column("volume")?.f64()?;

        let times: Vec<i64> = time_col.into_no_null_iter().collect();
        let close: Vec<f64> = close_col.into_no_null_iter().collect();
        let high: Vec<f64> = high_col.into_no_null_iter().collect();
        let low: Vec<f64> = low_col.into_no_null_iter().collect();
        let vol: Vec<f64> = vol_col.into_no_null_iter().collect();

        sym_data.insert(sym.to_string(), SymData { close, high, low, vol, times });
    }

    // Verify we have pre-2021 data
    let btc_times = &sym_data.get("BTCUSDT").unwrap().times;
    let earliest_ts = btc_times.first().copied().unwrap_or(0);
    let latest_ts = btc_times.last().copied().unwrap_or(0);
    println!("Data range: {} to {}", earliest_ts, latest_ts);
    println!("Pre-2021 cutoff: 1577836800");
    println!("");

    // Test each ATR_RANK value across each held-out period
    let mut results: Vec<(f64, String, WfResult)> = Vec::new();

    for &atr_t in ATR_RANK_VALUES {
        let t_label = if atr_t == 0.0 { "T=0 (no filter)".to_string() } else { format!("T={:.0}", atr_t) };

        for (period_name, start_ts, end_ts) in HELD_OUT_PERIODS {
            let syms: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();

            // Find bar indices for this period
            let btc_times = &sym_data.get("BTCUSDT").unwrap().times;
            let start_bar = btc_times.iter().position(|&t| t >= *start_ts).unwrap_or(0);
            let end_bar = btc_times.iter().rposition(|&t| t <= *end_ts).unwrap_or(btc_times.len() - 1);

            if end_bar <= start_bar + 50 {
                println!("Skipping {} — insufficient bars", period_name);
                continue;
            }

            // Walk-forward within the held-out period
            let window_bars = 252;
            let n_windows = (end_bar.saturating_sub(start_bar)) / window_bars;

            for w in 0..n_windows {
                let test_start = start_bar + w * window_bars;
                let test_end = (test_start + window_bars).min(end_bar);

                if test_end > btc_times.len() { break; }

                let res = run_sim(&sym_data, &syms, test_start, test_end, atr_t);
                results.push((atr_t, format!("{} W{}", period_name, w), res));
            }
        }
    }

    // Aggregate by ATR_RANK threshold
    println!("\n=== RESULTS BY ATR_RANK THRESHOLD ===");
    println!("{:<20} {:>6} {:>8} {:>10} {:>8} {:>8}", "Config", "Pass", "Sharpe", "Return%", "DD%", "Trades");
    println!("{}", "-".repeat(65));

    let mut summary: Vec<(f64, usize, usize, f64, f64, f64, usize)> = Vec::new();

    for &atr_t in ATR_RANK_VALUES {
        let t_label = if atr_t == 0.0 { "T=0 (no filter)" } else { &format!("T={:.0}", atr_t) };

        let matching: Vec<_> = results.iter().filter(|(t, _, _)| (*t - atr_t).abs() < 0.5).collect();
        let total = matching.len();
        let passes = matching.iter().filter(|(_, _, r)| r.trades >= MIN_TRADES && r.sharpe > 0.0).count();
        let avg_sharpe = matching.iter().map(|(_, _, r)| r.sharpe).sum::<f64>() / total as f64;
        let avg_ret = matching.iter().map(|(_, _, r)| (r.equity - 1.0) * 100.0).sum::<f64>() / total as f64;
        let avg_dd = matching.iter().map(|(_, _, r)| r.dd).sum::<f64>() / total as f64;
        let total_trades: usize = matching.iter().map(|(_, _, r)| r.trades).sum();

        println!("{:<20} {:>6} {:>8.3} {:>10.1}% {:>8.1}% {:>8}",
            t_label, passes, avg_sharpe, avg_ret, avg_dd, total_trades);

        summary.push((atr_t, passes, total, avg_sharpe, avg_ret, avg_dd, total_trades));
    }

    // Per-period breakdown for T=0 vs T=24
    println!("\n=== T=0 vs T=24 PER-HELD-OUT-PERIOD ===");
    for (period_name, start_ts, end_ts) in HELD_OUT_PERIODS {
        println!("\n{}", period_name);
        for &atr_t in &[0.0, 24.0] {
            let t_label = if atr_t == 0.0 { "T=0 (no filter)" } else { &format!("T={:.0}", atr_t) };
            let matching: Vec<_> = results.iter()
                .filter(|(t, lbl, _)| (*t - atr_t).abs() < 0.5 && lbl.contains(period_name.split(' ').next().unwrap_or("")))
                .collect();

            if matching.is_empty() { continue; }
            let total = matching.len();
            let passes = matching.iter().filter(|(_, _, r)| r.trades >= MIN_TRADES && r.sharpe > 0.0).count();
            let avg_sharpe = matching.iter().map(|(_, _, r)| r.sharpe).sum::<f64>() / total as f64;
            let avg_ret = matching.iter().map(|(_, _, r)| (r.equity - 1.0) * 100.0).sum::<f64>() / total as f64;
            let total_trades: usize = matching.iter().map(|(_, _, r)| r.trades).sum();
            println!("  {:<18}: {}/{} pass, Sharpe {:.3}, Return {:+.1}%, {} trades",
                t_label, passes, total, avg_sharpe, avg_ret, total_trades);
        }
    }

    // Write CSV
    {
        let path = "snapshots/t52_atr_rank_held_out.csv";
        let mut f = std::fs::File::create(path)?;
        use std::io::Write;
        writeln!(f, "atr_t,period_label,equity,sharpe,dd_pct,trades")?;
        for (atr_t, lbl, r) in &results {
            writeln!(f, "{:.0},{},{:.6},{:.4},{:.2},{}", atr_t, lbl, r.equity, r.sharpe, r.dd, r.trades)?;
        }
        println!("\nResults: {}", path);
    }

    // Final verdict
    println!("\n=== VERDICT ===");
    let t5_res = summary.iter().find(|(t, _, _, _, _, _, _)| (*t - 5.0).abs() < 0.5).copied();
    let t24_res = summary.iter().find(|(t, _, _, _, _, _, _)| (*t - 24.0).abs() < 0.5).copied();
    let t0_res = summary.iter().find(|(t, _, _, _, _, _, _)| (*t - 0.0).abs() < 0.5).copied();

    if let Some((_, p5, tot5, sh5, _, _, _)) = t5_res {
        if let Some((_, p24, tot24, sh24, _, _, _)) = t24_res {
            let delta_pass = p24 as isize - p5 as isize;
            let delta_sharpe = sh24 - sh5;
            println!("T=24 vs T=5: {} more passes, Sharpe {:+.3}", delta_pass, delta_sharpe);
            if delta_pass >= 0 && delta_sharpe >= 0.3 {
                println!("✅ ATR_RANK=24 CONFIRMED — promote to production default");
            } else if delta_pass > 0 && delta_sharpe >= 0.0 {
                println!("⚠️ ATR_RANK=24 MARGINAL — improvement in passes but marginal Sharpe");
                println!("   Keep T=5 as stable default pending further validation");
            } else {
                println!("❌ ATR_RANK=24 FAILS held-out — revert to T=5.0");
            }
        }
    }

    if let Some((_, p0, _, sh0, _, _, _)) = t0_res {
        if let Some((_, p24, _, sh24, _, _, _)) = t24_res {
            println!("T=24 vs T=0 (no filter): {} vs {} passes, Sharpe {:+.3}",
                p24, p0, sh24 - sh0);
        }
    }

    println!("\nReference: EP=24 failed held-out with 25/29 vs 27/29 passes.");
    println!("ATR_RANK=24 must beat T=5 on BOTH pass count AND Sharpe to be promoted.");

    Ok(())
}
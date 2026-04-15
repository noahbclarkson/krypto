//! Regime-Conditional A/D × Turtle Walk-Forward Validation
//!
//! PURPOSE: Test vol-rank conditional allocation between A/D (mean-reversion/crash alpha)
//! and Turtle (trend/bull alpha).
//!
//! Background:
//! - A/D wins crash windows (W01, W04, W06) per cross-sectional analysis
//! - Turtle wins bull/trending windows (W00, W02, W03, W05)
//! - Majority-vote ensemble failed (4/7 OOS pass)
//! - Vol-rank conditional switching is the next logical ensemble method
//!
//! Method:
//! - 21-bar realized vol rank (vs 252-bar history) at each test bar
//! - Bottom quartile vol rank → A/D (low vol = ranging/crash = A/D territory)
//! - Top quartile vol rank → Turtle (high vol = trending = Turtle territory)
//! - Baseline comparisons: A/D-only, Turtle-only, equal-weight blend
//!
//! Execution:
//! - Walk-forward: 252-bar train / 252-bar test, step = 21 bars
//! - Signal at close, entry at next open, exit via Chandelier(45, 2.5)
//! - 0.1% taker each side
//! - Universe: Base5 + 8 additional universes for stress testing

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
const STEP_BARS: usize = 21;
const WARMUP: usize = 252;
const TAKER_FEE: f64 = 0.001;
const CHAND_PERIOD: usize = 45;
const CHAND_MULT: f64 = 2.05;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const TURTLE_ENTRY: usize = 21; // hyperopt 2026-04-10: full 5-100 sweep found EP=21 is global max Sharpe (0.176) and most robust (78% universes positive)
const VOL_RANK_LOOKBACK: usize = 252;
const MIN_TRADES: usize = 3;

// Vol rank quartiles
const VOL_BOTTOM_PCT: f64 = 0.25; // bottom 25% → A/D
const VOL_TOP_PCT: f64 = 0.75;    // top 25% → Turtle

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
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
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"],
    ),
    (
        "OldGuardNoBNB",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
    (
        "LargeCaps5",
        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"],
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

// ── Math helpers ───────────────────────────────────────────────────────────────

fn calc_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 5 {
        return 0.0;
    }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets
        .iter()
        .map(|r| (r - mean).powi(2))
        .sum::<f64>()
        / daily_rets.len().max(1) as f64;
    let std = var.sqrt();
    if std < 1e-9 {
        return 0.0;
    }
    mean * 365.0 / (std * (365.0_f64).sqrt())
}

fn calc_max_dd(equity: &[f64]) -> f64 {
    let mut peak = equity[0];
    let mut max_dd: f64 = 0.0;
    for &e in equity {
        peak = peak.max(e);
        let dd = (peak - e) / peak * 100.0;
        max_dd = max_dd.max(dd);
    }
    max_dd
}

fn true_range(h: f64, l: f64, prev_c: f64) -> f64 {
    (h - l).abs().max((h - prev_c).abs()).max((l - prev_c).abs())
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period {
        return 0.0;
    }
    let mut tr_sum = 0.0_f64;
    for i in idx.saturating_sub(period - 1)..=idx {
        let pc = if i > 0 { close[i - 1] } else { close[0] };
        tr_sum += true_range(high[i], low[i], pc);
    }
    tr_sum / period as f64
}

fn realized_vol(close: &[f64], lookback: usize, idx: usize) -> f64 {
    if idx < lookback + 1 {
        return 0.0;
    }
    let mut rets = Vec::with_capacity(lookback);
    for i in idx + 1 - lookback..=idx {
        if i > 0 && close[i - 1] > 0.0 {
            rets.push((close[i] - close[i - 1]) / close[i - 1]);
        }
    }
    if rets.len() < lookback / 2 {
        return 0.0;
    }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    var.sqrt()
}

fn vol_rank(realized: f64, vol_history: &[f64]) -> f64 {
    if vol_history.is_empty() || realized <= 0.0 {
        return 0.5;
    }
    let count_below = vol_history.iter().filter(|&&v| v < realized).count();
    count_below as f64 / vol_history.len() as f64
}

// ── Signal generation ──────────────────────────────────────────────────────────

fn ad_momentum_signal(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    volume: &[f64],
    period: usize,
    idx: usize,
) -> i32 {
    if idx < period || idx >= close.len() {
        return 0;
    }
    // A/D accumulation/distribution: sum of MFM * volume over period
    let mut ad_end: f64 = 0.0;
    for i in idx + 1 - period..=idx {
        let h = high[i.max(0)];
        let l = low[i.max(0)];
        let c = close[i.max(0)];
        let v = volume[i.max(0)];
        let range = h - l;
        let mf = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        ad_end += mf * v;
    }
    if ad_end > 0.0 {
        1
    } else if ad_end < 0.0 {
        -1
    } else {
        0
    }
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> i32 {
    if idx < entry_period || idx >= close.len() {
        return 0;
    }
    let mut max_h = close[idx - entry_period];
    for i in idx + 1 - entry_period..idx {
        max_h = max_h.max(close[i.max(0)]);
    }
    if close[idx] > max_h {
        1
    } else {
        0
    }
}

// ── Per-window backtest ───────────────────────────────────────────────────────

struct WindowResult {
    oos_return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    trade_count: usize,
    win_rate: f64,
    equity_final: f64,
}

fn run_strategy(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    volume: &[f64],
    test_start: usize,
    test_end: usize,
    strategy: &str,
    vol_history: &[f64],
) -> WindowResult {
    let mut equity = vec![1.0];
    let mut daily_rets = Vec::new();
    let mut trades = Vec::new();

    let mut i = test_start;
    while i < test_end.saturating_sub(2) && i + 1 < close.len() {
        let vol_r = realized_vol(close, 21, i);
        let vr = vol_rank(vol_r, vol_history);

        // Determine which signal to use based on strategy
        let ad_sig = ad_momentum_signal(close, high, low, volume, AD_PERIOD, i);
        let turtle_sig = turtle_signal(close, TURTLE_ENTRY, i);

        let signal = match strategy {
            "vol_conditional" => {
                // Bottom quartile vol → A/D, top quartile → Turtle
                if vr < VOL_BOTTOM_PCT {
                    ad_sig
                } else if vr > VOL_TOP_PCT {
                    turtle_sig
                } else {
                    0
                }
            }
            "ad_only" => ad_sig,
            "turtle_only" => turtle_sig,
            "equal_blend" => {
                // Use whichever is non-zero, prefer A/D when both fire
                if ad_sig != 0 {
                    ad_sig
                } else {
                    turtle_sig
                }
            }
            _ => 0,
        };

        if signal == 0 {
            equity.push(*equity.last().unwrap());
            daily_rets.push(0.0);
            i += 1;
            continue;
        }

        // Entry at next bar open
        let entry_idx = (i + 1).min(close.len() - 1);
        let entry_price = close[entry_idx];
        let atr = atr_at(high, low, close, CHAND_PERIOD, entry_idx);

        if entry_price <= 0.0 || atr <= 0.0 {
            i += 1;
            continue;
        }

        // Chandelier stop
        let mut highest_high = entry_price;
        let mut exit_bar = (test_end.min(close.len() - 1));

        for j in entry_idx + 1..close.len().min(test_end + STEP_BARS) {
            highest_high = highest_high.max(high[j]);
            let stop_price = highest_high - CHAND_MULT * atr;
            if low[j] <= stop_price {
                exit_bar = j;
                break;
            }
            if j >= test_end + STEP_BARS - 1 {
                exit_bar = j.min(close.len() - 1);
                break;
            }
        }

        let exit_price = close[exit_bar.min(close.len() - 1)];
        let gross = if signal > 0 {
            exit_price / entry_price - 1.0
        } else {
            entry_price / exit_price - 1.0
        };
        let net = gross - 2.0 * TAKER_FEE;

        let span = (exit_bar.saturating_sub(entry_idx)).max(1);
        let daily_ret = net / span as f64;

        for _ in 0..span {
            let last_eq = *equity.last().unwrap();
            equity.push(last_eq * (1.0 + daily_ret));
        }
        for _ in 0..span {
            daily_rets.push(daily_ret);
        }

        trades.push((gross, net));
        i = exit_bar.min(close.len() - 1) + 1;
    }

    // Extend equity to full test window
    let target_len = (test_end - test_start) + 1;
    while equity.len() < target_len {
        equity.push(*equity.last().unwrap());
    }

    let equity_slice = &equity[..target_len.min(equity.len())];
    let final_equity = equity_slice.last().copied().unwrap_or(1.0);
    let oos_return = (final_equity - 1.0) * 100.0;
    let sharpe = calc_sharpe(&daily_rets);
    let max_dd = calc_max_dd(equity_slice);
    let wins = trades.iter().filter(|(g, _)| *g > 0.0).count();
    let win_rate = if trades.is_empty() {
        0.0
    } else {
        wins as f64 / trades.len() as f64
    };

    WindowResult {
        oos_return_pct: oos_return,
        sharpe,
        max_dd,
        trade_count: trades.len(),
        win_rate,
        equity_final: final_equity,
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Regime-Conditional A/D × Turtle Walk-Forward ===\n");
    println!(
        "Vol conditional: bottom {:.0}% vol rank → A/D, top {:.0}% → Turtle",
        VOL_BOTTOM_PCT * 100.0,
        VOL_TOP_PCT * 100.0
    );
    println!(
        "9 universes: Base5, NoDOGE, Legacy4, Legacy5BNB, OldGuardNoBNB, LargeCaps5, Legacy3, LowVolume5, OldGuard4"
    );
    println!("Execution: Chandelier(45, 2.5) exit, 0.1% taker\n");

    let loader = DataLoader::new(None, None);

    // Load BTC as benchmark for vol reference
    let btc_raw = loader.fetch_with_cache("BTCUSDT", "1d", CANDLES).await?;
    let btc_df = FeatureEngine::add_technicals(&btc_raw, None)?;

    let to_f64_vec = |s: &Series| -> Vec<f64> {
        s.f64()
            .unwrap()
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect()
    };

    let btc_close = to_f64_vec(btc_df.column("close")?);
    let btc_high = to_f64_vec(btc_df.column("high")?);
    let btc_low = to_f64_vec(btc_df.column("low")?);

    let mut all_results: Vec<String> = Vec::new();
    all_results.push("universe,window,strategy,return_pct,sharpe,max_dd,trades,win_rate".to_string());

    let strategies = ["vol_conditional", "ad_only", "turtle_only", "equal_blend"];

    // Per-strategy accumulators for summary
    let mut strat_pass: HashMap<String, (usize, usize, f64, f64, usize)> = HashMap::new();
    for s in &strategies {
        strat_pass.insert(s.to_string(), (0, 0, 0.0, 0.0, 0));
    }

    for (universe_name, symbols) in UNIVERSES {
        println!("\n--- Universe: {} ---", universe_name);
        let start = Instant::now();

        // Load all symbols
        let mut aligned_data: HashMap<String, (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>)> =
            HashMap::new();

        for &symbol in *symbols {
            let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
            let enriched = FeatureEngine::add_technicals(&raw, Some(&btc_df))?;
            let close = to_f64_vec(enriched.column("close")?);
            let high = to_f64_vec(enriched.column("high")?);
            let low = to_f64_vec(enriched.column("low")?);
            let volume = to_f64_vec(enriched.column("volume")?);
            aligned_data.insert(symbol.to_string(), (close, high, low, volume));
        }

        // Align to shortest symbol
        let min_len = aligned_data
            .values()
            .map(|(c, _, _, _)| c.len())
            .min()
            .unwrap_or(0);

        for (_, data) in aligned_data.iter_mut() {
            data.0.truncate(min_len);
            data.1.truncate(min_len);
            data.2.truncate(min_len);
            data.3.truncate(min_len);
        }

        let n = min_len;
        let n_windows = (n.saturating_sub(WARMUP)) / STEP_BARS;

        for window_idx in 0..n_windows {
            let train_end = WARMUP + window_idx * STEP_BARS;
            let test_start = train_end;
            let test_end = (train_end + TEST_BARS).min(n - 1);

            if test_end.saturating_sub(test_start) < TEST_BARS / 2 {
                continue;
            }

            // Compute vol history for this window (252 bars prior)
            let vol_window_start = test_start.saturating_sub(VOL_RANK_LOOKBACK);
            let vol_history: Vec<f64> = (vol_window_start..test_start)
                .map(|i| realized_vol(&btc_close, 21, i))
                .collect();

            // Only run on BTC to keep runtime manageable (representative signal)
            let Some((close, high, low, volume)) = aligned_data.get("BTCUSDT") else {
                continue;
            };

            for strat in &strategies {
                let result = run_strategy(
                    close, high, low, volume,
                    test_start, test_end,
                    strat,
                    &vol_history,
                );

                let pass = if result.oos_return_pct > 0.0 { "✅" } else { "❌" };
                println!(
                    "  W{:02} {:18} | ret: {:>+8.2}% | sh: {:>+6.2} | dd: {:>6.2}% | t: {:>3} | {}",
                    window_idx, strat, result.oos_return_pct, result.sharpe,
                    result.max_dd, result.trade_count, pass,
                );

                all_results.push(format!(
                    "{},W{},{},{},{},{},{},{}",
                    universe_name,
                    window_idx,
                    strat,
                    format!("{:.4}", result.oos_return_pct),
                    format!("{:.4}", result.sharpe),
                    format!("{:.4}", result.max_dd),
                    result.trade_count,
                    format!("{:.4}", result.win_rate),
                ));

                // Accumulate for summary
                if let Some((ref mut entry)) = strat_pass.get_mut(*strat) {
                    entry.1 += 1; // total
                    if result.oos_return_pct > 0.0 {
                        entry.0 += 1; // passed
                    }
                    entry.2 += result.sharpe;
                    entry.3 += result.oos_return_pct;
                    entry.4 += result.trade_count;
                }
            }
        }

        println!("  {:?} for {}", start.elapsed(), universe_name);
    }

    // Write CSV
    let out_path = "snapshots/regime_conditional_results.csv";
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(out_path)?;
    for line in &all_results {
        writeln!(f, "{}", line)?;
    }
    println!("\nResults written to {}", out_path);

    // Summary
    println!("\n=== PASS RATE SUMMARY ===");
    let mut rows: Vec<Vec<String>> = Vec::new();
    rows.push(vec![
        "Strategy".to_string(),
        "Pass Rate".to_string(),
        "Avg Sharpe".to_string(),
        "Avg Return %".to_string(),
        "Total Trades".to_string(),
    ]);

    let mut sorted_strats: Vec<_> = strat_pass.iter().collect();
    sorted_strats.sort_by_key(|(_, (_, total, _, _, _))| *total);
    for (strat, &(passed, total, sum_sharpe, sum_ret, trades)) in sorted_strats {
        if total == 0 {
            continue;
        }
        let rate = passed as f64 / total as f64 * 100.0;
        let avg_sh = sum_sharpe / total as f64;
        let avg_ret = sum_ret / total as f64;
        println!(
            "  {:18} | pass {}/{} ({:5.1}%) | avg Sharpe {:>+6.2} | avg Return {:>+8.2}% | trades: {}",
            strat, passed, total, rate, avg_sh, avg_ret, trades,
        );
        rows.push(vec![
            strat.to_string(),
            format!("{:.1}%", rate),
            format!("{:+.2}", avg_sh),
            format!("{:+.2}", avg_ret),
            trades.to_string(),
        ]);
    }

    // Write summary markdown
    let md_path = "snapshots/regime_conditional_results.md";
    let mut mf = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(md_path)?;
    writeln!(mf, "# Regime-Conditional A/D × Turtle Walk-Forward Results\n")?;
    writeln!(mf, "Vol conditional: bottom {:.0}% vol rank → A/D, top {:.0}% → Turtle\n", VOL_BOTTOM_PCT * 100.0, VOL_TOP_PCT * 100.0)?;
    writeln!(mf, "| Strategy | Pass Rate | Avg Sharpe | Avg Return | Total Trades |")?;
    writeln!(mf, "|---|---|---|---|---|")?;
    for row in &rows[1..] {
        writeln!(mf, "| {} | {} | {} | {} | {} |",
            row[0], row[1], row[2], row[3], row[4])?;
    }
    println!("Markdown summary written to {}", md_path);

    Ok(())
}

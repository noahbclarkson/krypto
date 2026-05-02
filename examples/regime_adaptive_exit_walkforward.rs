//! T40 — Regime-Adaptive Exit Walk-Forward
//!
//! Mechanism: Condition Chandelier multiplier on slow BTC ATR percentile rank.
//!   - High-vol regime (rank > 75th pct):  M × 1.1 (looser stop — let winners run)
//!   - Low-vol regime (rank < 25th pct):  M × 0.9 (tighter stop — avoid chop)
//!   - Neutral:                             M × 1.0 (base multiplier 2.30)
//!
//! ATR percentile rank uses REGIME_ATR_P=64 and REGIME_LOOKBACK=42 — slow enough
//! to be a genuine regime classifier, NOT noise (contrast: prior fast 21-bar vol-rank
//! produced IDENTICAL results across all configs — regime is too fast to be useful).
//!
//! Prior vol-contingent Chandelier attempt (2026-04-12): DEAD GRAVEYARD.
//!   - Used 21-bar realized vol: all configs produced identical Sharpe (1.57-1.60).
//!   - Root cause: 21-bar vol_rank barely crosses 0.75/0.25 thresholds.
//!   - T40 uses 64-bar ATR percentile rank: slow enough to persist across bars.
//!
//! Configs tested:
//!   baseline:  fixed M=2.30 (no regime adaptation)
//!   config_1:  high=1.1, low=0.9  (modest vol conditioning)
//!   config_2:  high=1.2, low=0.8  (stronger vol conditioning)
//!   config_3:  high=1.15, low=0.85 (middle ground)
//!
//! Scope: Base5 × 7 walk-forward windows × 4 configs = 28 runs per config.
//! Guardrail: must beat fixed M=2.30 by ≥3 windows (≥+3pp pass rate) to accept.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

// --- Core strategy params (fixed) ---
const EP: usize = 21;
const CHAND_P: usize = 7;
const BASE_CHAND_M: f64 = 2.30;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 96;  // current production default

// --- Regime classifier params (fixed for T40) ---
const REGIME_ATR_P: usize = 64;      // ATR period for regime classification
const REGIME_LOOKBACK: usize = 42;   // lookback for percentile rank
const REGIME_PCT_HIGH: f64 = 75.0;   // high-vol threshold (percentile)
const REGIME_PCT_LOW: f64 = 25.0;   // low-vol threshold (percentile)

const BASE5: &[&str] = &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];

const CSV_OUT: &str = "snapshots/regime_adaptive_exit_wf.csv";
const MD_OUT: &str = "snapshots/regime_adaptive_exit_wf.md";

// --- Configs to test ---
struct RAEConfig {
    name: &'static str,
    mult_high: f64,  // applied when rank > REGIME_PCT_HIGH
    mult_low: f64,   // applied when rank < REGIME_PCT_LOW
}

const CONFIGS: &[RAEConfig] = &[
    RAEConfig { name: "baseline",  mult_high: 1.00, mult_low: 1.00 },
    RAEConfig { name: "rae_1.1_0.9", mult_high: 1.10, mult_low: 0.90 },
    RAEConfig { name: "rae_1.2_0.8", mult_high: 1.20, mult_low: 0.80 },
    RAEConfig { name: "rae_1.15_0.85", mult_high: 1.15, mult_low: 0.85 },
];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn tr(high: f64, low: f64, prev_close: f64) -> f64 {
    (high - low).max((high - prev_close).abs()).max((low - prev_close).abs())
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0_f64;
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        sum += tr(h, l, c0);
    }
    sum / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

/// Compute BTC ATR percentile rank at bar `idx` using REGIME_ATR_P / REGIME_LOOKBACK.
/// Returns value in [0.0, 100.0] = what pct of historical ATR values are BELOW current.
/// Returns 50.0 if insufficient history.
fn btc_atr_percentile_rank(btc_high: &[f64], btc_low: &[f64], btc_close: &[f64], idx: usize) -> f64 {
    let current_atr = atr_at(btc_high, btc_low, btc_close, REGIME_ATR_P, idx);
    if current_atr <= 0.0 { return 50.0; }

    if idx < REGIME_LOOKBACK + REGIME_ATR_P {
        return 50.0; // insufficient warmup
    }

    let mut count = 0usize;
    for i in (idx.saturating_sub(REGIME_LOOKBACK))..idx {
        let historical_atr = atr_at(btc_high, btc_low, btc_close, REGIME_ATR_P, i);
        if historical_atr < current_atr {
            count += 1;
        }
    }
    (count as f64 / REGIME_LOOKBACK as f64) * 100.0
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
    } else {
        false
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_sim(
    btc_data: &SymData,
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    cfg: &RAEConfig,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Turtle breakout entry (no regime filter)
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, EP, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Capture regime multiplier at entry
                        let rank = btc_atr_percentile_rank(&btc_data.high, &btc_data.low, &btc_data.close, bar);
                        let mult = if rank > REGIME_PCT_HIGH {
                            cfg.mult_high
                        } else if rank < REGIME_PCT_LOW {
                            cfg.mult_low
                        } else {
                            1.0
                        };
                        let chand_m = BASE_CHAND_M * mult;

                        // DUAL_EXIT: Chandelier ATR OR Turtle ATR
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            // Chandelier ATR with regime-adaptive multiplier
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_P, b);
                            let trail_chand = highest_high_chand - chand_m * atr_chand;
                            // Turtle ATR trailing stop
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            // Exit on EITHER stop
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            if equity > peak { peak = equity; }
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== T40: Regime-Adaptive Exit Walk-Forward ====");
    eprintln!("Base5 × 7 windows × {} configs\n", CONFIGS.len());

    let loader = DataLoader::new(None, None);

    // Load BTC data first (for regime classifier)
    let btc_df = loader.fetch_with_cache("BTCUSDT", "1d", CANDLES).await?;
    let n_btc = btc_df.height().min(2800);
    let btc_data = {
        let col = |name: &str| -> Vec<f64> {
            btc_df.column(name).unwrap().f64().unwrap()
                .into_iter().filter_map(|x| x).take(n_btc).collect()
        };
        SymData {
            close: col("close"),
            high: col("high"),
            low: col("low"),
            vol: col("volume"),
        }
    };
    eprintln!("BTC: {} bars loaded", btc_data.close.len());

    // Load universe symbols
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;
    for &sym in BASE5 {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let n_sym = df.height().min(2800);
                min_len = min_len.min(n_sym);
                let col = |name: &str| -> Vec<f64> {
                    df.column(name).unwrap().f64().unwrap()
                        .into_iter().filter_map(|x| x).take(n_sym).collect()
                };
                sym_data_map.insert(sym.to_string(), SymData {
                    close: col("close"),
                    high: col("high"),
                    low: col("low"),
                    vol: col("volume"),
                });
                eprintln!("  {}: {} bars", sym, n_sym);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len;
    eprintln!("\nCommon bars: {}\n", n);

    let symbols: Vec<String> = BASE5.iter().map(|s| s.to_string()).collect();

    // Walk-forward windows
    let total_train_test = TRAIN_BARS + TEST_BARS;
    let n_windows = if n > total_train_test { (n - TRAIN_BARS) / TEST_BARS } else { 0 };
    eprintln!("{} walk-forward windows\n", n_windows);

    // Results storage
    #[derive(Default)]
    struct ConfigResults {
        results: Vec<WfResult>,
    }
    let mut all_results: HashMap<&'static str, ConfigResults> = HashMap::new();
    for cfg in CONFIGS {
        all_results.insert(cfg.name, ConfigResults::default());
    }

    // CSV header
    let mut csv = File::create(CSV_OUT)?;
    writeln!(csv, "config,window,ret_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;

    eprintln!("{:<20} {:>6} {:>10} {:>8} {:>10} {:>6} {:>8} {:>5}",
        "config", "win", "ret%", "sharpe", "maxDD%", "trds", "win%", "pass");
    eprintln!("{}", "-".repeat(80));

    for w in 0..n_windows {
        let test_start = TRAIN_BARS + w * TEST_BARS;
        let test_end = (test_start + TEST_BARS).min(n);

        if test_end - test_start < 50 { continue; }

        for cfg in CONFIGS {
            let res = run_sim(&btc_data, &sym_data_map, &symbols, test_start, test_end, cfg);
            let pass_str = if res.pass { "PASS" } else { "FAIL" };
            eprintln!("{:<20} {:>6} {:>10.2} {:>8.3} {:>10.2} {:>6} {:>8.1} {:>5}",
                cfg.name, w, res.ret, res.sharpe, res.max_dd, res.trades, res.win_rate, pass_str);
            writeln!(csv, "{},{},{:.4},{:.6},{:.4},{},{:.4},{}",
                cfg.name, w, res.ret, res.sharpe, res.max_dd, res.trades, res.win_rate, res.pass)?;
            all_results.get_mut(cfg.name).unwrap().results.push(res);
        }
    }

    drop(csv);

    // Summary
    eprintln!("\n{}", "=".repeat(80));
    eprintln!("SUMMARY: T40 Regime-Adaptive Exit (Base5 × {} windows)", n_windows);
    eprintln!("{}", "=".repeat(80));

    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# T40: Regime-Adaptive Exit Walk-Forward Results")?;
    writeln!(md, "\nUniverse: Base5 | Windows: {} | Train/Test: {}/{}\n", n_windows, TRAIN_BARS, TEST_BARS)?;
    writeln!(md, "| Config | Pass | Avg Sharpe | Avg Ret% | Avg DD% | Trades | Win% |")?;
    writeln!(md, "|--------|------|------------|---------|---------|--------|------|")?;

    let mut best_sharpe = f64::NEG_INFINITY;
    let mut best_name = "";
    let mut best_pass_count = 0;
    let mut best_avg_ret = 0.0;

    for cfg in CONFIGS {
        let results = &all_results[cfg.name].results;
        if results.is_empty() { continue; }
        let pass_count = results.iter().filter(|r| r.pass).count();
        let avg_sharpe: f64 = results.iter().map(|r| r.sharpe).sum::<f64>() / results.len() as f64;
        let avg_ret: f64 = results.iter().map(|r| r.ret).sum::<f64>() / results.len() as f64;
        let avg_dd: f64 = results.iter().map(|r| r.max_dd).sum::<f64>() / results.len() as f64;
        let avg_win: f64 = results.iter().map(|r| r.win_rate).sum::<f64>() / results.len() as f64;
        let total_trades: usize = results.iter().map(|r| r.trades).sum();

        let pct = pass_count as f64 / results.len() as f64 * 100.0;
        eprintln!("{:<20} {:>3}/{} ({:>5.1}%%)  sharpe={:>8.3}  ret={:>10.2}%%  DD={:>8.2}%%  trds={:>5}",
            cfg.name, pass_count, results.len(), pct,
            avg_sharpe, avg_ret, avg_dd, total_trades);

        writeln!(md, "| {} | {}/{} ({:.1}%%) | {:.3} | {:.1}%% | {:.1}%% | {} | {:.1}%% |",
            cfg.name, pass_count, results.len(), pct,
            avg_sharpe, avg_ret, avg_dd, total_trades, avg_win)?;

        if avg_sharpe > best_sharpe {
            best_sharpe = avg_sharpe;
            best_name = cfg.name;
            best_pass_count = pass_count;
            best_avg_ret = avg_ret;
        }
    }

    // Baseline comparison
    let baseline = all_results.get("baseline").and_then(|b| Some(&b.results)).unwrap();
    let baseline_pass = baseline.iter().filter(|r| r.pass).count();
    let baseline_sharpe: f64 = baseline.iter().map(|r| r.sharpe).sum::<f64>() / baseline.len().max(1) as f64;

    writeln!(md, "\n## Verdict\n")?;
    if best_name == "baseline" {
        writeln!(md, "**REJECTED — baseline (fixed M=2.30) wins.** Regime-adaptive multiplier provides no improvement.\n")?;
        eprintln!("\nVERDICT: REJECTED — baseline fixed M=2.30 wins.");
    } else {
        let best_results = &all_results[best_name].results;
        let best_pass = best_results.iter().filter(|r| r.pass).count();
        let delta_pass = best_pass as i32 - baseline_pass as i32;
        let delta_sharpe = best_sharpe - baseline_sharpe;
        writeln!(md, "**Candidate: {}** vs baseline (fixed M=2.30)\n", best_name)?;
        writeln!(md, "| Metric | Baseline | {} | Delta |", best_name)?;
        writeln!(md, "|--------|----------|------|-------|")?;
        let baseline_pct = baseline_pass as f64 / baseline.len() as f64 * 100.0;
        let best_pct = best_pass as f64 / best_results.len() as f64 * 100.0;
        let dp_str = format!("{:+}", delta_pass);
        writeln!(md, "| Pass rate | {}/{} ({:.1}%) | {}/{} ({:.1}%) | {} |",
            baseline_pass, baseline.len(), baseline_pct,
            best_pass, best_results.len(), best_pct,
            dp_str)?;
        let ds_str = format!("{:+.3}", delta_sharpe);
        writeln!(md, "| Avg Sharpe | {:.3} | {:.3} | {} |", baseline_sharpe, best_sharpe, ds_str)?;
        let baseline_avg_ret = baseline.iter().map(|r| r.ret).sum::<f64>() / baseline.len().max(1) as f64;
        let best_avg_ret_v = best_results.iter().map(|r| r.ret).sum::<f64>() / best_results.len().max(1) as f64;
        let delta_ret = best_avg_ret_v - baseline_avg_ret;
        writeln!(md, "| Avg Return | {:.1}% | {:.1}% | {:+.1}pp |",
            baseline_avg_ret,
            best_avg_ret_v,
            delta_ret)?;

        let improved = delta_pass >= 3;
        let dp2 = delta_pass;
        if improved {
            writeln!(md, "\n**ACCEPTED — beats baseline by ≥3 windows.**\n")?;
            eprintln!("\nVERDICT: ACCEPTED — {} beats baseline by {} windows.", best_name, dp2);
        } else {
            writeln!(md, "\n**MARGINAL — beats baseline by <3 windows. Anti-overfit: NOT promoted.**\n")?;
            eprintln!("\nVERDICT: MARGINAL — {} only {} windows over baseline. Not promoted.", best_name, dp2);
        }
    }

    drop(md);

    let elapsed = t0.elapsed();
    eprintln!("\nDone in {:.1}s → {}", elapsed.as_secs_f64(), MD_OUT);
    eprintln!("CSV: {}", CSV_OUT);

    Ok(())
}

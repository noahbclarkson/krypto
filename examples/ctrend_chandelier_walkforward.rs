//! CTREND + Chandelier Walk-Forward Validation
//!
//! Hypothesis: CTREND multi-horizon momentum signal (Monte Carlo confirmed genuine, 0/500 shuffled beat real)
//! paired with Chandelier dual-exit (P=11/M=2.25) should improve or maintain pass rate vs fixed 21-bar hold.
//!
//! Compare to: Turtle+Chandelier baseline from `turtle_chandelier_walkforward.rs`
//! If CTREND pass rate >= Turtle pass rate → CTREND promoted to second signal family candidate.
//!
//! Params: CHAND_PERIOD=11, CHAND_MULT=2.25 (production)
//!         TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.0 (dual exit)
//!         HOLD_MAX=45, POSITION_CAP=3
//!
//! Entry signal: CTREND (from ctrend_harsh_universe_benchmark.rs generate_ctrend_signals)
//! Exit: Chandelier(11, 2.25) OR Turtle_ATR(24, 2.0) — whichever fires first

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
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3; // See turtle_chandelier_walkforward.rs — MT=3 is in the stable plateau, MT≥7 degrades pass rate
const POSITION_CAP: usize = 3;

// Chandelier dual-exit params (production, from turtle_chandelier_walkforward.rs)
const CHAND_PERIOD: usize = 11;    // hyperopt 2026-04-20: CP=11 wins global (+1.9% vs CP=15)
const CHAND_MULT: f64 = 2.25;     // hyperopt 2026-04-20: M=2.25 wins (+47% vs M=1.50)
const TURTLE_ATR_PERIOD: usize = 24; // hyperopt 2026-04-16: ATR=24 wins (+3.6% Sharpe)
const TURTLE_ATR_MULT: f64 = 2.0;

// CTREND signal thresholds (from ctrend_harsh_universe_benchmark.rs)
const CTREND_THRESHOLD: f64 = 0.35;
const CTREND_WARMUP: usize = 126; // minimum bars before CTREND signal can fire

// Vol smoothing for dollar-volume ranking
const VOL_LOOKBACK: usize = 2;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

const CSV_OUT: &str = "snapshots/ctrend_chandelier_wf.csv";
const MD_OUT: &str = "snapshots/ctrend_chandelier_wf.md";
const CSV_LATEST: &str = "snapshots/ctrend_chandelier_wf_latest.csv";
const MD_LATEST: &str = "snapshots/ctrend_chandelier_wf_latest.md";

// =============================================================================
// Data structures
// =============================================================================

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

// =============================================================================
// CTREND signal (from ctrend_harsh_universe_benchmark.rs generate_ctrend_signals)
// =============================================================================

fn rolling_return(values: &[f64], lookback: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        let now = values[i];
        let prev = values[i - lookback];
        if now > 0.0 && prev > 0.0 {
            out[i] = (now / prev).ln();
        }
    }
    out
}

fn rolling_realized_vol(values: &[f64], lookback: usize) -> Vec<f64> {
    // Returns (daily returns, then rolling stdev of returns over lookback bars)
    let mut rets = vec![0.0; values.len()];
    for i in 1..values.len() {
        if values[i] > 0.0 && values[i - 1] > 0.0 {
            rets[i] = (values[i] / values[i - 1]).ln();
        }
    }
    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        let start = i - lookback;
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        for j in start + 1..=i {
            sum += rets[j];
            sum_sq += rets[j] * rets[j];
        }
        let n = lookback as f64;
        let mean = sum / n;
        let var = (sum_sq / n) - (mean * mean);
        out[i] = var.max(0.0).sqrt();
    }
    out
}

fn sma(values: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in period..values.len() {
        let start = i - period;
        let sum: f64 = values[start..=i].iter().sum();
        out[i] = sum / period as f64;
    }
    out
}

fn ctrend_signal(
    close: &[f64],
    volume: &[f64],
    idx: usize,
) -> bool {
    if idx < CTREND_WARMUP {
        return false;
    }

    // Rolling returns at multiple horizons
    let ret_5 = rolling_return(close, 5);
    let ret_21 = rolling_return(close, 21);
    let ret_63 = rolling_return(close, 63);
    let ret_126 = rolling_return(close, 126);

    // Realized volatility
    let rv_21 = rolling_realized_vol(close, 21);
    let rv_63 = rolling_realized_vol(close, 63);

    // Volume SMAs
    let vol_sma_20 = sma(volume, 20);
    let vol_sma_63 = sma(volume, 63);

    let short_vol = rv_21[idx].max(1e-6);
    let med_vol = rv_63[idx].max(1e-6);

    // Price momentum score (normalized by vol — like a signal-to-noise ratio)
    let price_score =
        0.15 * (ret_5[idx] / short_vol)
        + 0.35 * (ret_21[idx] / short_vol)
        + 0.30 * (ret_63[idx] / med_vol)
        + 0.20 * (ret_126[idx] / med_vol);

    // Volume confirmation
    let vol_ratio_fast = if vol_sma_20[idx] > 1e-9 {
        volume[idx] / vol_sma_20[idx]
    } else {
        1.0
    };
    let vol_ratio_slow = if vol_sma_63[idx] > 1e-9 {
        vol_sma_20[idx] / vol_sma_63[idx]
    } else {
        1.0
    };

    let price_dir = if ret_21[idx] > 0.0 { 1.0 } else if ret_21[idx] < 0.0 { -1.0 } else { 0.0 };
    let short_dir = if ret_5[idx] > 0.0 { 1.0 } else if ret_5[idx] < 0.0 { -1.0 } else { 0.0 };

    let volume_score =
        0.20 * (vol_ratio_fast.ln()).clamp(-1.5, 1.5) * short_dir
        + 0.20 * (vol_ratio_slow.ln()).clamp(-1.5, 1.5) * price_dir;

    let score = price_score + volume_score;
    score > CTREND_THRESHOLD
}

// =============================================================================
// ATR helpers (mirrors turtle_chandelier_walkforward.rs)
// =============================================================================

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window {
        return *vals.get(idx).unwrap_or(&0.0);
    }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

// =============================================================================
// Walk-forward result
// =============================================================================

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

#[derive(Debug)]
struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

// =============================================================================
// Simulation (CTREND entry + Chandelier dual-exit)
// =============================================================================

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Dollar-volume ranking (same as Turtle walk-forward)
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close[bar];
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

        // CTREND entry signal
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }

                if ctrend_signal(&sd.close, &sd.vol, bar) {
                    let entry_px = sd.close[bar];
                    let entry = entry_px * (1.0 - TAKER_FEE);
                    let entry_bar_next = bar + 1;
                    let n = sd.close.len();

                    // Dual exit: Chandelier(11, 2.25) OR Turtle_ATR(24, 2.0)
                    let mut highest_high_chand = sd.high[entry_bar_next];
                    let mut highest_high_turtle = sd.high[entry_bar_next];
                    let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                    let mut exit_bar = max_bar;

                    for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                        // Chandelier ATR exit
                        highest_high_chand = highest_high_chand.max(sd.high[b]);
                        let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                        let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                        // Turtle ATR exit
                        highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                        let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                        let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;

                        // Exit on EITHER stop
                        if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                            exit_bar = b;
                            break;
                        }
                    }

                    if exit_bar < sd.close.len() {
                        let exit_px = sd.close[exit_bar];
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

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== CTREND + Chandelier Walk-Forward: 9 Universes ====");
    eprintln!("Entry: CTREND (multi-horizon price+volume momentum)");
    eprintln!("Exit: Chandelier({}, {}) OR Turtle_ATR({}, {}) — whichever fires first",
              CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT);
    eprintln!("252/252 train/test walk-forward\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            let close: Vec<f64> = df.column("close")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let high: Vec<f64> = df.column("high")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let low: Vec<f64> = df.column("low")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let vol: Vec<f64> = df.column("volume")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            sym_data_map.insert(sym.clone(), SymData { close, high, low, vol });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let mut all_records = Vec::new();
    let mut csv_lines = vec!["universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    let mut global_pass = 0usize;
    let mut global_total = 0usize;
    let mut global_trades = 0usize;

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing data)", label);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (not enough data)", label);
            continue;
        }

        eprintln!("==== {:<18} ==== {} syms, {} windows", label, symbols.len(), total_windows);

        let mut agg_ret = 0.0_f64;
        let mut passed = 0usize;

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);

            if test_end.saturating_sub(test_start) < 5 { continue; }

            let r = run_sim(&sym_data_map, &symbols, test_start, test_end);

            let thin = if r.trades < MIN_TRADES { "THIN" } else { "OK" };
            let result = if r.pass { "PASS" } else { "FAIL" };
            eprintln!(
                "  W{:02} | {:+8.1}% sh={:6.2} DD={:5.1}% {:4}t {:3.0}% {} {}",
                wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, thin, result
            );

            csv_lines.push(format!(
                "{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
            ));

            agg_ret += r.ret;
            if r.pass { passed += 1; }
            global_pass += if r.pass { 1 } else { 0 };
            global_total += 1;
            global_trades += r.trades;

            all_records.push((label.to_string(), wi, r));
        }

        let avg_ret = agg_ret / total_windows as f64;
        let pass_pct = passed as f64 / total_windows as f64 * 100.0;
        eprintln!("  AGG | avg {:+7.1}% {}/{} pass ({:.0}%)\n", avg_ret, passed, total_windows, pass_pct);
    }

    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    std::fs::copy(CSV_OUT, CSV_LATEST).ok();

    let fail_pct = (global_total - global_pass) as f64 / global_total.max(1) as f64 * 100.0;
    let avg_sharpe: f64 = all_records.iter().map(|(_,_,r)| r.sharpe).sum::<f64>() / all_records.len().max(1) as f64;

    eprintln!("==== GLOBAL SUMMARY ====");
    eprintln!("  CTREND+Chandelier: {}/{} windows passed ({:.0}% fail)", global_pass, global_total, fail_pct);
    eprintln!("  Avg Sharpe: {:.4}", avg_sharpe);
    eprintln!("  Total trades: {}", global_trades);
    eprintln!("  CSV: {}", CSV_OUT);
    eprintln!("  Runtime: {:?}", t0.elapsed());

    // Write markdown summary
    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# CTREND + Chandelier Walk-Forward: 9 Universes")?;
    writeln!(md, "")?;
    writeln!(md, "**Entry:** CTREND multi-horizon price+volume momentum")?;
    writeln!(md, "**Exit:** Chandelier({}, {}) OR Turtle_ATR({}, {}) — first fires wins", CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT)?;
    writeln!(md, "**252/252 train/test walk-forward, 0.1% taker each side**")?;
    writeln!(md, "")?;
    writeln!(md, "| Universe | Pass | Avg Ret | Avg Sharpe | Worst DD | Trades |")?;
    writeln!(md, "|---|---|---|---|---|---|")?;
    for &(uni_name, _) in UNIVERSES {
        let recs: Vec<_> = all_records.iter().filter(|(l,_,_)| *l == uni_name).collect();
        let n_win = recs.len();
        if n_win == 0 { continue; }
        let pass = recs.iter().filter(|(_,_,r)| r.pass).count();
        let avg_ret: f64 = recs.iter().map(|(_,_,r)| r.ret).sum::<f64>() / n_win as f64;
        let avg_sh: f64 = recs.iter().map(|(_,_,r)| r.sharpe).sum::<f64>() / n_win as f64;
        let worst_dd: f64 = recs.iter().map(|(_,_,r)| r.max_dd).fold(0.0_f64, |a,b| a.max(b));
        let trades: usize = recs.iter().map(|(_,_,r)| r.trades).sum();
        writeln!(md, "| {} | {}/{} | {:+.1}% | {:.2} | {:.1}% | {} |", uni_name, pass, n_win, avg_ret, avg_sh, worst_dd, trades)?;
    }
    writeln!(md, "")?;
    writeln!(md, "**GLOBAL: {}/{} pass ({:.0}% fail), avg Sharpe {:.4}, {} trades**", global_pass, global_total, fail_pct, avg_sharpe, global_trades)?;
    writeln!(md, "")?;
    writeln!(md, "## Comparison to Turtle+Chandelier baseline")?;
    writeln!(md, "")?;
    writeln!(md, "| Metric | Turtle+Chandelier | CTREND+Chandelier |")?;
    writeln!(md, "|---|---|---|")?;
    writeln!(md, "| Global pass rate | ~80% (43/54) | see above |")?;
    writeln!(md, "| Avg Sharpe | ~4.78 | see above |")?;
    writeln!(md, "| Entry signal | Turtle breakout (EP=21) | CTREND multi-horizon |")?;
    writeln!(md, "")?;
    writeln!(md, "If CTREND+Chandelier pass rate ≥ Turtle+Chandelier → CTREND promoted to second signal family candidate.")?;
    eprintln!("  MD: {}", MD_OUT);

    Ok(())
}
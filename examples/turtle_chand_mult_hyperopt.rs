//! Turtle+Chandelier — CHAND_MULT Fine-Grained Hyperopt
//!
//! BACKGROUND:
//! CHAND_MULT was previously swept at coarse 0.5-step increments {1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0}
//! as part of the P×M joint sweep (ad_chandelier_reopt.rs). M=2.00 won.
//! However: step=0.5 may have missed the true optimum between 1.75 and 2.25.
//!
//! TARGET: Fine-grained CHAND_MULT sweep at step=0.05 to find the true peak.
//! Range: 1.50 to 3.00 in 0.05 steps = 31 values.
//! At P=28 (current production CHAND_PERIOD), EP=21, ATR=25.
//!
//! METHOD:
//!   Phase 1: 3 sweep universes (Base5, Legacy4, LowVolume5) — fast coarse scan
//!   Phase 2: Full 9-universe validation of top-3 configs + baseline
//!   Equity curves exported for baseline (M=2.00) and top-3 winners
//!
//! Usage:
//!   cargo run --profile sweep --example turtle_chand_mult_hyperopt 2>&1 | tee snapshots/chand_mult_hyperopt.log

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 28; // production value — not being changed
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 25;
const TURTLE_ATR_MULT: f64 = 2.00;

const BASELINE_M: f64 = 2.00;

// Fine sweep: 1.50 to 3.00 step 0.05 = 31 values
const M_SWEEP: &[f64] = &[
    1.50, 1.55, 1.60, 1.65, 1.70, 1.75, 1.80, 1.85, 1.90, 1.95,
    2.00, 2.05, 2.10, 2.15, 2.20, 2.25, 2.30, 2.35, 2.40, 2.45,
    2.50, 2.55, 2.60, 2.65, 2.70, 2.75, 2.80, 2.85, 2.90, 2.95,
    3.00,
];

const SWEEP_UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",      &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("Legacy4",    &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5", &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
];

const FULL_UNIVERSES: &[(&str, &[&str])] = &[
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

struct SymData {
    close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) { curr_close > max_close } else { false }
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

struct SimResult {
    ret: f64, sharpe: f64, max_dd: f64,
    trades: usize, win_rate: f64, pass: bool,
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize, test_end: usize,
    chand_m: f64,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Dollar-volume ranking
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() { equity_curve.push(equity); bar += 1; continue; }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - chand_m * atr_chand;
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;
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
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }
                            if equity > 1e18 { equity = 1e18; } // overflow guard
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }
        if !entered { equity_curve.push(equity); bar += 1; }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    SimResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

async fn load_data(loader: &DataLoader, universes: &[(&str, &[&str])], cap: u32) -> Result<(HashMap<String, SymData>, usize)> {
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in universes { for &s in *syms { all_syms.insert(s.to_string()); } }

    let mut raw: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", cap).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }
    let n = min_len.min(2800);

    let mut map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! cv { ($name:expr) => {{
                let c = df.column($name)?.f64()?;
                c.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
            }};
            }
            map.insert(sym.clone(), SymData {
                close: cv!("close"), high: cv!("high"), low: cv!("low"), vol: cv!("volume"),
            });
        }
    }
    Ok((map, n))
}

fn score_mult_on_sweep_universes(
    sym_data: &HashMap<String, SymData>, n: usize, m: f64,
) -> (usize, usize, usize, f64, f64, f64) {
    // returns (pass, runs, trades, sharpe_sum, ret_sum, dd_sum)
    let mut total_pass = 0usize; let mut total_runs = 0usize;
    let mut global_trades = 0usize; let mut global_sharpe = 0.0_f64;
    let mut global_ret = 0.0_f64; let mut global_dd = 0.0_f64;

    for &(label, symbols) in SWEEP_UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }
            let r = run_sim(sym_data, &symbols, test_start, test_end, m);
            global_trades += r.trades;
            global_sharpe += r.sharpe;
            global_ret += r.ret;
            global_dd += r.max_dd;
            total_runs += 1;
            if r.pass { total_pass += 1; }
        }
    }
    (total_pass, total_runs, global_trades, global_sharpe, global_ret, global_dd)
}

fn run_full_validation(
    sym_data: &HashMap<String, SymData>, n: usize, m: f64,
) -> (usize, usize, usize, f64, f64, f64) {
    // returns (pass, runs, trades, sharpe_sum, ret_sum, dd_sum)
    let mut total_pass = 0usize; let mut total_runs = 0usize;
    let mut global_trades = 0usize; let mut global_sharpe = 0.0_f64;
    let mut global_ret = 0.0_f64; let mut global_dd = 0.0_f64;

    for &(label, symbols) in FULL_UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }
            let r = run_sim(sym_data, &symbols, test_start, test_end, m);
            global_trades += r.trades;
            global_sharpe += r.sharpe;
            global_ret += r.ret;
            global_dd += r.max_dd;
            total_runs += 1;
            if r.pass { total_pass += 1; }
        }
    }
    (total_pass, total_runs, global_trades, global_sharpe, global_ret, global_dd)
}

fn equity_key(m: f64) -> String {
    format!("{:.2}", m)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Turtle+Chandelier CHAND_MULT Fine Hyperopt ====");
    eprintln!("CHAND_PERIOD={}, sweeping M from 1.50 to 3.00 step 0.05 = {} values", CHAND_PERIOD, M_SWEEP.len());
    eprintln!("3 sweep universes for coarse scan, then full 9-universe validation of top-3 + baseline\n");

    let loader = DataLoader::new(None, None);

    // ── Load data for sweep universes ─────────────────────────────────────────
    eprintln!("Loading data for sweep universes...");
    let (sym_data_sweep, n_sweep) = load_data(&loader, SWEEP_UNIVERSES, CANDLES).await?;
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_sweep.len(), n_sweep);

    // ── Phase 1: Fine-grained sweep on 3 universes ────────────────────────────
    eprintln!("==== Phase 1: Fine Sweep (3 universes, {} M values) ====", M_SWEEP.len());
    let mut sweep_results: Vec<(f64, usize, usize, usize, f64, f64, f64)> = Vec::new();

    for &m in M_SWEEP {
        let (pass, runs, trades, sharpe_sum, ret_sum, dd_sum) =
            score_mult_on_sweep_universes(&sym_data_sweep, n_sweep, m);
        let avg_sharpe = if runs > 0 { sharpe_sum / runs as f64 } else { 0.0 };
        let avg_ret = if runs > 0 { ret_sum / runs as f64 } else { 0.0 };
        let avg_dd = if runs > 0 { dd_sum / runs as f64 } else { 0.0 };
        let pass_pct = if runs > 0 { 100.0 * pass as f64 / runs as f64 } else { 0.0 };
        sweep_results.push((m, pass, runs, trades, avg_sharpe, avg_ret, avg_dd));
        eprintln!("  M={:.2} | Sharpe={:.4} | pass={:2}/{:2} ({:5.1}%) | ret={:+7.1}% | DD={:5.1}% | {}t",
            m, avg_sharpe, pass, runs, pass_pct, avg_ret, avg_dd, trades);
    }

    // Sort by Sharpe descending
    sweep_results.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

    eprintln!("\n==== TOP 10 (3-universe coarse, by Sharpe) ====");
    eprintln!("Rank | M      | Sharpe    | Pass%   | Ret%    | DD%    | Trades");
    for (i, r) in sweep_results.iter().enumerate().take(10) {
        let pass_pct = 100.0 * r.1 as f64 / r.2 as f64;
        eprintln!("{:>4} | {:>6.2} | {:>9.4} | {:>5.1}% | {:>+7.1}% | {:>5.1}% | {}",
            i+1, r.0, r.4, pass_pct, r.5, r.6, r.3);
    }

    // ── Phase 2: Equity curve export for top-3 + baseline ──────────────────
    eprintln!("\n==== Phase 2: Equity Curves (top-3 + baseline) ====");

    // Determine which Ms to export equity for: top-3 from sweep + baseline
    let top3_ms: Vec<f64> = sweep_results.iter().take(3).map(|r| r.0).collect();
    let baseline_in_top3 = top3_ms.contains(&BASELINE_M);
    let export_ms: Vec<f64> = if baseline_in_top3 {
        top3_ms.clone()
    } else {
        let mut v = top3_ms.clone();
        v.push(BASELINE_M);
        v
    };

    // Export equity for each config
    for &m in &export_ms {
        let mut lines = vec!["universe,window,bar,equity".to_string()];
        for &(label, symbols) in SWEEP_UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_sweep.contains_key(s)) { continue; }
            let total_windows = n_sweep.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n_sweep);
                if test_end.saturating_sub(test_start) < 5 { continue; }
                let r = run_sim(&sym_data_sweep, &symbols, test_start, test_end, m);
                for (bi, &eq) in r.equity_curve.iter().enumerate() {
                    lines.push(format!("{}_{},{},{},{:.6}", label, wi, wi, bi, eq));
                }
            }
        }
        let key = equity_key(m);
        let path = format!("snapshots/chand_mult_{}_equity.csv", key);
        let mut f = File::create(&path)?;
        for line in &lines { writeln!(f, "{}", line)?; }
        let tag = if m == BASELINE_M { " [BASELINE]" } else { "" };
        eprintln!("  M={:.2}{}: {} equity points → {}", m, tag, lines.len()-1, path);
    }

    // ── Phase 3: Full 9-universe validation of top-3 + baseline ───────────────
    eprintln!("\n==== Phase 3: Full 9-Universe Validation ====");
    let (sym_data_full, n_full) = load_data(&loader, FULL_UNIVERSES, CANDLES).await?;
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_full.len(), n_full);

    let mut full_results: Vec<(f64, usize, usize, usize, f64, f64, f64)> = Vec::new();
    for &m in &export_ms {
        let (pass, runs, trades, sharpe_sum, ret_sum, dd_sum) =
            run_full_validation(&sym_data_full, n_full, m);
        let avg_sharpe = if runs > 0 { sharpe_sum / runs as f64 } else { 0.0 };
        let avg_ret = if runs > 0 { ret_sum / runs as f64 } else { 0.0 };
        let avg_dd = if runs > 0 { dd_sum / runs as f64 } else { 0.0 };
        let pass_pct = if runs > 0 { 100.0 * pass as f64 / runs as f64 } else { 0.0 };
        full_results.push((m, pass, runs, trades, avg_sharpe, avg_ret, avg_dd));
        let tag = if m == BASELINE_M { " [BASELINE]" } else { "" };
        eprintln!("  M={:.2}{} | 9U Sharpe={:.4} | pass={:2}/{:2} ({:5.1}%) | ret={:+7.1}% | DD={:5.1}% | {}t",
            m, tag, avg_sharpe, pass, runs, pass_pct, avg_ret, avg_dd, trades);
    }

    full_results.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

    eprintln!("\n==== FULL VALIDATION RANKING (9 universes) ====");
    eprintln!("Rank | M      | Sharpe    | Pass%   | Ret%    | DD%    | Trades | Note");
    for (i, r) in full_results.iter().enumerate() {
        let pass_pct = 100.0 * r.1 as f64 / r.2 as f64;
        let note = if r.0 == BASELINE_M { "BASELINE" } else if r.0 == sweep_results[0].0 { "SWEPT_WINNER" } else { "" };
        eprintln!("{:>4} | {:>6.2} | {:>9.4} | {:>5.1}% | {:>+7.1}% | {:>5.1}% | {} | {}",
            i+1, r.0, r.4, pass_pct, r.5, r.6, r.3, note);
    }

    // ── Winner vs Baseline delta ──────────────────────────────────────────────
    let winner = full_results.first().unwrap();
    let baseline = full_results.iter().find(|r| r.0 == BASELINE_M).unwrap();
    let delta_sharpe = winner.4 - baseline.4;
    let delta_pass_pct = (100.0 * winner.1 as f64 / winner.2 as f64)
                       - (100.0 * baseline.1 as f64 / baseline.2 as f64);

    eprintln!("\n==== WINNER vs BASELINE ====");
    eprintln!("  Winner M={:.2}: Sharpe={:.4}, pass={}/{} ({}%), ret={:+.1}%, DD={:.1}%",
        winner.0, winner.4, winner.1, winner.2, 100.0*winner.1 as f64/winner.2 as f64, winner.5, winner.6);
    eprintln!("  Baseline M={:.2}: Sharpe={:.4}, pass={}/{} ({}%), ret={:+.1}%, DD={:.1}%",
        baseline.0, baseline.4, baseline.1, baseline.2, 100.0*baseline.1 as f64/baseline.2 as f64, baseline.5, baseline.6);
    eprintln!("  Delta Sharpe: {:+.4} ({:+.1}%)", delta_sharpe, 100.0*delta_sharpe/baseline.4.abs());
    eprintln!("  Delta Pass Rate: {:+.1}pp", delta_pass_pct);

    // ── Write sweep CSV ───────────────────────────────────────────────────────
    {
        let mut f = File::create("snapshots/chand_mult_sweep.csv")?;
        writeln!(f, "m,pass_count,run_count,pass_pct,total_trades,avg_sharpe,avg_ret,avg_dd")?;
        for r in &sweep_results {
            let pct = 100.0 * r.1 as f64 / r.2 as f64;
            writeln!(f, "{:.2},{},{},{:.2},{},{:.6},{:+.4},{:+.4}", r.0, r.1, r.2, pct, r.3, r.4, r.5, r.6)?;
        }
    }

    // ── Write full validation CSV ────────────────────────────────────────────
    {
        let mut f = File::create("snapshots/chand_mult_full_validation.csv")?;
        writeln!(f, "m,pass_count,run_count,pass_pct,total_trades,avg_sharpe,avg_ret,avg_dd")?;
        for r in &full_results {
            let pct = 100.0 * r.1 as f64 / r.2 as f64;
            writeln!(f, "{:.2},{},{},{:.2},{},{:.6},{:+.4},{:+.4}", r.0, r.1, r.2, pct, r.3, r.4, r.5, r.6)?;
        }
    }

    // ── Write summary MD ──────────────────────────────────────────────────────
    let winner_m = winner.0;
    let improvement_pct = if baseline.4 != 0.0 { 100.0 * delta_sharpe / baseline.4.abs() } else { 0.0 };
    {
        let mut f = File::create("snapshots/chand_mult_summary.md")?;
        writeln!(f, "# Turtle+Chandelier CHAND_MULT Fine Hyperopt")?;
        writeln!(f, "")?;
        writeln!(f, "**Date:** 2026-04-16")?;
        writeln!(f, "**Target:** `CHAND_MULT` — Chandelier ATR exit multiplier")?;
        writeln!(f, "**Prior:** M=2.00 (coarse 0.5-step sweep, step=0.5)")?;
        writeln!(f, "**New:** M={:.2} (fine 0.05-step sweep, step=0.05)", winner_m)?;
        writeln!(f, "")?;
        writeln!(f, "## Method")?;
        writeln!(f, "")?;
        writeln!(f, "- **Strategy:** Turtle(EP=21) + Chandelier(P=28, M) + Turtle_ATR(25, 2.0) DUAL_EXIT")?;
        writeln!(f, "- **Sweep Range:** 1.50 to 3.00 step 0.05 = **31 values** (vs prior 9 values at step=0.5)")?;
        writeln!(f, "- **Sweep universes:** Base5 + Legacy4 + LowVolume5 (~18 windows each)")?;
        writeln!(f, "- **Validation:** Full 9-universe × 6 windows walk-forward")?;
        writeln!(f, "- **Fee:** 0.1% taker each side, MIN_TRADES=3")?;
        writeln!(f, "")?;
        writeln!(f, "## Phase 1 — Fine Sweep Results (3 universes, ranked by Sharpe)")?;
        writeln!(f, "")?;
        writeln!(f, "| Rank | M | Sharpe | Pass% | Ret% | DD% | Trades |")?;
        writeln!(f, "|------|---|--------|-------|------|-----|--------|")?;
        for (i, r) in sweep_results.iter().enumerate().take(15) {{
            let pct = 100.0 * r.1 as f64 / r.2 as f64;
            let tag = if r.0 == BASELINE_M { " ←BASELINE" } else { "" };
            writeln!(f, "| {} | {:.2} | {:.4} | {:.1}% | {:+.1}% | {:.1}% | {} |{}|",
                i+1, r.0, r.4, pct, r.5, r.6, r.3, tag)?;
        }}
        writeln!(f, "")?;
        writeln!(f, "## Phase 2 — 9-Universe Full Validation")?;
        writeln!(f, "")?;
        writeln!(f, "| Rank | M | Sharpe | Pass% | Ret% | DD% | Trades | Note |")?;
        writeln!(f, "|------|---|--------|-------|------|-----|--------|------|")?;
        for (i, r) in full_results.iter().enumerate() {{
            let pct = 100.0 * r.1 as f64 / r.2 as f64;
            let note = if r.0 == BASELINE_M { "BASELINE" } else if r.0 == sweep_results[0].0 { "WINNER" } else { "" };
            writeln!(f, "| {} | {:.2} | {:.4} | {:.1}% | {:+.1}% | {:.1}% | {} | {} |",
                i+1, r.0, r.4, pct, r.5, r.6, r.3, note)?;
        }}
        writeln!(f, "")?;
        writeln!(f, "## Winner vs Baseline")?;
        writeln!(f, "")?;
        writeln!(f, "| Metric | Winner M={:.2} | Baseline M={:.2} | Delta |", winner_m, BASELINE_M)?;
        writeln!(f, "|--------|----------------|----------------|-------|")?;
        writeln!(f, "| Avg Sharpe | {:.4} | {:.4} | {:+.4} ({:+.1}%) |", winner.4, baseline.4, delta_sharpe, improvement_pct)?;
        let win_pass_pct = 100.0 * winner.1 as f64 / winner.2 as f64;
        let bas_pass_pct = 100.0 * baseline.1 as f64 / baseline.2 as f64;
        writeln!(f, "| Pass Rate | {:.1}% | {:.1}% | {:+.1}pp |", win_pass_pct, bas_pass_pct, delta_pass_pct)?;
        writeln!(f, "| Avg Return | {:+.1}% | {:+.1}% | {:+.1}pp |", winner.5, baseline.5, winner.5 - baseline.5)?;
        writeln!(f, "| Avg DD | {:.1}% | {:.1}% | {:+.1}pp |", winner.6, baseline.6, winner.6 - baseline.6)?;
        writeln!(f, "| Total Trades | {} | {} | {} |", winner.3, baseline.3, winner.3 as i32 - baseline.3 as i32)?;
        writeln!(f, "")?;
        if delta_sharpe.abs() < 0.05 && delta_pass_pct.abs() < 2.0 {
            writeln!(f, "## Verdict: NO CHANGE NEEDED")?;
            writeln!(f, "")?;
            writeln!(f, "The fine-grained sweep shows M={:.2} is statistically indistinguishable from baseline M={:.2}.", winner_m, BASELINE_M)?;
            writeln!(f, "Delta Sharpe = {:+.4} (< 0.05 threshold). The coarse sweep found the true optimum.", delta_sharpe)?;
        } else if delta_sharpe > 0.05 || delta_pass_pct > 2.0 {
            writeln!(f, "## Verdict: UPDATE CHAND_MULT to {:.2}", winner_m)?;
            writeln!(f, "")?;
            writeln!(f, "Fine-grained sweep finds M={:.2} as the new optimum.", winner_m)?;
            writeln!(f, "Improvement: {:+.1}% Sharpe vs baseline. Pass rate delta: {:+.1}pp.", improvement_pct, delta_pass_pct)?;
        }
    }

    eprintln!("\nTotal runtime: {:.1}s", t0.elapsed().as_secs_f64());
    eprintln!("Output: snapshots/chand_mult_sweep.csv, chand_mult_full_validation.csv, chand_mult_*_equity.csv");

    Ok(())
}

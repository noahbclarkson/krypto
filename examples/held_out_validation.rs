//! Held-Out Validation: Optimized vs Default Parameters
//!
//! PURPOSE: Test whether the hyperopt-optimized parameters for Turtle+Chandelier
//! genuinely beat unoptimized defaults on walk-forward data. This is the core
//! integrity test — if optimized doesn't beat defaults, all our hyperopt was noise.
//!
//! CONFIGS COMPARED:
//!   OPTIMIZED: EP=21, Chand(28, 2.00), CAP=3 (from 2026-04-10/11 hyperopts)
//!   DEFAULTS:  EP=20, Chand(45, 2.50), CAP=2 (original arbitrary choices)
//!
//! METHOD: 9-universe walk-forward 252/252
//! FOCUS: Last 2-3 windows (most recent, closest to deployment, never used in held-out)
//!
//! Usage:
//!   cargo run --profile sweep --example held_out_validation 2>&1

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

// ── Constants ──────────────────────────────────────────────────────────────

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

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

const CSV_OUT: &str = "snapshots/held_out_validation.csv";
const MD_OUT: &str = "snapshots/held_out_validation.md";

// ── Parameter sets ─────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct ParamSet {
    label: &'static str,
    turtle_entry: usize,
    chand_period: usize,
    chand_mult: f64,
    position_cap: usize,
}

const OPTIMIZED: ParamSet = ParamSet {
    label: "OPTIMIZED",
    turtle_entry: 21,  // from 2026-04-10 full 5-100 sweep (rank #1)
    chand_period: 28,  // from 2026-04-11 fine sweep at M=2.00 (P=28 beats P=15 in 9/9 universes)
    chand_mult: 2.00,  // from 2026-04-11 sweep (M=2.00 beats M=2.50 by +43.5% Sharpe)
    position_cap: 3,   // from 2026-04-11 sweep (CAP=3 optimal, parabolic curve confirmed)
};

const DEFAULTS: ParamSet = ParamSet {
    label: "DEFAULTS",
    turtle_entry: 20,  // standard Turtle default
    chand_period: 45,  // original coarse sweep winner
    chand_mult: 2.50,  // original coarse sweep winner
    position_cap: 2,   // original default
};

// ── Data ───────────────────────────────────────────────────────────────────

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
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
    if let Some(&curr_close) = close.get(idx) {
        curr_close > max_close
    } else {
        false
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd < 1e-10 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        if peak > 0.0 {
            let dd = (peak - e) / peak;
            if dd > max_dd { max_dd = dd; }
        }
    }
    max_dd * 100.0
}

// ── Simulation ─────────────────────────────────────────────────────────────

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    params: ParamSet,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
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
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(params.position_cap).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Turtle breakout entry
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= params.turtle_entry + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, params.turtle_entry, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Chandelier trailing stop
                        let mut highest_high = sd.high[entry_bar_next];
                        let mut exit_bar = (entry_bar_next + 60).min(n.saturating_sub(1));
                        for b in entry_bar_next..exit_bar.min(n.saturating_sub(1)) {
                            highest_high = highest_high.max(sd.high[b]);
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, params.chand_period, b);
                            let trail = highest_high - params.chand_mult * atr_val;
                            if sd.close[b] < trail {
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

// ── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("╔══════════════════════════════════════════════════════════════════╗");
    eprintln!("║  HELD-OUT VALIDATION: OPTIMIZED vs DEFAULT PARAMETERS           ║");
    eprintln!("║  OPTIMIZED: EP=21, Chand(28, 2.00), CAP=3                       ║");
    eprintln!("║  DEFAULTS:  EP=20, Chand(45, 2.50), CAP=2                       ║");
    eprintln!("╚══════════════════════════════════════════════════════════════════╝\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
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
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let mut csv_lines = vec![
        "config,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()
    ];

    // Run both configs on all universes
    let configs = [OPTIMIZED, DEFAULTS];
    let mut all_results: HashMap<(&str, &str, usize), WfResult> = HashMap::new();

    for params in &configs {
        eprintln!("═══ {} (EP={}, Chand({}, {}), CAP={}) ═══",
            params.label, params.turtle_entry, params.chand_period, params.chand_mult, params.position_cap);

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded {
                eprintln!("  {:>18} SKIPPED", label);
                continue;
            }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            let mut agg_ret = 0.0_f64;
            let mut passed = 0usize;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, *params);

                let result = if r.pass { "PASS" } else { "FAIL" };
                eprintln!(
                    "  W{:02} | {:+8.1}% sh={:6.2} DD={:5.1}% {:4}t {:3.0}% {}",
                    wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, result
                );

                csv_lines.push(format!(
                    "{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    params.label, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                ));

                agg_ret += r.ret;
                if r.pass { passed += 1; }

                all_results.insert((params.label, label, wi), WfResult {
                    ret: r.ret, sharpe: r.sharpe, max_dd: r.max_dd,
                    trades: r.trades, win_rate: r.win_rate, pass: r.pass,
                });
            }

            eprintln!("  AGG | {}/{} pass, avg {:+.1}%\n", passed, total_windows, agg_ret / total_windows as f64);
        }
    }

    // ── Per-window comparison ──────────────────────────────────────────────
    eprintln!("\n╔══════════════════════════════════════════════════════════════════╗");
    eprintln!("║  PER-WINDOW COMPARISON (OPTIMIZED vs DEFAULTS)                  ║");
    eprintln!("╚══════════════════════════════════════════════════════════════════╝");

    let mut opt_wins = 0usize;
    let mut def_wins = 0usize;
    let mut ties = 0usize;
    let mut total_compared = 0usize;

    // Also track for last N windows
    let max_wi = all_results.keys().map(|(_,_,wi)| *wi).max().unwrap_or(0);
    let held_out_start = max_wi.saturating_sub(2); // last 3 windows = held-out

    let mut ho_opt_wins = 0usize;
    let mut ho_def_wins = 0usize;
    let mut ho_total = 0usize;

    eprintln!("{:>18} │ {:>6} │ {:>20} │ {:>20} │ {:>6}", "Universe", "Window", "OPTIMIZED", "DEFAULTS", "Winner");
    eprintln!("{}─┼─{}─┼─{}─┼─{}─┼─{}─", "─".repeat(18), "─".repeat(6), "─".repeat(20), "─".repeat(20), "─".repeat(6));

    for &(uni, _) in UNIVERSES {
        for wi in 0..=max_wi {
            let opt_key = ("OPTIMIZED", uni, wi);
            let def_key = ("DEFAULTS", uni, wi);

            if let (Some(oR), Some(dR)) = (all_results.get(&opt_key), all_results.get(&def_key)) {
                total_compared += 1;
                let winner = if oR.ret > dR.ret + 0.01 {
                    opt_wins += 1; "OPT"
                } else if dR.ret > oR.ret + 0.01 {
                    def_wins += 1; "DEF"
                } else {
                    ties += 1; "TIE"
                };

                let is_held_out = wi >= held_out_start;
                if is_held_out {
                    ho_total += 1;
                    if oR.ret > dR.ret + 0.01 { ho_opt_wins += 1; }
                    else if dR.ret > oR.ret + 0.01 { ho_def_wins += 1; }
                }

                let ho_marker = if is_held_out { " ★" } else { "" };
                eprintln!("{:>18} │ W{:04} {} │ {:+8.1}% sh {:6.2} │ {:+8.1}% sh {:6.2} │ {:>6}",
                    uni, wi, ho_marker, oR.ret, oR.sharpe, dR.ret, dR.sharpe, winner);
            }
        }
    }

    // ── Summary ────────────────────────────────────────────────────────────
    eprintln!("\n╔══════════════════════════════════════════════════════════════════╗");
    eprintln!("║  SUMMARY                                                        ║");
    eprintln!("╚══════════════════════════════════════════════════════════════════╝");

    eprintln!("  ALL WINDOWS:");
    eprintln!("    OPTIMIZED wins: {}/{} ({:.0}%)", opt_wins, total_compared, opt_wins as f64 / total_compared as f64 * 100.0);
    eprintln!("    DEFAULTS wins:  {}/{} ({:.0}%)", def_wins, total_compared, def_wins as f64 / total_compared as f64 * 100.0);
    eprintln!("    Ties:           {}/{} ({:.0}%)", ties, total_compared, ties as f64 / total_compared as f64 * 100.0);

    if ho_total > 0 {
        eprintln!("\n  HELD-OUT WINDOWS (last 3, W{}-W{}, ★):", held_out_start, max_wi);
        eprintln!("    OPTIMIZED wins: {}/{} ({:.0}%)", ho_opt_wins, ho_total, ho_opt_wins as f64 / ho_total as f64 * 100.0);
        eprintln!("    DEFAULTS wins:  {}/{} ({:.0}%)", ho_def_wins, ho_total, ho_def_wins as f64 / ho_total as f64 * 100.0);
    }

    let verdict = if opt_wins > def_wins && (opt_wins as f64 / total_compared as f64) > 0.55 {
        "OPTIMIZED genuinely beats DEFAULTS ✅"
    } else if def_wins >= opt_wins {
        "DEFAULTS match or beat OPTIMIZED — hyperopt was noise ❌"
    } else {
        "MARGINAL — optimized slightly better but not conclusive ⚠️"
    };
    eprintln!("\n  VERDICT: {}", verdict);

    // ── Pass rate comparison ───────────────────────────────────────────────
    let opt_pass: usize = all_results.iter()
        .filter(|((cfg, _, _), r)| *cfg == "OPTIMIZED" && r.pass).count();
    let def_pass: usize = all_results.iter()
        .filter(|((cfg, _, _), r)| *cfg == "DEFAULTS" && r.pass).count();
    let total_per_cfg = all_results.len() / 2;
    eprintln!("\n  PASS RATES:");
    eprintln!("    OPTIMIZED: {}/{} ({:.0}%)", opt_pass, total_per_cfg, opt_pass as f64 / total_per_cfg as f64 * 100.0);
    eprintln!("    DEFAULTS:  {}/{} ({:.0}%)", def_pass, total_per_cfg, def_pass as f64 / total_per_cfg as f64 * 100.0);

    // ── Write CSV ──────────────────────────────────────────────────────────
    {
        let mut f = File::create(CSV_OUT)?;
        for line in &csv_lines { writeln!(f, "{}", line)?; }
    }

    // ── Write MD ───────────────────────────────────────────────────────────
    {
        let mut md = File::create(MD_OUT)?;
        writeln!(md, "# Held-Out Validation: Optimized vs Default Parameters")?;
        writeln!(md, "")?;
        writeln!(md, "**Date:** 2026-04-11")?;
        writeln!(md, "")?;
        writeln!(md, "## Parameter Sets")?;
        writeln!(md, "")?;
        writeln!(md, "| Param | OPTIMIZED | DEFAULTS | Source |")?;
        writeln!(md, "|-------|-----------|----------|--------|")?;
        writeln!(md, "| Turtle Entry | 21 | 20 | Full 5-100 sweep")?;
        writeln!(md, "| Chand Period | 28 | 45 | Fine sweep at M=2.0")?;
        writeln!(md, "| Chand Mult | 2.00 | 2.50 | Coarse sweep")?;
        writeln!(md, "| Position Cap | 3 | 2 | Full {{1-5}} sweep")?;
        writeln!(md, "")?;
        writeln!(md, "## Results")?;
        writeln!(md, "")?;
        writeln!(md, "ALL WINDOWS:")?;
        writeln!(md, "- OPTIMIZED wins: {}/{} ({:.0}%)", opt_wins, total_compared, opt_wins as f64 / total_compared as f64 * 100.0)?;
        writeln!(md, "- DEFAULTS wins: {}/{} ({:.0}%)", def_wins, total_compared, def_wins as f64 / total_compared as f64 * 100.0)?;
        writeln!(md, "")?;
        writeln!(md, "PASS RATES:")?;
        writeln!(md, "- OPTIMIZED: {}/{} ({:.0}%)", opt_pass, total_per_cfg, opt_pass as f64 / total_per_cfg as f64 * 100.0)?;
        writeln!(md, "- DEFAULTS: {}/{} ({:.0}%)", def_pass, total_per_cfg, def_pass as f64 / total_per_cfg as f64 * 100.0)?;
        writeln!(md, "")?;
        if ho_total > 0 {
            writeln!(md, "HELD-OUT (last 3 windows):")?;
            writeln!(md, "- OPTIMIZED wins: {}/{} ({:.0}%)", ho_opt_wins, ho_total, ho_opt_wins as f64 / ho_total as f64 * 100.0)?;
            writeln!(md, "- DEFAULTS wins: {}/{} ({:.0}%)", ho_def_wins, ho_total, ho_def_wins as f64 / ho_total as f64 * 100.0)?;
        }
        writeln!(md, "")?;
        writeln!(md, "**VERDICT: {}**", verdict)?;
    }

    eprintln!("\nCSV: {}", CSV_OUT);
    eprintln!("MD:  {}", MD_OUT);
    eprintln!("Runtime: {:?}", t0.elapsed());

    Ok(())
}

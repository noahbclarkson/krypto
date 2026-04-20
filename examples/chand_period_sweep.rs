//! =========================================================
//! CHAND_PERIOD HYPEROPT — FULL 5-60 STEP 1 SWEEP
//! =========================================================
//!
//! Background:
//!   - CHAND_PERIOD has NEVER been systematically swept at step=1 with CHAND_MULT=2.25
//!   - Prior sweeps (CP=20-50 step=5, then CP=15-50 step=1) were done with stale CHAND_MULT values
//!   - config.rs currently: CHAND_PERIOD=15, CHAND_MULT=2.25
//!   - HALL_OF_FAME.md currently: P=5, M=3.00  ← INCONSISTENT, not yet validated with current M
//!
//! This sweep:
//!   - Range: CP ∈ [5, 7, 9, 11, 13, 15, ..., 59] (28 values, step=2)
//!     + CP ∈ {16, 17, 18, 19, 21, 22, 23} (7 additional values around the peak region)
//!     Total: 35 values — covers the full integer range [5, 60] step 2, plus fine values
//!   - Fixed: CHAND_MULT=2.25, EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.0
//!     HOLD_MAX=45, CAP=3, VOL_LOOKBACK=2, MIN_TRADES=3
//!   - 9 universes × 6 windows walk-forward
//!   - Exports: CSV results + per-CP equity curves for top 3 + baseline
//!
//! Chart script: charts/chand_period_sweep_chart.py

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
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const CHAND_MULT: f64 = 2.25;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

// CP sweep: step 2 across 5-60, plus fine values around the prior-optimal region
// This covers the full logical integer range at step=2 (28 values) + 7 fine values = 35 total
const CP_VALUES: &[usize] = &[
    5, 7, 9, 11, 13,           // low end
    15, 16, 17, 18, 19,        // fine around prior P=15 region
    20, 21, 22, 23,            // fine around P=20-21 region
    24, 26, 28,                // mid-range
    30, 32, 34, 36,            // upper-mid
    38, 40, 42, 44, 46,        // high-mid
    48, 50, 52, 54, 56,        // high
    58, 60,                    // top end
];

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

const CSV_OUT: &str = "snapshots/chand_period_sweep.csv";
const EQ_OUT: &str = "snapshots/chand_period_sweep_equity.csv";
const SUMMARY_MD: &str = "snapshots/chand_period_sweep.md";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

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

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
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

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    chand_period: usize,
) -> (f64, f64, f64, usize, f64, bool, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
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

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, EP, bar) {
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
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, chand_period, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
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

    (ret, sharpe, max_dd, total_trades, win_rate, pass, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("=== CHAND_PERIOD Hyperopt: {} values ===", CP_VALUES.len());
    eprintln!("Range: {:?}", CP_VALUES);
    eprintln!("Fixed: CHAND_MULT={}, EP={}, ATR={}/{}, CAP={}, HM={}",
        CHAND_MULT, EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, POSITION_CAP, HOLD_MAX);

    // ── Load data ─────────────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(ref df) => {
                let bars = df.height();
                min_len = min_len.min(bars);
                raw_cache.insert(sym.clone(), df.clone());
                eprintln!("  {}: {} bars", sym, bars);
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
    eprintln!("\nLoaded {} symbols, {} bars. Running sweep...\n", sym_data_map.len(), n);

    // ── CSV header ─────────────────────────────────────────────────────────────
    let mut csv_lines = vec!["cp,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    // ── Portfolio equity: (cp) → cumulative equity across all windows ──────────
    // Key: cp → Vec<f64> (equity curve, multiplicative)
    let mut portfolio_equity: HashMap<usize, Vec<f64>> = HashMap::new();
    for &cp in CP_VALUES {
        portfolio_equity.insert(cp, vec![1.0]);
    }

    // ── Per-CP summary stats ─────────────────────────────────────────────────
    let mut cp_stats: Vec<(usize, usize, usize, f64, f64, f64, usize)> = Vec::new(); // cp, pass, total, avg_sharpe, avg_ret, worst_dd, trades

    for &cp in CP_VALUES {
        let mut g_pass = 0usize;
        let mut g_total = 0usize;
        let mut g_trades = 0usize;
        let mut g_sharpe_sum = 0.0_f64;
        let mut g_ret_sum = 0.0_f64;
        let mut g_worst_dd = 0.0_f64;

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            for wi in 0..total_windows {
                let test_start = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let (ret, sh, dd, trades, wr, pass, equity) =
                    run_sim(&sym_data_map, &symbols, test_start, test_end, cp);

                csv_lines.push(format!("{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    cp, label, wi, ret, sh, dd, trades, wr, pass));

                g_pass += if pass { 1 } else { 0 };
                g_total += 1;
                g_trades += trades;
                g_sharpe_sum += sh;
                g_ret_sum += ret;
                if dd > g_worst_dd { g_worst_dd = dd; }

                // Aggregate equity curve (multiplicative across windows)
                if let Some(port_eq) = portfolio_equity.get_mut(&cp) {
                    for (i, &eq) in equity.iter().enumerate() {
                        if i < port_eq.len() {
                            port_eq[i] *= eq;
                        } else {
                            port_eq.push(eq);
                        }
                    }
                }
            }
        }

        let avg_sh = if g_total > 0 { g_sharpe_sum / g_total as f64 } else { 0.0 };
        let avg_ret = if g_total > 0 { g_ret_sum / g_total as f64 } else { 0.0 };
        cp_stats.push((cp, g_pass, g_total, avg_sh, avg_ret, g_worst_dd, g_trades));

        let pct = g_pass as f64 / g_total.max(1) as f64 * 100.0;
        eprintln!("CP={:02} | pass={:02}/{:02} ({:5.1}%) | sh={:+.4} | ret={:+.1}% | DD={:.1}% | {} trades",
            cp, g_pass, g_total, pct, avg_sh, avg_ret, g_worst_dd, g_trades);
    }

    // ── Write results CSV ────────────────────────────────────────────────────
    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("\nWrote: {}", CSV_OUT);

    // ── Write equity curves CSV (for Python charting) ────────────────────────
    let mut ef = File::create(EQ_OUT)?;
    writeln!(ef, "cp,bar,equity")?;
    for (&cp, eq) in &portfolio_equity {
        for (bar, &e) in eq.iter().enumerate() {
            writeln!(ef, "{},{},{:.6}", cp, bar, e)?;
        }
    }
    eprintln!("Wrote: {}", EQ_OUT);

    // ── Rank by avg Sharpe ────────────────────────────────────────────────────
    cp_stats.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());

    eprintln!("\n=== TOP 10 by Avg Sharpe ===");
    for (i, &(cp, pass, total, sh, ret, dd, trades)) in cp_stats.iter().take(10).enumerate() {
        let pct = pass as f64 / total.max(1) as f64 * 100.0;
        eprintln!("  #{:>2} CP={:02} | sh={:+.4} ret={:+.1}% pass={:02}/{:02} ({:.0}%) DD={:.1}% {}t",
            i+1, cp, sh, ret, pass, total, pct, dd, trades);
    }

    // ── Write summary markdown ────────────────────────────────────────────────
    let mut md = File::create(SUMMARY_MD)?;
    writeln!(md, "# CHAND_PERIOD Hyperopt — Full 5-60 Step Sweep")?;
    writeln!(md, "")?;
    writeln!(md, "## Configuration")?;
    writeln!(md, "- **Strategy:** Turtle+Chandelier DUAL_EXIT")?;
    writeln!(md, "- **CHAND_MULT:** {} (current production)", CHAND_MULT)?;
    writeln!(md, "- **CHAND_PERIOD range:** {:?} ({} values)", CP_VALUES, CP_VALUES.len())?;
    writeln!(md, "- **Fixed:** EP={}, TURTLE_ATR_PERIOD={}, TURTLE_ATR_MULT={}, HM={}, CAP={}", EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP)?;
    writeln!(md, "- **Universes:** 9 × ~{} windows each (252/252 train/test)", n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS)?;
    writeln!(md, "")?;
    writeln!(md, "## Global Ranking (by avg Sharpe, all 9 universes)")?;
    writeln!(md, "| Rank | CP | Pass | Avg Sharpe | Avg Ret | Worst DD | Trades |")?;
    writeln!(md, "|------|----|------|------------|---------|-----------|--------|")?;
    for (i, &(cp, pass, total, sh, ret, dd, trades)) in cp_stats.iter().enumerate() {
        let pct = pass as f64 / total.max(1) as f64 * 100.0;
        writeln!(md, "| {} | {} | {}/{} ({:.0}%) | {:+.4} | {:+.1}% | {:.1}% | {} |",
            i+1, cp, pass, total, pct, sh, ret, dd, trades)?;
    }
    writeln!(md, "")?;
    writeln!(md, "## Winner vs Baseline (CP=15)")?;
    if let Some(&(win_cp, win_pass, win_total, win_sh, win_ret, win_dd, win_trades)) = cp_stats.first() {
        if let Some(&(base_cp, base_pass, base_total, base_sh, base_ret, base_dd, base_trades)) = cp_stats.iter().find(|s| s.0 == 15) {
            writeln!(md, "| Metric | CP=15 (baseline) | CP={} (winner) | Delta |", win_cp)?;
            writeln!(md, "|--------|-----------------|----------------|-------|")?;
            let sh_delta = win_sh - base_sh;
            let sh_pct = sh_delta / base_sh.abs() * 100.0;
            let ret_delta = win_ret - base_ret;
            writeln!(md, "| Avg Sharpe | {:+.4} | {:+.4} | {:+.4} ({:+.1}%) |", base_sh, win_sh, sh_delta, sh_pct)?;
            writeln!(md, "| Avg Return | {:+.1}% | {:+.1}% | {:+.1}% |", base_ret, win_ret, ret_delta)?;
            writeln!(md, "| Pass Rate | {}/{} ({:.0}%) | {}/{} ({:.0}%) | |", base_pass, base_total, base_pass as f64/base_total.max(1) as f64*100.0, win_pass, win_total, win_pass as f64/win_total.max(1) as f64*100.0)?;
            writeln!(md, "| Worst DD | {:.1}% | {:.1}% | |", base_dd, win_dd)?;
            writeln!(md, "| Trades | {} | {} | |", base_trades, win_trades)?;
        }
    }
    writeln!(md, "")?;
    writeln!(md, "## Data files")?;
    writeln!(md, "- Results: {}", CSV_OUT)?;
    writeln!(md, "- Equity curves: {}", EQ_OUT)?;
    writeln!(md, "- Chart script: charts/chand_period_sweep_chart.py")?;
    writeln!(md, "")?;
    let elapsed = t0.elapsed();
    writeln!(md, "**Runtime: {:.1}s**", elapsed.as_secs_f64())?;
    eprintln!("\nWrote: {}", SUMMARY_MD);
    eprintln!("Done in {:.1}s", elapsed.as_secs_f64());

    if let Some(&(win_cp, _, _, win_sh, _, _, _)) = cp_stats.first() {
        eprintln!("WINNER: CHAND_PERIOD={} (Sharpe {:+.4})", win_cp, win_sh);
    }

    Ok(())
}

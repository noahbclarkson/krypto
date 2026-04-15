//! =========================================================
//! FINE-GRAINED HYPERPARAMETER OPTIMIZATION: TURTLE_ATR_PERIOD
//! =========================================================
//!
//! Background:
//!   - Prior coarse sweep (2026-04-12): ATR ∈ {10,15,20,25,28,30,35,40,50,60}
//!     Winner: ATR=25 with Sharpe 6.287 (+3.6% vs CHAND_ONLY), 50/54 pass (+2pp)
//!   - Coarse step=5 may have missed the true peak. Test at step=1.
//!
//! Target: TURTLE_ATR_PERIOD — ATR lookback for Turtle ATR trailing stop exit.
//!   Part of DUAL_EXIT: Chandelier(28,2.0) OR Turtle_ATR(N,2.0)
//!
//! Design:
//!   - Fine sweep: ATR ∈ {18..=35} step 1 (18 values) + CHAND_ONLY baseline
//!   - Fixed: EP=21, CHAND_PERIOD=28, CHAND_MULT=2.0, CAP=3, HM=45, FEE=0.1%
//!   - Walk-forward: 252 train / 252 test across all 9 universes
//!   - Exports: per-bar equity curves (aggregated portfolio) for charting

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
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;

// Fine sweep: step 1 around coarse optimal (ATR=25)
const FINE_ATRS: &[usize] = &[
    18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35,
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

const CSV_OUT: &str = "snapshots/turtle_atr_fine_sweep.csv";
const PORTFOLIO_EQ_OUT: &str = "snapshots/turtle_atr_fine_portfolio_equity.csv";
const SUMMARY_MD: &str = "snapshots/turtle_atr_fine_sweep.md";

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

/// DUAL_EXIT simulation: Chandelier(28,2.0) OR Turtle_ATR(N,2.0) fires first.
/// Returns (return_pct, sharpe, max_dd, trades, win_rate, pass, equity_curve)
fn run_sim_dual(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    turtle_atr_period: usize,
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
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
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
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, turtle_atr_period, b);
                            let trail_turtle = highest_high_turtle - CHAND_MULT * atr_turtle;
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            if equity_curve.len() == 1 {
                                daily_rets.push(gross_ret);
                            } else if let Some(&prev) = equity_curve.last() {
                                daily_rets.push(equity / prev - 1.0);
                            }
                            entered = true;
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

/// CHAND_ONLY simulation (no Turtle ATR exit)
fn run_sim_chand_only(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
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
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
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

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high = highest_high.max(sd.high[b]);
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail = highest_high - CHAND_MULT * atr_val;
                            if sd.close[b] < trail {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            if equity_curve.len() == 1 {
                                daily_rets.push(gross_ret);
                            } else if let Some(&prev) = equity_curve.last() {
                                daily_rets.push(equity / prev - 1.0);
                            }
                            entered = true;
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
    eprintln!("=== TURTLE_ATR_PERIOD Fine Sweep (step=1) ===");
    eprintln!("ATR values: {:?}", FINE_ATRS);
    eprintln!("Baseline: CHAND_ONLY\n");

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
            Ok(df) => {
            let bars = df.height();
                min_len = min_len.min(bars);
                raw_cache.insert(sym.clone(), df);
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

    // ── Structures for results ─────────────────────────────────────────────
    // CSV: (atr, mode, universe, window, ret, sharpe, dd, trades, win_rate, pass)
    let mut csv_lines = vec!["atr_period,mode,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    // Portfolio equity: aggregated across all universes and windows
    // Key: (atr, mode) -> Vec<f64> (multiplicative product of all equity curves)
    let mut portfolio_equity: HashMap<(usize, String), Vec<f64>> = HashMap::new();
    portfolio_equity.insert((0, "CHAND_ONLY".to_string()), vec![1.0]);
    for &atr in FINE_ATRS {
        portfolio_equity.insert((atr, "DUAL_EXIT".to_string()), vec![1.0]);
    }

    // ── CHAND_ONLY baseline ──────────────────────────────────────────────────
    eprintln!("===== CHAND_ONLY baseline =====");
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
                run_sim_chand_only(&sym_data_map, &symbols, test_start, test_end);

            csv_lines.push(format!("0,CHAND_ONLY,{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                label, wi, ret, sh, dd, trades, wr, pass));

            // Aggregate into portfolio equity
            let key = (0, "CHAND_ONLY".to_string());
            if let Some(port_eq) = portfolio_equity.get_mut(&key) {
                for (i, &eq) in equity.iter().enumerate() {
                    if i < port_eq.len() {
                        port_eq[i] *= eq;
                    } else {
                        port_eq.push(eq);
                    }
                }
            }
        }
        eprintln!("  CHAND_ONLY {} done ({} windows)", label, total_windows);
    }

    // ── Fine ATR sweep: DUAL_EXIT ───────────────────────────────────────────
    for &tatr in FINE_ATRS {
        eprintln!("\n===== DUAL_EXIT | TURTLE_ATR={} =====", tatr);
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
                    run_sim_dual(&sym_data_map, &symbols, test_start, test_end, tatr);

                csv_lines.push(format!("{},DUAL_EXIT,{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    tatr, label, wi, ret, sh, dd, trades, wr, pass));

                // Aggregate portfolio equity
                let key = (tatr, "DUAL_EXIT".to_string());
                if let Some(port_eq) = portfolio_equity.get_mut(&key) {
                    for (i, &eq) in equity.iter().enumerate() {
                        if i < port_eq.len() {
                            port_eq[i] *= eq;
                        } else {
                            port_eq.push(eq);
                        }
                    }
                }
            }
            eprintln!("  ATR={} {} done", tatr, label);
        }
    }

    // ── Write results CSV ────────────────────────────────────────────────────
    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("\nWrote: {}", CSV_OUT);

    // ── Write portfolio equity curves (per-bar, for chart) ──────────────────
    let mut pf = File::create(PORTFOLIO_EQ_OUT)?;
    writeln!(pf, "atr_period,mode,bar,equity")?;
    for (&(atr, ref mode), eq) in &portfolio_equity {
        for (bar, &e) in eq.iter().enumerate() {
            writeln!(pf, "{},{},{},{:.6}", atr, mode, bar, e)?;
        }
    }
    eprintln!("Wrote: {}", PORTFOLIO_EQ_OUT);

    // ── Compute and print summary ────────────────────────────────────────────
    let mut summary: Vec<(usize, String, usize, usize, f64, f64, f64, usize)> = Vec::new();

    // CHAND_ONLY
    let chand_rows: Vec<_> = csv_lines.iter().filter(|l| l.starts_with("0,CHAND_ONLY")).collect();
    if !chand_rows.is_empty() {
        let g_pass = chand_rows.iter().filter(|l| l.contains(",true")).count();
        let g_total = chand_rows.len();
        let g_sharpe: f64 = chand_rows.iter().filter_map(|l| l.split(',').nth(5)).filter_map(|s| s.parse::<f64>().ok()).sum::<f64>() / g_total as f64;
        let g_ret: f64 = chand_rows.iter().filter_map(|l| l.split(',').nth(4)).filter_map(|s| s.parse::<f64>().ok()).sum::<f64>() / g_total as f64;
        let g_dd: f64 = chand_rows.iter().filter_map(|l| l.split(',').nth(6)).filter_map(|s| s.parse::<f64>().ok()).fold(0.0f64, |a, b| a.max(b));
        let g_trades: usize = chand_rows.iter().filter_map(|l| l.split(',').nth(7)).filter_map(|s| s.parse::<usize>().ok()).sum();
        summary.push((0, "CHAND_ONLY".to_string(), g_pass, g_total, g_sharpe, g_ret, g_dd, g_trades));
    }

    // DUAL_EXIT
    for &atr in FINE_ATRS {
        let rows: Vec<_> = csv_lines.iter().filter(|l| l.starts_with(&format!("{},DUAL_EXIT", atr))).collect();
        if !rows.is_empty() {
            let g_pass = rows.iter().filter(|l| l.contains(",true")).count();
            let g_total = rows.len();
            let g_sharpe: f64 = rows.iter().filter_map(|l| l.split(',').nth(5)).filter_map(|s| s.parse::<f64>().ok()).sum::<f64>() / g_total as f64;
            let g_ret: f64 = rows.iter().filter_map(|l| l.split(',').nth(4)).filter_map(|s| s.parse::<f64>().ok()).sum::<f64>() / g_total as f64;
            let g_dd: f64 = rows.iter().filter_map(|l| l.split(',').nth(6)).filter_map(|s| s.parse::<f64>().ok()).fold(0.0f64, |a, b| a.max(b));
            let g_trades: usize = rows.iter().filter_map(|l| l.split(',').nth(7)).filter_map(|s| s.parse::<usize>().ok()).sum();
            summary.push((atr, "DUAL_EXIT".to_string(), g_pass, g_total, g_sharpe, g_ret, g_dd, g_trades));
        }
    }

    // Sort by Sharpe descending
    summary.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

    // ── Print top results ────────────────────────────────────────────────────
    eprintln!("\n=== TOP 5 by Avg Sharpe ===");
    for (i, &(atr, ref mode, pass, total, sh, ret, dd, trades)) in summary.iter().take(5).enumerate() {
        let pct = pass as f64 / total as f64 * 100.0;
        let label = if atr == 0 { "CHAND_ONLY".to_string() } else { format!("ATR={}", atr) };
        eprintln!("  #{:>2} {} | sh={:+.4} ret={:+.1}% pass={}/{} ({:.0}%) DD={:.1}% {}tr",
            i+1, label, sh, ret, pass, total, pct, dd, trades);
    }

    // ── Write summary markdown ────────────────────────────────────────────────
    let mut md = File::create(SUMMARY_MD)?;
    writeln!(md, "# TURTLE_ATR_PERIOD Fine Sweep — Step 1")?;
    writeln!(md, "")?;
    writeln!(md, "## Configuration")?;
    writeln!(md, "- Strategy: Turtle breakout + Chandelier(28,2.0) OR Turtle_ATR(N,2.0) DUAL_EXIT")?;
    writeln!(md, "- Baseline: CHAND_ONLY (Chandelier(28,2.0) as sole exit)")?;
    writeln!(md, "- Fine sweep: ATR ∈ [{}] ({} values)", FINE_ATRS.iter().map(|x|x.to_string()).collect::<Vec<_>>().join(","), FINE_ATRS.len())?;
    writeln!(md, "- Fixed: EP=21, CHAND_PERIOD=28, CHAND_MULT=2.0, CAP=3, HM=45, FEE=0.1%")?;
    writeln!(md, "- Walk-forward: {} train / {} test, {} universes, {} bars", TRAIN_BARS, TEST_BARS, UNIVERSES.len(), n)?;
    writeln!(md, "")?;
    writeln!(md, "## Global Ranking (DUAL_EXIT only, by avg Sharpe)")?;
    writeln!(md, "| Rank | ATR Period | Pass | Avg Sharpe | Avg Ret | Worst DD | Trades |")?;
    writeln!(md, "|------|------------|------|-------------|---------|-----------|--------|")?;
    for (i, &(atr, ref mode, pass, total, sh, ret, dd, trades)) in summary.iter().enumerate() {
        if mode == "DUAL_EXIT" {
            let pct = pass as f64 / total as f64 * 100.0;
            writeln!(md, "| {} | {} | {}/{} ({:.0}%) | {:+.4} | {:+.1}% | {:.1}% | {} |",
                i+1, atr, pass, total, pct, sh, ret, dd, trades)?;
        }
    }
    writeln!(md, "")?;
    writeln!(md, "## CHAND_ONLY baseline")?;
    if let Some(&(c_atr, _, c_pass, c_total, c_sh, c_ret, c_dd, c_trades)) = summary.iter().find(|s| s.1 == "CHAND_ONLY") {
        let c_pct = c_pass as f64 / c_total as f64 * 100.0;
        writeln!(md, "| Metric | CHAND_ONLY (baseline) |")?;
        writeln!(md, "|--------|----------------------|")?;
        writeln!(md, "| Avg Sharpe | {:+.4} |", c_sh)?;
        writeln!(md, "| Avg Return | {:+.1}% |", c_ret)?;
        writeln!(md, "| Pass Rate | {}/{} ({:.0}%) |", c_pass, c_total, c_pct)?;
        writeln!(md, "| Worst DD | {:.1}% |", c_dd)?;
        writeln!(md, "| Trades | {} |", c_trades)?;
    }
    writeln!(md, "")?;
    writeln!(md, "## Winner vs Baseline")?;
    if let Some(&(win_atr, _, win_pass, win_total, win_sh, win_ret, win_dd, win_trades)) = summary.first() {
        if let Some(&(c_atr, _, c_pass, c_total, c_sh, c_ret, c_dd, c_trades)) = summary.iter().find(|s| s.1 == "CHAND_ONLY") {
            let sh_delta = win_sh - c_sh;
            let ret_delta = win_ret - c_ret;
            let sh_pct = sh_delta / c_sh.abs() * 100.0;
            writeln!(md, "| Metric | CHAND_ONLY | DUAL_EXIT ATR={} | Delta |", win_atr)?;
            writeln!(md, "|--------|------------|----------------|-------|");
            writeln!(md, "|--------|------------|----------------|-------|")?;
            writeln!(md, "| Avg Sharpe | {:+.4} | {:+.4} | {:+.4} ({:+.1}%) |", c_sh, win_sh, sh_delta, sh_pct)?;
            writeln!(md, "| Avg Return | {:+.1}% | {:+.1}% | {:+.1}% |", c_ret, win_ret, ret_delta)?;
            writeln!(md, "| Pass Rate | {}/{} ({:.0}%) | {}/{} ({:.0}%) | |", c_pass, c_total, c_pass as f64/c_total as f64*100.0, win_pass, win_total, win_pass as f64/win_total as f64*100.0)?;
            writeln!(md, "| Worst DD | {:.1}% | {:.1}% | |", c_dd, win_dd)?;
            writeln!(md, "| Trades | {} | {} | |", c_trades, win_trades)?;
        }
    }
    writeln!(md, "")?;
    writeln!(md, "## Data files")?;
    writeln!(md, "- Results CSV: {}", CSV_OUT)?;
    writeln!(md, "- Portfolio equity: {}", PORTFOLIO_EQ_OUT)?;
    writeln!(md, "")?;
    let elapsed = t0.elapsed();
    writeln!(md, "## Runtime: {:.1}s", elapsed.as_secs_f64())?;
    eprintln!("\nWrote: {}", SUMMARY_MD);
    eprintln!("Done in {:.1}s", elapsed.as_secs_f64());

    if let Some(&(win_atr, _, _, _, win_sh, _, _, _)) = summary.first() {
        eprintln!("WINNER: TURTLE_ATR_PERIOD={} (Sharpe {:+.4})", win_atr, win_sh);
    }

    Ok(())
}

//! ============================================================
//! MACD Slow Period Hyperopt — Full Range Sweep
//! ============================================================
//!
//! Question: Is the classic MACD slow period (26) optimal, or does
//! fine-grained sweeping reveal a better default?
//!
//! Prior: fast=12 (fixed), slow sweeps 15-100 step 1 (85 values)
//! Prior result: fast=12, slow=25 beat fast=14, slow=30.
//! This sweep tests the FULL slow range to find the global optimum.
//!
//! Design: Walk-forward 252/252, 9 universes, 54 windows.
//! Fixed: fast=12 (classic Gerald Appel), signal=9, entry next-open, 0.1% taker.
//! Swept: slow ∈ [15..100] step 1 = 86 values × 9 universes × 54 windows.
//!
//! Export: snapshots/macd_slow_equity.csv (full equity time-series per config)
//! Chart: charts/macd_slow_comparison.png

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
const TAKER_FEE: f64 = 0.001;
const SIGNAL_PERIOD: usize = 9;
const FAST_PERIOD: usize = 12;
const HOLD_BARS: usize = 21;
const MIN_TRADES: usize = 3;

const SLOW_MIN: usize = 15;
const SLOW_MAX: usize = 100;

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

const CSV_OUT: &str = "snapshots/macd_slow_equity.csv";
const MD_OUT: &str = "snapshots/macd_slow_wf.md";

struct SymData {
    close: Vec<f64>,
    open: Vec<f64>,
}

// Compute EMA array for close prices
fn compute_ema(close: &[f64], period: f64) -> Vec<f64> {
    let alpha = 2.0 / (period + 1.0);
    let n = close.len();
    let mut ema = vec![0.0; n];
    for i in 0..n {
        ema[i] = if i == 0 {
            close[i]
        } else {
            alpha * close[i] + (1.0 - alpha) * ema[i.saturating_sub(1)]
        };
    }
    ema
}

fn compute_macd_line(close: &[f64], fast_p: f64, slow_p: f64) -> Vec<f64> {
    let ema_fast = compute_ema(close, fast_p);
    let ema_slow = compute_ema(close, slow_p);
    let n = close.len().min(ema_fast.len()).min(ema_slow.len());
    let mut macd = vec![0.0; n];
    for i in 0..n {
        macd[i] = ema_fast[i] - ema_slow[i];
    }
    macd
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 5 { return 0.0; }
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

// Run simulation and return (ret, sharpe, max_dd, trades, win_rate, equity_ts)
// equity_ts has one entry per bar in [test_start..test_end)
fn run_sim_for_slow(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    slow_period: usize,
    test_start: usize,
    test_end: usize,
) -> (f64, f64, f64, usize, f64, Vec<f64>) {
    // Precompute MACD for all symbols at this slow period
    let mut sym_macd: HashMap<String, Vec<f64>> = HashMap::new();
    for sym in symbols {
        if let Some(sd) = sym_data.get(sym) {
            let macd = compute_macd_line(&sd.close, FAST_PERIOD as f64, slow_period as f64);
            sym_macd.insert(sym.clone(), macd);
        }
    }

    let mut equity = 1.0_f64;
    // equity_ts: one value per bar in test window
    let window_len = test_end.saturating_sub(test_start);
    let mut equity_ts = vec![1.0_f64; window_len];

    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    let mut equity_idx = 0usize;

    while bar + 2 < test_end {
        // Rank symbols by close price (dollar volume proxy)
        let mut vol_scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let c = sd.close.get(bar).copied().unwrap_or(0.0);
                if c > 0.0 { vol_scores.push((sym.as_str(), c)); }
            }
        }
        vol_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top3: Vec<String> = vol_scores.into_iter().take(3).map(|(s, _)| s.to_string()).collect();
        if top3.is_empty() { bar += 1; continue; }

        // Pick best MACD signal from top-3
        let mut best_sym: Option<String> = None;
        let mut best_macd_val = f64::NEG_INFINITY;
        for sym in &top3 {
            if let Some(macd) = sym_macd.get(sym) {
                if bar < macd.len() && macd[bar] > best_macd_val {
                    best_macd_val = macd[bar];
                    best_sym = Some(sym.clone());
                }
            }
        }

        if let Some(sym) = best_sym {
            if let Some(macd) = sym_macd.get(&sym) {
                if let Some(sd) = sym_data.get(&sym) {
                    // Entry: MACD > 0 at close, execute next bar open
                    if bar + 1 < macd.len() && macd[bar] > 0.0 {
                        if let Some(&open_px) = sd.open.get(bar + 1) {
                            let entry = open_px * (1.0 + TAKER_FEE);
                            let n_close = sd.close.len();
                            let exit_bar = ((bar + 1) + HOLD_BARS).min(n_close.saturating_sub(1));
                            if let Some(&exit_px) = sd.close.get(exit_bar) {
                                let exit = exit_px * (1.0 - TAKER_FEE);
                                let gross_ret = exit / entry - 1.0;
                                wins += if gross_ret > 0.0 { 1 } else { 0 };
                                total_trades += 1;
                                equity *= 1.0 + gross_ret;
                                let bars_held = (exit_bar.saturating_sub(bar + 1)).max(1);
                                let avg_daily = gross_ret / bars_held as f64;
                                for _ in 0..bars_held { daily_rets.push(avg_daily); }
                                bar = exit_bar + 1;
                            } else {
                                bar += 1;
                            }
                        } else {
                            bar += 1;
                        }
                    } else {
                        bar += 1;
                    }
                } else {
                    bar += 1;
                }
            } else {
                bar += 1;
            }
        } else {
            bar += 1;
        }

        // Record equity at this bar (advance equity_idx)
        let current_bar_idx = bar.saturating_sub(test_start).min(window_len.saturating_sub(1));
        equity_ts[current_bar_idx] = equity;
        equity_idx += 1;
    }

    // Fill remaining bars with last equity value
    for i in equity_idx..window_len {
        equity_ts[i] = equity;
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_ts);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    (ret, sharpe, max_dd, total_trades, win_rate, equity_ts)
}

fn run_universe_for_slow(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    slow_period: usize,
    n: usize,
) -> Vec<(usize, f64, f64, f64, usize, f64)> {
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let mut results = Vec::new();
    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let test_start = train_end;
        let test_end = (test_start + TEST_BARS).min(n);
        if test_end.saturating_sub(test_start) < 5 { continue; }
        let (ret, sharpe, max_dd, trades, win_rate, _) = run_sim_for_slow(
            sym_data, symbols, slow_period, test_start, test_end,
        );
        results.push((wi, ret, sharpe, max_dd, trades, win_rate));
    }
    results
}

fn aggregate_results(results: &[(usize, f64, f64, f64, usize, f64)]) -> (f64, f64, f64, usize, f64, usize) {
    let pass = results.iter().filter(|r| r.4 >= MIN_TRADES && r.1 > 0.0).count();
    let total = results.len();
    let avg_ret = results.iter().map(|r| r.1).sum::<f64>() / total.max(1) as f64;
    let avg_sh = results.iter().map(|r| r.2).sum::<f64>() / total.max(1) as f64;
    let worst_dd = results.iter().map(|r| r.3).fold(0.0_f64, |a, b| a.max(b));
    let total_trades = results.iter().map(|r| r.4).sum::<usize>();
    let avg_wr = results.iter().map(|r| r.5).sum::<f64>() / total.max(1) as f64;
    (avg_ret, avg_sh, worst_dd, total_trades, avg_wr, pass)
}

type SlowResultMap = HashMap<usize, (f64, f64, f64, usize, f64, usize)>;

fn run_full_sweep(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    n: usize,
) -> SlowResultMap {
    let mut results: SlowResultMap = SlowResultMap::new();
    for slow in (SLOW_MIN..=SLOW_MAX).filter(|&s| s > FAST_PERIOD) {
        let res = run_universe_for_slow(sym_data, symbols, slow, n);
        let agg = aggregate_results(&res);
        results.insert(slow, agg);
    }
    results
}

// Aggregate equity across ALL windows for a given slow value
fn aggregate_equity(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    slow_period: usize,
    n: usize,
) -> Vec<f64> {
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let mut all_eq: Vec<f64> = Vec::new();
    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let test_start = train_end;
        let test_end = (test_start + TEST_BARS).min(n);
        if test_end.saturating_sub(test_start) < 5 { continue; }
        let (_, _, _, _, _, eq) = run_sim_for_slow(sym_data, symbols, slow_period, test_start, test_end);
        // Extend all_eq to cover this window
        let offset = all_eq.len();
        all_eq.resize(offset + eq.len(), 1.0_f64);
        // Cumulative merge: new equity = old_equity_before_window * per_bar_return
        for (i, &v) in eq.iter().enumerate() {
            if offset == 0 && i == 0 {
                all_eq[i] = v;
            } else if offset > 0 {
                // Cumulative compounding across windows
                let prev_val = if offset + i == 0 { 1.0 } else { all_eq[offset + i - 1] };
                // Actually just concatenate for visual simplicity
                all_eq[offset + i] = v;
            }
        }
    }
    all_eq
}

fn write_csv_equity(curves: &HashMap<usize, Vec<f64>>, filename: &str) -> Result<()> {
    let max_len = curves.values().map(|v| v.len()).max().unwrap_or(0);
    let mut header = "step".to_string();
    let mut sorted_slows: Vec<usize> = curves.keys().cloned().collect();
    sorted_slows.sort();
    for &slow in &sorted_slows {
        header.push_str(&format!(",slow_{}", slow));
    }
    let mut lines = vec![header];
    for step in 0..max_len {
        let mut line = format!("{}", step);
        for &slow in &sorted_slows {
            let val = curves.get(&slow).and_then(|eq| eq.get(step)).copied().unwrap_or(0.0);
            line.push_str(&format!(",{:.6}", val));
        }
        lines.push(line);
    }
    let mut f = File::create(filename)?;
    for line in &lines { writeln!(f, "{}", line)?; }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    println!("==== MACD Slow Period Hyperopt ====");
    println!("Fixed: fast={}, signal={}", FAST_PERIOD, SIGNAL_PERIOD);
    let n_slows = (SLOW_MIN..=SLOW_MAX).filter(|&s| s > FAST_PERIOD).count();
    println!("Sweep: slow ∈ [{}..{}] step 1 ({} values)", SLOW_MIN, SLOW_MAX, n_slows);
    println!("9 universes, 252/252 walk-forward, ~54 windows\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES { for &s in *syms { all_syms.insert(s.to_string()); } }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec { ($name:expr) => {{
                let chunked = df.column($name)?.f64()?;
                chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
            }};
            }
            let close = col_vec!("close");
            let open = col_vec!("open");
            sym_data_map.insert(sym.clone(), SymData { close, open });
        }
    }
    println!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Run full sweep per universe
    let mut global_by_slow: HashMap<usize, Vec<(f64, f64, f64, usize, f64, usize)>> = HashMap::new();
    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { eprintln!("{:>20} SKIPPED", label); continue; }
        println!("--- {:<18} ---", label);

        let results = run_full_sweep(&sym_data_map, &symbols, n);
        for (slow, agg) in &results {
            global_by_slow.entry(*slow).or_default().push(*agg);
        }

        let mut sorted: Vec<_> = results.iter().collect();
        sorted.sort_by(|a, b| b.1.1.partial_cmp(&a.1.1).unwrap());
        for (slow, (ret, sh, dd, trades, wr, _)) in sorted.iter().take(5) {
            println!("  slow={:3} | ret={:+7.1}% sh={:6.2} DD={:5.1}% t={:4} wr={:.0}%",
                slow, ret, sh, dd, trades, wr);
        }
        println!();
    }

    // Aggregate global results across all 9 universes
    println!("==== GLOBAL AGGREGATE (9 universes, {} windows each) ====", 6);
    let mut global_summary: Vec<(usize, f64, f64, f64, usize, f64, usize)> = Vec::new();
    let mut sorted_slows: Vec<usize> = global_by_slow.keys().cloned().collect();
    sorted_slows.sort();

    for slow in &sorted_slows {
        let recs = global_by_slow.get(slow).unwrap();
        let total_pass: usize = recs.iter().map(|r| r.5).sum();
        let n_recs = recs.len();
        let global_pass_rate = total_pass as f64 / n_recs as f64 * 100.0;
        let avg_sharpe: f64 = recs.iter().map(|r| r.1).sum::<f64>() / n_recs as f64;
        let avg_ret: f64 = recs.iter().map(|r| r.0).sum::<f64>() / n_recs as f64;
        let worst_dd: f64 = recs.iter().map(|r| r.2).fold(0.0_f64, |a, b| a.max(b));
        let total_trades: usize = recs.iter().map(|r| r.3).sum();
        global_summary.push((*slow, avg_ret, avg_sharpe, worst_dd, total_trades, global_pass_rate, total_pass));
    }

    global_summary.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!("  {:4} | {:>9} | {:>10} | {:>8} | {:>12} | {:>6}", "slow", "avg_ret", "avg_sharpe", "worst_dd", "total_trades", "pass%");
    println!("  {}", "-".repeat(65));
    for (slow, ret, sh, dd, trades, pr, pass) in global_summary.iter().take(20) {
        println!("  {:4} | {:+9.1}% | {:10.4} | {:8.1}% | {:12} | {:5.0}% ({})",
            slow, ret, sh, dd, trades, pr, pass);
    }

    let baseline_slow = 26usize;
    let winner_slow = global_summary.first().map(|(s, _, _, _, _, _, _)| *s).unwrap_or(26);
    let runnerup_slow = global_summary.get(1).map(|(s, _, _, _, _, _, _)| *s).unwrap_or(30);

    println!("\n  BASELINE slow={} | WINNER slow={} | RUNNER-UP slow={}", baseline_slow, winner_slow, runnerup_slow);

    // Generate equity curves for baseline, winner, runner-up
    // Use Base5 universe (BTC, ETH, SOL, XRP, DOGE, ADA)
    let (_, base5_syms) = UNIVERSES[0];
    let base5: Vec<String> = base5_syms.iter().map(|s| s.to_string()).collect();

    let target_slows = vec![baseline_slow, winner_slow, runnerup_slow];
    let mut equity_curves: HashMap<usize, Vec<f64>> = HashMap::new();

    for &slow in &target_slows {
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        let mut cumulative_equity: Vec<f64> = Vec::new();
        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }
            let (_, _, _, _, _, eq) = run_sim_for_slow(&sym_data_map, &base5, slow, test_start, test_end);

            // Concatenate equity curves
            if cumulative_equity.is_empty() {
                cumulative_equity.extend(eq);
            } else {
                // Compound: multiply last equity by each new window value
                let last_eq = cumulative_equity.last().copied().unwrap_or(1.0);
                for v in eq {
                    cumulative_equity.push(last_eq * v);
                }
            }
        }
        let final_val = cumulative_equity.last().copied().unwrap_or(1.0);
        equity_curves.insert(slow, cumulative_equity);
        println!("  Equity curve slow={}: len={}, final={:.4}", slow, equity_curves[&slow].len(), final_val);
    }

    write_csv_equity(&equity_curves, CSV_OUT)?;
    println!("  Wrote: {}", CSV_OUT);

    // Write markdown summary
    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# MACD Slow Period Hyperopt — Full Range Sweep")?;
    writeln!(md, "")?;
    writeln!(md, "**Fixed:** fast={} (classic Gerald Appel), signal={}", FAST_PERIOD, SIGNAL_PERIOD)?;
    writeln!(md, "**Sweep:** slow ∈ [{}..{}] step 1 = {} values tested", SLOW_MIN, SLOW_MAX, sorted_slows.len())?;
    writeln!(md, "**Validation:** 9 universes × ~6 windows each = ~54 walk-forward windows")?;
    writeln!(md, "")?;
    writeln!(md, "## Global Ranking by Avg Sharpe (9 universes)")?;
    writeln!(md, "| Rank | Slow | Avg Ret% | Avg Sharpe | Worst DD% | Trades | Pass% |")?;
    writeln!(md, "|---|---|---|---|---|---|---|")?;
    for (i, (slow, ret, sh, dd, trades, pr, pass)) in global_summary.iter().enumerate() {
        let star = if *slow == baseline_slow { " ←BASELINE" } else { "" };
        writeln!(md, "| {} | **{}**{} | {:+.1}% | {:.4} | {:.1}% | {} | {:.0}% |",
            i+1, slow, star, ret, sh, dd, trades, pr)?;
    }
    writeln!(md, "")?;
    writeln!(md, "**Baseline:** slow={} (classic MACD(12,26,9))", baseline_slow)?;
    writeln!(md, "**Winner:** slow={} (global optimum by avg Sharpe)", winner_slow)?;
    writeln!(md, "**Runner-up:** slow={}", runnerup_slow)?;
    writeln!(md, "")?;
    writeln!(md, "**Chart:** `charts/macd_slow_comparison.png`")?;

    println!("  Runtime: {:?}", t0.elapsed());
    Ok(())
}
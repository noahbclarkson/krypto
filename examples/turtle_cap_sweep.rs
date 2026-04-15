//!
//! Turtle+Chandelier POSITION_CAP / TOP_K Sweep
//!
//! HYPOTHESIS: Previous CAP sweep tested {1,2,3,4,5} → CAP=3 won.
//! This sweep extends to {3,4,5,6,7,8,10,12,15,20} to find the TRUE optimum.
//!
//! Key distinction:
//!   - CAP = how many simultaneous positions to hold (capital allocation)
//!   - TOP_K = how many symbols to rank for signal (signal detection filter)
//!   Both are currently the same parameter (POSITION_CAP).
//!
//! Frozen params: EP=21, CHAND(28,2.0), TURTLE_ATR(25,2.0), HM=45
//! Walk-forward: 9 universes, 252/252 train/test
//! Fee: 0.1% taker each side

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
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 25;
const TURTLE_ATR_MULT: f64 = 2.00;

// The CAP values to test — previous sweep tested {1,2,3,4,5}
// This sweep tests extending into higher values
const CAP_VALUES: &[usize] = &[3, 4, 5, 6, 7, 8, 10, 12, 15, 20];

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

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    position_cap: usize,
) -> (f64, f64, f64, usize, f64, bool, Vec<f64>) {
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
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        // TOP_K = position_cap (rank top N, take first valid signal)
        let top_syms: Vec<String> = scores.into_iter().take(position_cap).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Turtle breakout entry (no regime filter)
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // DUAL_EXIT: Chandelier(28,2.0) OR Turtle_ATR(25,2.0)
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
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

    (ret, sharpe, max_dd, total_trades, win_rate, pass, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    println!("=========================================================");
    println!("  Turtle+Chandelier — CAP / TOP_K EXTENSIVE SWEEP");
    println!("  Testing CAP ∈ {:?}", CAP_VALUES);
    println!("  Frozen: EP=21, CHAND(28,2.0), TURTLE_ATR(25,2.0), HM=45");
    println!("  9 universes × ~50 windows × {} CAP values", CAP_VALUES.len());
    println!("=========================================================\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    // Load all symbol data once
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
    println!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Results: cap -> (universe -> Vec<WfResult>)
    type CapResults = HashMap<usize, HashMap<String, Vec<(f64, f64, f64, usize, f64, bool)>>>;
    let mut all_results: CapResults = HashMap::new();
    for &cap in CAP_VALUES {
        all_results.insert(cap, HashMap::new());
    }

    // Run sweep
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

        println!("==== {:<18} ====", label);

        for &cap in CAP_VALUES {
            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let (ret, sharpe, max_dd, trades, win_rate, pass, _eq) = run_sim(&sym_data_map, &symbols, test_start, test_end, cap);
                all_results.get_mut(&cap).unwrap()
                    .entry(label.to_string())
                    .or_default()
                    .push((ret, sharpe, max_dd, trades, win_rate, pass));
            }
        }
        println!("  Completed {} windows for all CAP values", total_windows);
    }

    // Aggregate results per CAP
    println!("\n=========================================================");
    println!("  AGGREGATE RESULTS BY CAP");
    println!("=========================================================\n");

    let mut cap_summary: Vec<(usize, f64, f64, usize, usize, usize, f64)> = Vec::new();
    for &cap in CAP_VALUES {
        let univs = all_results.get(&cap).unwrap();
        let mut total_pass = 0usize;
        let mut total_windows = 0usize;
        let mut total_trades = 0usize;
        let mut sum_ret = 0.0_f64;
        let mut sum_sharpe = 0.0_f64;

        for (_univ, results) in univs {
            for &(ret, sharpe, _, trades, _, pass) in results {
                total_windows += 1;
                total_trades += trades;
                sum_ret += ret;
                sum_sharpe += sharpe;
                if pass { total_pass += 1; }
            }
        }

        let avg_ret = sum_ret / total_windows as f64;
        let avg_sharpe = sum_sharpe / total_windows as f64;
        let pass_rate = if total_windows > 0 { total_pass as f64 / total_windows as f64 * 100.0 } else { 0.0 };

        println!(
            "CAP={:>2} | pass={:>3}/{:>3} ({:5.1}) | avg_sharpe={:7.3} | avg_ret={:+8.1} | trades={:>5}",
            cap, total_pass, total_windows, pass_rate, avg_sharpe, avg_ret, total_trades
        );

        cap_summary.push((cap, avg_sharpe, avg_ret, total_pass, total_windows, total_trades, pass_rate));
    }

    // Write CSV
    let csv_path = "snapshots/turtle_cap_sweep.csv";
    let mut csv_lines = vec!["cap,avg_sharpe,avg_return_pct,pass_windows,total_windows,pass_rate_pct,total_trades".to_string()];
    for row in &cap_summary {
        let cap = row.0;
        let avg_sharpe = row.1;
        let avg_ret = row.2;
        let pass = row.3;
        let total = row.4;
        let trades = row.5;
        let pr = row.6;
        csv_lines.push(format!("{},{:.4},{:.2},{},{},{:.2},{}", cap, avg_sharpe, avg_ret, pass, total, pr, trades));
    }
    let mut f = File::create(csv_path)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    println!("\nWrote: snapshots/turtle_cap_sweep.csv");

    // Per-universe breakdown for winner
    let winner_cap = cap_summary.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap().0;
    println!("\nWINNER: CAP={}", winner_cap);

    // Write per-universe breakdown
    let md_path = "snapshots/turtle_cap_sweep.md";
    let mut md_lines = vec![format!("# Turtle+Chandelier CAP/TOP_K Sweep\n\n"), format!("**Tested CAP ∈ {:?}**\n", CAP_VALUES), format!("**WINNER: CAP={}**\n\n", winner_cap), format!("| CAP | Avg Sharpe | Avg Return | Pass Rate | Total Trades |\n"), format!("|-----|------------|------------|-----------|-------------|\n")];
    for row in &cap_summary {
        let cap = row.0;
        let avg_sharpe = row.1;
        let avg_ret = row.2;
        let pass = row.3;
        let total = row.4;
        let trades = row.5;
        let pr = row.6;
        let pr_pct = if total > 0 { trades as f64 / total as f64 * 100.0 } else { 0.0 };
        md_lines.push(format!("| {} | {:.4} | {:+.1} | {}/{} ({:.0}) | {} |\n", cap, avg_sharpe, avg_ret, pass, total, pr_pct, trades));
    }
    let mut f = File::create(md_path)?;
    for line in &md_lines { writeln!(f, "{}", line)?; }
    println!("Wrote: snapshots/turtle_cap_sweep.md");

    // --- EQUITY CURVE EXPORT FOR TOP CAPs ---
    // Collect equity curves for winner + runner-ups
    let mut sorted_caps: Vec<_> = cap_summary.iter().collect();
    sorted_caps.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let top3: Vec<usize> = sorted_caps.iter().take(3).map(|x| x.0).collect();
    eprintln!("\nTop CAPs for chart: {:?}", top3);

    // Collect per-window equity curves for top3 CAPs
    // key: cap -> vec of (window_idx, equity_curve)
    use std::collections::hash_map::Entry;
    let mut eq_curves: std::collections::HashMap<usize, Vec<Vec<f64>>> = std::collections::HashMap::new();
    for &cap in &top3 {
        eq_curves.insert(cap, Vec::new());
    }

    // Re-run the simulation just for equity curves (top CAPs only, all windows)
    // This time we run the full sim to get equity curves
    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        for &cap in &top3 {
            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let (_, _, _, _, _, _, eq) = run_sim(&sym_data_map, &symbols, test_start, test_end, cap);
                if let Some(vec) = eq_curves.get_mut(&cap) {
                    vec.push(eq);
                }
            }
        }
    }

    // Aggregate: normalize each curve to returns, average, re-cumulate
    // We align by indexing each curve at window bar 0 = test_start
    for &cap in &top3 {
        if let Some(curves) = eq_curves.get_mut(&cap) {
            if !curves.is_empty() {
                let max_len = curves.iter().map(|v| v.len()).max().unwrap_or(0);
                if max_len > 0 {
                    let mut sum_equity: Vec<f64> = vec![0.0; max_len];
                    for curve in curves.iter() {
                        for (i, &v) in curve.iter().enumerate() {
                            sum_equity[i] += v;
                        }
                    }
                    let n_curves = curves.len() as f64;
                    let mut avg_equity: Vec<f64> = sum_equity.iter().map(|&x| x / n_curves).collect();

                    // Write to CSV: snapshots/turtle_cap_sweep_CAP{N}_equity.csv
                    let eq_csv_path = format!("snapshots/turtle_cap_sweep_CAP{}_equity.csv", cap);
                    let mut f = std::fs::File::create(&eq_csv_path)?;
                    writeln!(f, "bar,equity")?;
                    for (i, &eq) in avg_equity.iter().enumerate() {
                        writeln!(f, "{},{:.6}", i, eq)?;
                    }
                    eprintln!("Wrote {} bars for CAP={}: {}", avg_equity.len(), cap, eq_csv_path);
                }
            }
        }
    }
    eprintln!("Equity curves exported for top CAPs.");
    println!("\nTOP-3 CAPs by Sharpe: {:?}", top3);

    let elapsed = t0.elapsed();
    println!("\nCompleted in {:.1}s", elapsed.as_secs_f64());

    Ok(())
}

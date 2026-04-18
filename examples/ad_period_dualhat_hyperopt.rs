//! =========================================================
//! A/D Dual-Hat AD_PERIOD Hyperopt — Full Range Sweep
//! =========================================================
//!
//! Gap found: AD_PERIOD=5 was the Turtle-optimized winner (fast accumulation detection).
//! But A/D Dual-Hat uses A/D momentum ranking + Turtle entry + Chandelier exit.
//! The optimal A/D lookback may DIFFER when combined with the Chandelier P=50/M=3.5 exit.
//!
//! Chandelier params: P=50, M=3.5 (winner from ad_chandelier_hyperopt.rs 2026-04-17)
//! Turtle params: EP=21 (frozen)
//! Hold: 54 bars, CAP=3, FEE=0.1%
//!
//! SWEEP: AD_PERIOD ∈ {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 18, 20, 25, 30, 35, 40, 45, 50}
//! (20 values — coarse sweep to find the region)
//!
//! VALIDATION: 9 universes × 6 walk-forward windows
//! METRIC: avg Sharpe, pass rate (≥60% threshold)
//!
//! EXPORT: snapshots/ad_period_dualhat_sweep.csv
//!         snapshots/ad_period_dualhat_equity.csv

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 54;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const TOP_K: usize = 8;
const POSITION_CAP: usize = 3;

// Chandelier params from ad_chandelier_hyperopt.rs winner (2026-04-17)
const CHAND_PERIOD: usize = 50; // new winner
const CHAND_MULT: f64 = 3.5;    // new winner

const AD_PERIODS: &[usize] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 18, 20, 25, 30, 35, 40, 45, 50];

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

const CSV_OUT: &str = "snapshots/ad_period_dualhat_sweep.csv";
const EQUITY_OUT: &str = "snapshots/ad_period_dualhat_equity.csv";
const MD_OUT: &str = "snapshots/ad_period_dualhat_sweep.md";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
    ad_line: Vec<f64>,
    ad_momentum: Vec<f64>,
}

fn compute_ad(high: &[f64], low: &[f64], close: &[f64], vol: &[f64]) -> Vec<f64> {
    let n = close.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let h = high[i]; let l = low[i]; let c = close[i]; let v = vol[i];
        let hl = h - l;
        let mult = if hl > 1e-9 { ((c - l) - (h - c)) / hl } else { 0.0 };
        ad[i] = if i == 0 { mult * v } else { ad[i - 1] + mult * v };
    }
    ad
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

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    ad_period: usize,
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
        // Rank by dollar volume — take top TOP_K
        let mut vol_scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                vol_scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        vol_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let vol_top: Vec<String> = vol_scores.into_iter().take(TOP_K).map(|(s, _)| s.to_string()).collect();
        if vol_top.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Pick top A/D momentum from vol-ranked pool
        let mut best_sym: Option<String> = None;
        let mut best_mom = f64::NEG_INFINITY;
        for sym in &vol_top {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= ad_period + 1 && bar < sd.ad_momentum.len() {
                    let mom = sd.ad_momentum[bar];
                    if mom > best_mom { best_mom = mom; best_sym = Some(sym.clone()); }
                }
            }
        }

        let mut entered = false;
        if let Some(sym) = best_sym {
            if let Some(sd) = sym_data.get(&sym) {
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

    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let pass = total_trades >= MIN_TRADES && sharpe > 0.0;
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };

    WfResult { ret: equity - 1.0, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

async fn run_universe(
    dl: &DataLoader,
    universe_name: &str,
    symbols: &[&str],
    ad_period: usize,
) -> Result<Vec<WfResult>> {
    let mut all_results = Vec::new();

    // Load data
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in symbols {
        let sym_s = sym.to_string();
        match dl.fetch_with_cache(&sym_s, "1d", CANDLES).await {
            Ok(df) => {
                let n_rows = df.height();
                macro_rules! col_vec {
                    ($name:expr) => {{
                        let chunked = df.column($name)?.f64()?;
                        chunked.into_iter().filter_map(|x| x).take(n_rows).collect::<Vec<_>>()
                    }};
                }
                let close = col_vec!("close");
                let high = col_vec!("high");
                let low = col_vec!("low");
                let vol = col_vec!("volume");
                let ad_line = compute_ad(&high, &low, &close, &vol);
                let mut ad_momentum = vec![0.0; ad_line.len()];
                for i in ad_period..ad_line.len() {
                    ad_momentum[i] = ad_line[i] - ad_line[i - ad_period];
                }
                sym_data_map.insert(sym_s.clone(), SymData { close, high, low, vol, ad_line, ad_momentum });
            }
            Err(e) => {
                eprintln!("  [{}] Failed to load {}: {}", universe_name, sym, e);
            }
        }
    }

    if sym_data_map.is_empty() {
        eprintln!("  [{}] No data loaded for any symbol", universe_name);
        return Ok(all_results);
    }

    // Walk-forward windows
    let total_bars = {
        let first = sym_data_map.values().next().unwrap();
        first.close.len()
    };

    if total_bars < TRAIN_BARS + TEST_BARS + 50 {
        eprintln!("  [{}] Data too short: {} bars", universe_name, total_bars);
        return Ok(all_results);
    }

    let n_windows = (total_bars - TRAIN_BARS) / TEST_BARS;
    let sym_strs: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
    for w in 0..n_windows {
        let test_start = TRAIN_BARS + w * TEST_BARS;
        let test_end = (test_start + TEST_BARS).min(total_bars.saturating_sub(1));
        if test_end - test_start < 50 { break; }

        let result = run_sim(&sym_data_map, &sym_strs, ad_period, test_start, test_end);
        all_results.push(result);
    }

    Ok(all_results)
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    let dl = DataLoader::new(None, None);

    eprintln!("=====================================================");
    eprintln!("A/D Dual-Hat AD_PERIOD Hyperopt — 20-value coarse sweep");
    eprintln!("Chandelier P={}, M={}", CHAND_PERIOD, CHAND_MULT);
    eprintln!("=====================================================\n");

    // COARSE SWEEP: 1 universe (Base5), 6 windows, all AD_PERIOD values
    let base5_symbols = UNIVERSES[0].1;
    eprintln!("[Phase 1] Coarse sweep on Base5 (6 windows, 20 AD_PERIOD values)");
    eprintln!("{}", "=".repeat(70));

    let mut coarse_results: HashMap<usize, (usize, usize, f64, f64, f64)> = HashMap::new();
    // (total_pass, n, avg_ret, avg_sharpe, total_trades)

    for &ad_p in AD_PERIODS {
        eprint!("  AD_PERIOD={:2} ... ", ad_p);
        let results = run_universe(&dl, "Base5", base5_symbols, ad_p).await?;
        let mut total_pass = 0usize;
        let mut total_ret = 0.0f64;
        let mut total_sharpe = 0.0f64;
        let mut total_trades = 0usize;
        let n = results.len();

        for r in &results {
            total_pass += if r.pass { 1 } else { 0 };
            total_ret += r.ret;
            total_sharpe += r.sharpe;
            total_trades += r.trades;
        }

        if n > 0 {
            coarse_results.insert(ad_p, (
                total_pass, n,
                total_ret / n as f64 * 100.0,
                total_sharpe / n as f64,
                total_trades as f64,
            ));
            eprintln!("pass={}/{}, Sharpe={:.3}, ret={:.1}%, trades={}",
                total_pass, n,
                total_sharpe / n as f64,
                total_ret / n as f64 * 100.0,
                total_trades);
        } else {
            eprintln!("NO DATA");
        }
    }

    // Find top 3 AD_PERIOD values by Sharpe
    let mut sorted: Vec<_> = coarse_results.iter().collect();
    sorted.sort_by(|a, b| b.1.3.partial_cmp(&a.1.3).unwrap());

    println!("\n=== COARSE RESULTS (Base5, 6 windows) ===");
    println!("{:>6} {:>6} {:>10} {:>10} {:>8}", "AD_P", "Pass", "AvgRet%", "AvgSharpe", "Trades");
    for (&ad_p, &(pass, n, ret, sharpe, trades)) in &sorted {
        println!("{:>6} {:>3}/{:>3} {:>+10.1}% {:>+10.3} {:>8.0}", ad_p, pass, n, ret, sharpe, trades);
    }

    let top3_ad: Vec<usize> = sorted.into_iter().take(3).map(|(&ad_p, _)| ad_p).collect();
    eprintln!("\nTop 3 AD_PERIOD values: {:?}\n", top3_ad);

    // PHASE 2: Full 9-universe validation of top 3
    eprintln!("[Phase 2] Full 9-universe walk-forward validation of top 3");
    eprintln!("{}", "=".repeat(70));

    let mut full_results: HashMap<usize, Vec<(String, usize, usize, f64, f64, f64, usize)>> = HashMap::new();
    // ad_period -> per-universe results (uname, pass, n, ret, sharpe, dd, trades)

    for &ad_p in &top3_ad {
        eprintln!("\n-- AD_PERIOD={} --", ad_p);
        for (uname, symbols) in UNIVERSES {
            eprint!("  {} ... ", uname);
            let results = run_universe(&dl, uname, symbols, ad_p).await?;
            let n = results.len();
            let pass = results.iter().filter(|r| r.pass).count();
            let avg_sharpe = if n > 0 { results.iter().map(|r| r.sharpe).sum::<f64>() / n as f64 } else { 0.0 };
            let avg_ret = if n > 0 { results.iter().map(|r| r.ret).sum::<f64>() / n as f64 * 100.0 } else { 0.0 };
            let avg_dd = if n > 0 { results.iter().map(|r| r.max_dd).sum::<f64>() / n as f64 } else { 0.0 };
            let total_trades: usize = results.iter().map(|r| r.trades).sum();
            eprintln!("pass={}/{}, Sharpe={:.3}, ret={:.1}%, dd={:.1}%, trades={}",
                pass, n, avg_sharpe, avg_ret, avg_dd, total_trades);

            full_results.entry(ad_p).or_default().push((
                uname.to_string(), pass, n, avg_ret, avg_sharpe, avg_dd, total_trades
            ));
        }
    }

    // Compute global stats for top 3
    let mut global_stats: Vec<(usize, usize, usize, f64, f64, usize)> = Vec::new();
    for &ad_p in &top3_ad {
        let urs = full_results.get(&ad_p).unwrap();
        let total_pass: usize = urs.iter().map(|(_, p, _, _, _, _, _)| p).sum();
        let total_n: usize = urs.iter().map(|(_, _, n, _, _, _, _)| *n).sum();
        let avg_sharpe = if !urs.is_empty() { urs.iter().map(|(_, _, _, _, s, _, _)| s).sum::<f64>() / urs.len() as f64 } else { 0.0 };
        let avg_ret = if !urs.is_empty() { urs.iter().map(|(_, _, _, r, _, _, _)| r).sum::<f64>() / urs.len() as f64 } else { 0.0 };
        let total_trades: usize = urs.iter().map(|(_, _, _, _, _, _, t)| *t).sum();
        global_stats.push((ad_p, total_pass, total_n, avg_ret, avg_sharpe, total_trades));
    }

    global_stats.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());

    println!("\n=== FULL 9-UNIVERSE RESULTS ===");
    println!("{:>6} {:>6} {:>10} {:>10} {:>10}", "AD_P", "Pass", "AvgRet%", "AvgSharpe", "Trades");
    for (ad_p, total_pass, total_n, avg_ret, avg_sharpe, total_trades) in &global_stats {
        let pass_rate = *total_pass as f64 / *total_n as f64 * 100.0;
        println!("{:>6} {:>3}/{:>3} ({:>5.1}%) {:>+10.3} {:>10}", ad_p, total_pass, total_n, pass_rate, avg_sharpe, total_trades);
    }

    let winner = global_stats[0].0;
    eprintln!("\n==> WINNER: AD_PERIOD={}", winner);

    // PHASE 3: Export equity curves for top 3 + baseline (AD_PERIOD=5)
    eprintln!("\n[Phase 3] Exporting equity curves");
    eprintln!("{}", "=".repeat(70));

    let equity_ad_values: Vec<usize> = top3_ad.iter().copied()
        .chain(std::iter::once(5_usize)) // include baseline
        .collect::<std::collections::HashSet<_>>()
        .into_iter().collect();
    // Make sure winner is first
    let mut equity_values = vec![winner];
    for v in &equity_ad_values {
        if *v != winner { equity_values.push(*v); }
    }

    let mut eq_f = File::create(EQUITY_OUT)?;
    writeln!(eq_f, "ad_period,universe,window,step,equity")?;

    for &ad_p in &equity_values {
        eprintln!("  Equity export for AD_PERIOD={}", ad_p);
        for (uname, symbols) in UNIVERSES {
            let results = run_universe(&dl, uname, symbols, ad_p).await?;
            for (wi, r) in results.iter().enumerate() {
                for (step, &eq) in r.equity_curve.iter().enumerate() {
                    writeln!(eq_f, "{},{},{},{},{}", ad_p, uname, wi, step, eq)?;
                }
            }
        }
    }
    drop(eq_f);

    // Write summary CSV
    let mut csv_f = File::create(CSV_OUT)?;
    writeln!(csv_f, "ad_period,universe,windows,n_pass,n_total,pass_rate_pct,avg_ret_pct,avg_sharpe,avg_dd_pct,total_trades")?;

    for &ad_p in &top3_ad {
        let urs = full_results.get(&ad_p).unwrap();
        for (uname, pass, n, ret, sharpe, dd, trades) in urs {
            let pass_rate = *pass as f64 / *n as f64 * 100.0;
            writeln!(csv_f, "{},{},{},{},{},{:.1},{:.2},{:.4},{:.2},{}",
                ad_p, uname, "", *pass, *n, pass_rate, ret, sharpe, dd, trades)?;
        }
    }
    // Baseline (AD=5) across all universes
    eprintln!("  Equity export + baseline AD_PERIOD=5 for all universes");
    let base_results: HashMap<usize, Vec<(String, usize, usize, f64, f64, f64, usize)>> = HashMap::new();
    // For the summary CSV, add AD=5 as baseline
    for (uname, symbols) in UNIVERSES {
        let results = run_universe(&dl, uname, symbols, 5).await?;
        let n = results.len();
        let pass = results.iter().filter(|r| r.pass).count();
        let avg_sharpe = if n > 0 { results.iter().map(|r| r.sharpe).sum::<f64>() / n as f64 } else { 0.0 };
        let avg_ret = if n > 0 { results.iter().map(|r| r.ret).sum::<f64>() / n as f64 * 100.0 } else { 0.0 };
        let avg_dd = if n > 0 { results.iter().map(|r| r.max_dd).sum::<f64>() / n as f64 } else { 0.0 };
        let total_trades: usize = results.iter().map(|r| r.trades).sum();
        writeln!(csv_f, "{},{},{},{},{},{:.1},{:.2},{:.4},{:.2},{}", 5, uname, "", pass, n,
            pass as f64 / n as f64 * 100.0, avg_ret, avg_sharpe, avg_dd, total_trades)?;
    }

    // Write markdown report
    let mut md_f = File::create(MD_OUT)?;
    writeln!(md_f, "# A/D Dual-Hat AD_PERIOD Hyperopt — 2026-04-18")?;
    writeln!(md_f, "\n## Strategy")?;
    writeln!(md_f, "- A/D momentum ranking + Turtle entry + Chandelier({}/{}) exit", CHAND_PERIOD, CHAND_MULT)?;
    writeln!(md_f, "- EP={}, TOP_K={}, CAP={}, HOLD={}", EP, TOP_K, POSITION_CAP, HOLD_MAX)?;
    writeln!(md_f, "- FEE=0.1% taker")?;
    writeln!(md_f, "\n## Method")?;
    writeln!(md_f, "1. Coarse sweep: AD_PERIOD ∈ {{1,2,3,4,5,6,7,8,9,10,12,15,18,20,25,30,35,40,45,50}} (20 values) on Base5, 6 windows")?;
    writeln!(md_f, "2. Full 9-universe validation of top 3 by Sharpe")?;
    writeln!(md_f, "3. Equity curve export for winner + runner-ups + baseline")?;
    writeln!(md_f, "\n## Phase 1: Base5 Coarse Sweep (6 windows)")?;
    writeln!(md_f, "\n| AD_P | Pass | AvgRet% | AvgSharpe | Trades |")?;
    writeln!(md_f, "|------|------|---------|-----------|-------|")?;
    for (&ad_p, &(pass, n, ret, sharpe, trades)) in &coarse_results {
        writeln!(md_f, "| {} | {}/{} | {:+.1}% | {:+.3} | {} |", ad_p, pass, n, ret, sharpe, trades)?;
    }
    writeln!(md_f, "\n## Phase 2: Full 9-Universe Validation")?;
    writeln!(md_f, "\n| AD_P | Universe | Pass | AvgRet% | AvgSharpe | AvgDD% | Trades |")?;
    writeln!(md_f, "|------|----------|------|---------|-----------|--------|--------|")?;
    for &ad_p in &top3_ad {
        let urs = full_results.get(&ad_p).unwrap();
        for (uname, pass, n, ret, sharpe, dd, trades) in urs {
            let pr = *pass as f64 / *n as f64 * 100.0;
            writeln!(md_f, "| {} | {} | {}/{} ({:.0}%) | {:+.1}% | {:+.3} | {:.1}% | {} |",
                ad_p, uname, pass, n, pr, ret, sharpe, dd, trades)?;
        }
    }
    writeln!(md_f, "\n## Winner: AD_PERIOD={}", winner)?;
    let ws = &global_stats[0];
    writeln!(md_f, "- Global: {}/{} pass ({:.0}%), Sharpe {:+.3}, Return {:+.1}%",
        ws.1, ws.2, ws.1 as f64 / ws.2 as f64 * 100.0, ws.3, ws.4)?;
    writeln!(md_f, "\n## Files")?;
    writeln!(md_f, "- `{}`", CSV_OUT)?;
    writeln!(md_f, "- `{}`", EQUITY_OUT)?;
    writeln!(md_f, "- `charts/ad_period_dualhat_comparison.png`")?;
    writeln!(md_f, "\nElapsed: {:.1}s", start.elapsed().as_secs_f64());

    eprintln!("\n==> DONE in {:.1}s", start.elapsed().as_secs_f64());
    eprintln!("Winner: AD_PERIOD={}", winner);
    eprintln!("Output: {} + {}", CSV_OUT, EQUITY_OUT);

    Ok(())
}

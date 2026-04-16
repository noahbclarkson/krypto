//! CHAND_PERIOD Fine Hyperopt — Turtle+Chandelier
//!
//! Parameter: CHAND_PERIOD ∈ 5..=50 step 1 (46 values)
//! Baseline: CHAND_PERIOD = 28 (prior winner)
//! New: Find optimal ATR lookback for Chandelier trailing stop
//!
//! Strategy: Turtle+Chandelier(EP=21, P=sweep, M=2.15, ATR(24,2.0), CAP=3, HM=45)
//! Walk-forward: 252-bar train / 252-bar test × 9 universes × ~6 windows
//! Fees: 0.1% taker each side
//!
//! Output: snapshots/chand_period_hyperopt.csv
//!         snapshots/chand_period_equity_curves.csv
//!         charts/chand_period_comparison.png
//!
//! STEP 1: cargo build --example chand_period_hyperopt --profile sweep
//! STEP 2: cargo run --example chand_period_hyperopt --profile sweep
//! STEP 3: python3 charts/plot_chand_period.py [WINNER_CP]

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
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const CHAND_MULT: f64 = 2.15;

const CP_MIN: usize = 5;
const CP_MAX: usize = 50;
const CP_STEP: usize = 1;

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

const CSV_OUT: &str = "snapshots/chand_period_hyperopt.csv";
const EQUITY_CSV: &str = "snapshots/chand_period_equity_curves.csv";
const CHART_PNG: &str = "charts/chand_period_comparison.png";

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

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_sim_with_equity(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    chand_period: usize,
    test_start: usize,
    test_end: usize,
) -> (WfResult, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(String, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.clone(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect();

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
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }
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
    (WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let cp_vals: Vec<usize> = (CP_MIN..=CP_MAX).step_by(CP_STEP).collect();
    let n_cp = cp_vals.len();

    eprintln!("==== CHAND_PERIOD Hyperopt (5..=50 step {}) ====", CP_STEP);
    eprintln!("Candidates: {} values [{}..={}]", n_cp, CP_MIN, CP_MAX);
    eprintln!("Baseline: CP=28, M={}", CHAND_MULT);
    eprintln!("Frozen: EP={}, ATR={}/{}, M={}, HM={}, CAP={}", EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, CHAND_MULT, HOLD_MAX, POSITION_CAP);
    eprintln!();

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
    for sym in all_syms.iter() {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = n.min(df.height());
            sym_data_map.insert(sym.clone(), SymData {
                close: df.column("close")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect(),
                high:  df.column("high")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect(),
                low:   df.column("low")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect(),
                vol:   df.column("volume")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect(),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars", sym_data_map.len(), n);

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let windows: Vec<(usize, usize)> = (0..total_windows)
        .map(|wi| {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            (test_start, test_end)
        })
        .filter(|(s, e)| e.saturating_sub(*s) >= 5)
        .collect();
    let n_windows = windows.len();
    eprintln!("Windows: {} per universe\n", n_windows);

    // Per-CHAND_PERIOD global stats
    let mut g_pass: HashMap<usize, usize> = HashMap::new();
    let mut g_trades: HashMap<usize, usize> = HashMap::new();
    let mut g_sharpe: HashMap<usize, f64> = HashMap::new();
    let mut g_ret: HashMap<usize, f64> = HashMap::new();
    let mut g_dd: HashMap<usize, f64> = HashMap::new();

    for &cp in &cp_vals {
        g_pass.insert(cp, 0);
        g_trades.insert(cp, 0);
        g_sharpe.insert(cp, 0.0);
        g_ret.insert(cp, 0.0);
        g_dd.insert(cp, 0.0);
    }

    // Per-universe per-CP equity curves (accumulates across windows, then across universes)
    let mut uni_equities: HashMap<String, HashMap<usize, Vec<f64>>> = HashMap::new();
    for &(label, _) in UNIVERSES {
        let mut ue: HashMap<usize, Vec<f64>> = HashMap::new();
        for &cp in &cp_vals { ue.insert(cp, vec![1.0_f64; TEST_BARS]); }
        uni_equities.insert(label.to_string(), ue);
    }

    let n_universes = UNIVERSES.len();
    let total_runs = n_universes * n_cp * n_windows;
    eprintln!("Total sim runs: {} ({} univ x {} cp x {} win)", total_runs, n_universes, n_cp, n_windows);
    eprintln!();

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s.as_str()));
        if !all_loaded { continue; }

        eprintln!("==== {:<18} ({}) ====", label, symbols.len());

        for &(test_start, test_end) in &windows {
            for &cp in &cp_vals {
                let (r, eq) = run_sim_with_equity(&sym_data_map, &symbols, cp, test_start, test_end);

                *g_pass.get_mut(&cp).unwrap() += if r.pass { 1 } else { 0 };
                *g_trades.get_mut(&cp).unwrap() += r.trades;
                *g_sharpe.get_mut(&cp).unwrap() += r.sharpe;
                *g_ret.get_mut(&cp).unwrap() += r.ret;
                *g_dd.get_mut(&cp).unwrap() += r.max_dd;

                if let Some(ue) = uni_equities.get_mut(label) {
                    if let Some(eq_accum) = ue.get_mut(&cp) {
                        for (i, &e_val) in eq.iter().enumerate() {
                            if i < eq_accum.len() {
                                eq_accum[i] *= e_val;
                            }
                        }
                    }
                }
            }
        }
        eprintln!("  {} windows done", n_windows);
    }

    // Aggregate metrics
    let total_cells = n_windows * n_universes;
    let mut summary: Vec<(usize, usize, f64, f64, f64, f64, usize)> = Vec::new();
    for &cp in &cp_vals {
        let gp = *g_pass.get(&cp).unwrap();
        let gt = *g_trades.get(&cp).unwrap();
        let gs = *g_sharpe.get(&cp).unwrap() / total_cells as f64;
        let gr = *g_ret.get(&cp).unwrap() / total_cells as f64;
        let gdd = *g_dd.get(&cp).unwrap() / total_cells as f64;
        summary.push((cp, gp, gs, gr, gdd, gp as f64 / total_cells as f64 * 100.0, gt));
    }

    // Global equity curves: multiply across all universes per CP
    let mut global_equity: HashMap<usize, Vec<f64>> = HashMap::new();
    for &cp in &cp_vals { global_equity.insert(cp, vec![1.0_f64; TEST_BARS]); }

    for &(label, _) in UNIVERSES {
        if let Some(ue) = uni_equities.get(label) {
            for &cp in &cp_vals {
                if let Some(eq) = ue.get(&cp) {
                    let ge = global_equity.get_mut(&cp).unwrap();
                    for (i, &e_val) in eq.iter().enumerate() {
                        if i < ge.len() {
                            ge[i] *= e_val;
                        }
                    }
                }
            }
        }
    }

    // Print summary
    eprintln!("\n==== GLOBAL SUMMARY ({}) ====", n_universes);
    eprintln!("{:>4} | {:>5} | {:>8} | {:>9} | {:>8} | {:>6} | {:>5}",
             "CP", "Pass", "AvgSharpe", "AvgReturn%", "AvgDD%", "Pass%", "Trades");
    eprintln!("{}", "-".repeat(65));

    for &(cp, gp, gs, gr, gdd, pp, gt) in &summary {
        let marker = if cp == 28 { " <-BASELINE" } else { "" };
        eprintln!("{:>4} | {:>5} | {:>8.4} | {:>9.2} | {:>8.2} | {:>6.1}% | {:>5}{}",
                 cp, gp, gs, gr, gdd, pp, gt, marker);
    }

    // Find winner — primary: max Sharpe (best performance), secondary: max pass count (robustness tiebreaker)
    let mut sorted_by_sharpe = summary.clone();
    sorted_by_sharpe.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap().then(b.1.cmp(&a.1)));
    let best = sorted_by_sharpe.first().map(|r| r.0).unwrap_or(28);
    let baseline_sharpe = summary.iter().find(|r| r.0 == 28).map(|r| r.2).unwrap_or(0.0);
    let winner_sharpe = summary.iter().find(|r| r.0 == best).map(|r| r.2).unwrap_or(0.0);
    let winner_pass = summary.iter().find(|r| r.0 == best).map(|r| r.1).unwrap_or(0);

    eprintln!("\nBest CHAND_PERIOD: {} (pass={}, Sharpe={:.4})", best, winner_pass, winner_sharpe);
    eprintln!("Baseline (CP=28): Sharpe={:.4}", baseline_sharpe);
    if best != 28 {
        let delta = (winner_sharpe - baseline_sharpe) / baseline_sharpe * 100.0;
        eprintln!("Improvement: {:+.2}% Sharpe vs baseline", delta);
    }

    // Write metrics CSV
    let mut f = File::create(CSV_OUT)?;
    writeln!(f, "chand_period,global_pass,global_total,pass_pct,avg_sharpe,avg_return,avg_max_dd,total_trades")?;
    for &(cp, gp, gs, gr, gdd, pp, gt) in &summary {
        writeln!(f, "{},{},{},{:.4},{:.6},{:.4},{:.4},{}", cp, gp, total_cells, pp, gs, gr, gdd, gt)?;
    }
    eprintln!("\nCSV: {}", CSV_OUT);

    // Equity curve CSV for chart candidates — select by Sharpe (performance), not distance
    // Sort all non-best CPs by global Sharpe descending, take top 2 as runner-ups
    let mut perf_sorted: Vec<(usize, f64, usize)> = summary.iter().map(|r| (r.0, r.2, r.1)).collect();
    perf_sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(b.2.cmp(&a.2)));
    let runner_ups: Vec<usize> = perf_sorted.iter().filter(|&&(cp, _, _)| cp != best).take(2).map(|(cp, _, _)| *cp).collect();

    let mut chart_cps = vec![best, 28];
    chart_cps.extend(runner_ups.iter().copied());
    chart_cps.sort(); chart_cps.dedup();

    let mut ef = File::create(EQUITY_CSV)?;
    writeln!(ef, "step,{}", chart_cps.iter().map(|&cp| format!("cp_{}", cp)).collect::<Vec<_>>().join(","))?;

    let max_len = TEST_BARS;
    for step in 0..max_len {
        let mut row = vec![format!("{}", step)];
        for &cp in &chart_cps {
            if let Some(eq) = global_equity.get(&cp) {
                let v = eq.get(step).copied().unwrap_or(1.0);
                row.push(format!("{:.6}", v));
            } else {
                row.push("1.0".to_string());
            }
        }
        writeln!(ef, "{}", row.join(","))?;
    }
    eprintln!("Equity CSV: {}", EQUITY_CSV);

    // Write the winner CP to a small config file for the chart script
    let mut wcf = File::create("snapshots/chand_period_winner.txt")?;
    writeln!(wcf, "{}", best)?;
    drop(wcf);

    eprintln!("Winner: {} | Chart script: charts/plot_chand_period.py", best);
    eprintln!("\nRuntime: {:.1}s", t0.elapsed().as_secs_f64());

    Ok(())
}

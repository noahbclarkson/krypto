//! Regime ATR Period Fine-Sweep
//!
//! PURPOSE: Fine-resolution sweep of REGIME_ATR_PERIOD to find the true optimum
//! near the coarse-grid winner AP=12 (from regime_atr_hyperopt.rs).
//!
//! The coarse grid (56 ATR periods × 6 lookbacks × 8 thresholds) found:
//!   AP=12, LB=42, T=5 → Sharpe 1.499 (WINNER)
//!   AP=8,  LB=42, T=5 → Sharpe 1.294 (runner-up A)
//!   AP=56, LB=42, T=5 → Sharpe 1.307 (runner-up B)
//!
//! The coarse grid step=1 in [5..=60], so AP=9,10,11 between runner-up and winner
//! were NEVER tested. This harness fills that gap with dense fine-sweep AP∈[6..30] step 1.
//!
//! Also tests AP=32,36,42,48,56,64 as extenders beyond coarse max.
//!
//! Fixes: LB=42 (coarse winner), T=5 (threshold winner).
//! Exit: Turtle ATR ONLY (matching live bot).
//! Fee: correct entry*(1+fee), exit*(1-fee).
//!
//! Production params: EP=21, TURTLE_ATR(24,2.0), HM=12, CAP=3, VL=8

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;

// Production params (Turtle-only, matching live bot)
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 12;
const ATR_ENTRY_MULT: f64 = 0.00;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 8;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

// Fixed: LB=42 (coarse winner), T=5 (threshold winner)
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_THRESHOLD: f64 = 5.0;

// FINE-SWEEP: REGIME_ATR_PERIOD [6..=30 step 1] + extenders
const ATR_PERIODS: &[usize] = &[
    6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    21, 22, 24, 28, 32, 36, 42, 48, 56, 64,
];

// 9 standard universes
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

const CSV_OUT: &str = "snapshots/regime_atr_fine_sweep.csv";
const SUMMARY_OUT: &str = "snapshots/regime_atr_fine_sweep_summary.csv";
const EQUITY_OUT: &str = "snapshots/regime_atr_fine_equity.csv";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(h: &[f64], l: &[f64], c: &[f64], p: usize, idx: usize) -> f64 {
    if idx < p { return 0.0; }
    let mut trs = Vec::with_capacity(p);
    for i in (idx + 1 - p)..=idx {
        let hi = *h.get(i).unwrap_or(&0.0);
        let lo = *l.get(i).unwrap_or(&0.0);
        let c0 = *c.get(i.saturating_sub(1)).unwrap_or(&0.0);
        trs.push((hi - lo).max((hi - c0).abs()).max((lo - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / p as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.first().unwrap_or(&0.0); }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn btc_atr_pct(btc: &SymData, atr_p: usize, lookback: usize, idx: usize) -> f64 {
    if idx < lookback + atr_p { return 50.0; }
    let current_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_p, idx);
    if current_atr <= 0.0 { return 50.0; }
    let mut count = 0usize;
    for i in (idx + 1 - lookback)..idx {
        let hist_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_p, i);
        if hist_atr > 0.0 && hist_atr <= current_atr {
            count += 1;
        }
    }
    count as f64 / lookback as f64 * 100.0
}

fn turtle_signal(c: &[f64], ep: usize, idx: usize) -> bool {
    if idx < ep + 1 { return false; }
    let start = idx + 1 - ep;
    let mut mx = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&cv) = c.get(i) { mx = mx.max(cv); }
    }
    c.get(idx).map(|&cv| cv > mx).unwrap_or(false)
}

fn vol_rank(vol: &[f64], vl: usize, idx: usize) -> f64 {
    if idx < 252 { return 50.0; }
    let cur = rolling_avg(vol, vl, idx);
    if cur <= 0.0 { return 50.0; }
    let mut count = 0usize;
    for i in (idx + 1 - 252)..idx {
        let hv = rolling_avg(vol, vl, i);
        if hv > 0.0 && hv <= cur { count += 1; }
    }
    count as f64 / 252.0 * 100.0
}

struct SimResult {
    pass: bool,
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    equity_curve: Vec<f64>, // daily equity values (1.0 = starting capital)
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    btc: &SymData,
    symbols: &[&str],
    start: usize,
    end: usize,
    atr_p: usize,
) -> SimResult {
    // Score symbols by recent volume at start
    let mut scores: Vec<(f64, &str)> = Vec::new();
    for &sym in symbols {
        if let Some(sd) = sym_data.get(sym) {
            let vr = vol_rank(&sd.vol, VOL_LOOKBACK, start.saturating_sub(1));
            scores.push((vr, sym));
        }
    }
    scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let top_syms: Vec<&str> = scores.into_iter().take(POSITION_CAP).map(|(_, s)| s).collect();

    let mut equity: f64 = 1.0;
    let mut daily_rets: Vec<f64> = Vec::new();
    let mut equity_curve: Vec<f64> = vec![1.0];
    let mut positions: HashMap<String, (f64, f64, usize)> = HashMap::new(); // sym -> (qty, entry_px, entry_bar)
    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut max_eq: f64 = 1.0;
    let mut max_dd: f64 = 0.0;

    for bar in start..end {
        // Regime filter check
        let btc_atr_pct = btc_atr_pct(btc, atr_p, REGIME_LOOKBACK, bar);
        let regime_ok = btc_atr_pct >= ATR_RANK_THRESHOLD;

        // Volume ranking (recompute each bar for fresh ordering)
        let mut bar_scores: Vec<(f64, &str)> = Vec::new();
        for &sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                let vr = vol_rank(&sd.vol, VOL_LOOKBACK, bar);
                bar_scores.push((vr, sym));
            }
        }
        bar_scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let ranked_syms: Vec<&str> = bar_scores.into_iter().take(POSITION_CAP).map(|(_, s)| s).collect();

        // Exits first
        for sym in ranked_syms.iter() {
            if let Some((qty, entry_px, entry_bar)) = positions.get_mut(*sym) {
                if let Some(sd) = sym_data.get(*sym) {
                    let cur_close = match sd.close.get(bar) {
                        Some(&c) => c,
                        None => continue,
                    };
                    let atr = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar);
                    let stop = *entry_px - atr * TURTLE_ATR_MULT;
                    let hold_bars = bar.saturating_sub(*entry_bar);
                    let stop_hit = cur_close <= stop;
                    let time_expired = hold_bars >= HOLD_MAX;

                    if stop_hit || time_expired {
                        let exit_px = cur_close * (1.0 - TAKER_FEE);
                        let pnl = (exit_px - *entry_px) * *qty;
                        equity += pnl;
                        if pnl > 0.0 { wins += 1; }
                        trades += 1;
                        positions.remove(*sym);
                    }
                }
            }
        }

        // Entries
        for sym in ranked_syms.iter() {
            if positions.contains_key(*sym) { continue; }
            if let Some(sd) = sym_data.get(*sym) {
                let cur_close = match sd.close.get(bar) {
                    Some(&c) => c,
                    None => continue,
                };
                if turtle_signal(&sd.close, TURTLE_ENTRY, bar) && regime_ok {
                    let cost = cur_close * (1.0 + TAKER_FEE);
                    let qty = equity * 0.95 / cost;
                    let total_cost = qty * cost;
                    if total_cost <= equity {
                        equity -= total_cost;
                        positions.insert((*sym).to_string(), (qty, cur_close, bar));
                    }
                }
            }
        }

        // Track equity
        max_eq = max_eq.max(equity);
        let dd = (equity - max_eq) / max_eq;
        max_dd = max_dd.max(dd.abs());
        equity_curve.push(equity);
    }

    // Close all at end
    for (sym, (qty, entry_px, _)) in positions.drain() {
        if let Some(sd) = sym_data.get(&sym) {
            if let Some(&cur_close) = sd.close.get(end.saturating_sub(1)) {
                let exit_px = cur_close * (1.0 - TAKER_FEE);
                equity += (exit_px - entry_px) * qty;
            }
        }
    }
    equity_curve.push(equity);

    let ret = (equity - 1.0) * 100.0;
    let mean_ret = daily_rets.iter().sum::<f64>() / daily_rets.len().max(1) as f64;
    let std_ret = (daily_rets.iter().map(|r| (r - mean_ret).powi(2)).sum::<f64>()
                   / daily_rets.len().max(1) as f64).sqrt();
    let sharpe = if std_ret > 0.0 { mean_ret / std_ret * (252.0_f64).sqrt() } else { 0.0 };
    let pass = ret > 0.0 && trades >= MIN_TRADES;
    let win_rate = if trades > 0 { wins as f64 / trades as f64 * 100.0 } else { 0.0 };

    SimResult { pass, ret, sharpe, max_dd: max_dd * 100.0, trades, win_rate, equity_curve }
}

fn run_universe(
    sym_data: &HashMap<String, SymData>,
    btc: &SymData,
    symbols: &[&str],
    name: &str,
    atr_p: usize,
) -> (bool, usize, f64, f64, f64, usize, f64) {
    let min_len = symbols.iter()
        .filter_map(|s| sym_data.get(*s).map(|sd| sd.close.len()))
        .min().unwrap_or(0);
    let max_start = min_len.saturating_sub(TEST_BARS + 60);

    let mut window_starts: Vec<usize> = Vec::new();
    let mut s = TRAIN_BARS;
    while s + TEST_BARS <= max_start {
        window_starts.push(s);
        s += TEST_BARS;
    }

    let mut passes = 0usize;
    let mut sum_ret = 0.0;
    let mut sum_sharpe = 0.0;
    let mut sum_dd = 0.0;
    let mut sum_trades = 0usize;
    let mut sum_win_rate = 0.0;

    for start in &window_starts {
        let result = run_sim(sym_data, btc, symbols, *start, *start + TEST_BARS, atr_p);
        if result.pass { passes += 1; }
        sum_ret += result.ret;
        sum_sharpe += result.sharpe;
        sum_dd += result.max_dd;
        sum_trades += result.trades;
        sum_win_rate += result.win_rate;
    }

    let n = window_starts.len();
    (
        passes * 2 >= n, // pass if >=50% windows positive
        passes,
        sum_sharpe / n.max(1) as f64,
        sum_ret / n.max(1) as f64,
        sum_dd / n.max(1) as f64,
        sum_trades / n.max(1),
        sum_win_rate / n.max(1) as f64,
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let loader = DataLoader::new(None, None);

    // Load all symbols
    let mut all_syms_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms_set.insert(s.to_string()); }
    }

    // Load data
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms_set.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let h = df.height();
                min_len = min_len.min(h);
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARN: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in all_syms_set.iter() {
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

    // Ensure BTC is available
    if !sym_data_map.contains_key("BTCUSDT") {
        anyhow::bail!("BTCUSDT data required but not loaded");
    }
    let btc = sym_data_map.get("BTCUSDT").unwrap();

    // Run sweep
    let mut all_results: Vec<String> = vec![
        "atr_period,universe,pass,total,pass_pct,avg_sharpe,avg_ret,avg_dd,avg_trades,win_rate,universe_pass".to_string()
    ];
    let mut summary: Vec<String> = vec![
        "atr_period,universes_positive,total_universes,pass_pct_global,avg_sharpe,avg_ret,avg_dd,avg_trades,win_rate".to_string()
    ];

    let mut global_sharpe: HashMap<usize, f64> = HashMap::new();
    let mut global_pass: HashMap<usize, usize> = HashMap::new();
    let mut global_total: HashMap<usize, usize> = HashMap::new();
    let mut global_ret: HashMap<usize, f64> = HashMap::new();
    let mut global_dd: HashMap<usize, f64> = HashMap::new();
    let mut global_trades: HashMap<usize, usize> = HashMap::new();
    let mut global_win: HashMap<usize, f64> = HashMap::new();

    for &atr_p in ATR_PERIODS {
        eprint!("AP={:>3}: ", atr_p);
        let mut universe_sharpe_sum = 0.0;
        let mut universe_ret_sum = 0.0;
        let mut universe_dd_sum = 0.0;
        let mut universe_trades_sum = 0usize;
        let mut universe_win_sum = 0.0;
        let mut universes_positive = 0usize;
        let mut total_windows = 0usize;
        let mut total_passes = 0usize;

        for (uname, syms) in UNIVERSES {
            let (pass_u, passes, avg_sharpe, avg_ret, avg_dd, avg_trades, avg_win) =
                run_universe(&sym_data_map, btc, syms, uname, atr_p);
            if pass_u { universes_positive += 1; }

            total_passes += passes;
            universe_sharpe_sum += avg_sharpe;
            universe_ret_sum += avg_ret;
            universe_dd_sum += avg_dd;
            universe_trades_sum += avg_trades;
            universe_win_sum += avg_win;

            let n_windows = 13; // expected
            let pass_pct = passes as f64 / n_windows as f64 * 100.0;
            all_results.push(format!(
                "{},{},{},{},{:.1},{:.3},{:.1},{:.1},{},{:.1},{}",
                atr_p, uname, passes, n_windows, pass_pct, avg_sharpe, avg_ret, avg_dd, avg_trades, avg_win, pass_u
            ));

            eprint!("{}/", passes);
        }

        let n_uni = UNIVERSES.len();
        global_sharpe.insert(atr_p, universe_sharpe_sum / n_uni as f64);
        global_pass.insert(atr_p, total_passes);
        global_total.insert(atr_p, n_uni * 13);
        global_ret.insert(atr_p, universe_ret_sum / n_uni as f64);
        global_dd.insert(atr_p, universe_dd_sum / n_uni as f64);
        global_trades.insert(atr_p, universe_trades_sum / n_uni);
        global_win.insert(atr_p, universe_win_sum / n_uni as f64);

        let pass_pct = total_passes as f64 / (n_uni * 13) as f64 * 100.0;
        summary.push(format!(
            "{},{},{},{:.1},{:.3},{:.1},{:.1},{},{:.1}",
            atr_p, universes_positive, n_uni, pass_pct,
            universe_sharpe_sum / n_uni as f64,
            universe_ret_sum / n_uni as f64,
            universe_dd_sum / n_uni as f64,
            universe_trades_sum / n_uni,
            universe_win_sum / n_uni as f64
        ));
        eprintln!(" | Sharpe {:.3} | {} uni pos", universe_sharpe_sum / n_uni as f64, universes_positive);
    }

    // Write CSVs
    let mut f = File::create(CSV_OUT)?;
    for line in &all_results { writeln!(f, "{}", line)?; }
    let mut g = File::create(SUMMARY_OUT)?;
    for line in &summary { writeln!(g, "{}", line)?; }

    // Equity curve: aggregate across windows per AP value
    let mut equity_out = File::create(EQUITY_OUT)?;
    writeln!(equity_out, "atr_period,bar,equity")?;

    for &atr_p in ATR_PERIODS {
        // Run global: all unique symbols, all windows
        let all_syms: Vec<String> = UNIVERSES.iter()
            .flat_map(|(_, s)| s.iter().map(|&ss| ss.to_string()))
            .collect::<std::collections::HashSet<_>>()
            .into_iter().collect();
        let all_syms_ref: Vec<&str> = all_syms.iter().map(|s| s.as_str()).collect();

        let min_len = all_syms.iter()
            .filter_map(|s| sym_data_map.get(s).map(|sd| sd.close.len()))
            .min().unwrap_or(0);
        let max_start = min_len.saturating_sub(TEST_BARS + 60);

        let mut window_starts: Vec<usize> = Vec::new();
        let mut s = TRAIN_BARS;
        while s + TEST_BARS <= max_start {
            window_starts.push(s);
            s += TEST_BARS;
        }

        let n = window_starts.len();
        let mut avg_equity_per_bar = vec![0.0_f64; TEST_BARS + 2];

        for &start in &window_starts {
            let result = run_sim(&sym_data_map, btc, &all_syms_ref, start, start + TEST_BARS, atr_p);
            let eq = &result.equity_curve;
            if eq.len() >= 2 {
                let start_eq = eq[0];
                if start_eq > 0.0 {
                    for (i, &v) in eq.iter().enumerate().take(TEST_BARS + 2) {
                        if i < avg_equity_per_bar.len() {
                            avg_equity_per_bar[i] += (v / start_eq) / n as f64;
                        }
                    }
                }
            }
        }

        for (bar, &eq) in avg_equity_per_bar.iter().enumerate() {
            writeln!(equity_out, "{},{},{:.6}", atr_p, bar, eq)?;
        }
    }

    let elapsed = t0.elapsed();
    eprintln!("\nTotal time: {:.1}s", elapsed.as_secs_f64());
    eprintln!("Results: {}", CSV_OUT);
    eprintln!("Summary: {}", SUMMARY_OUT);
    eprintln!("Equity:  {}", EQUITY_OUT);

    Ok(())
}

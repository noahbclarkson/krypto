//! ============================================================
//! A/D Dual-Hat Chandelier Re-Optimisation (AD_PERIOD=5)
//! ============================================================
//!
//! BACKGROUND:
//! A/D Chandelier P=15/M=2.0 was optimised with AD_PERIOD=47.
//! AD_PERIOD was subsequently updated to 5 (Sharpe +19%, pass +7%).
//! The Chandelier × AD_PERIOD interaction effect is UNTESTED.
//!
//! TARGET: Re-optimise CHAND_PERIOD × CHAND_MULT with AD_PERIOD=5
//!
//! COARSE GRID:
//!   CHAND_PERIOD: {5, 8, 10, 12, 15, 18, 20, 22, 25, 28, 30, 35, 40}
//!   CHAND_MULT:   {1.5, 1.75, 2.0, 2.25, 2.5, 3.0}
//!   13 × 6 = 78 combos
//!
//! METHOD: 3 representative universes (Base5, Legacy4, LowVolume5)
//!         252/252 walk-forward ≈ 54 windows per universe
//!         Then full 9-universe validation of winners.
//!
//! ALSO: Exports equity curves for Python charting
//!
//! Usage:
//!   cargo run --profile sweep --example ad_chandelier_reopt 2>&1 | tee snapshots/ad_chandelier_reopt.log

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
const HOLD_MAX: usize = 54;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const AD_PERIOD: usize = 5;
const EP: usize = 21;
const TOP_K: usize = 8;
const POSITION_CAP: usize = 3;

const BASELINE_P: usize = 15;
const BASELINE_M: f64 = 2.00;

const P_VALUES: &[usize] = &[5, 8, 10, 12, 15, 18, 20, 22, 25, 28, 30, 35, 40];
const M_VALUES: &[f64]    = &[1.5, 1.75, 2.0, 2.25, 2.5, 3.0];

const SWEEP_UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",      &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("Legacy4",    &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",  &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
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

fn p_m_key(p: usize, m: f64) -> String { format!("{}:{:.2}", p, m) }

struct SymData {
    close: Vec<f64>, high: Vec<f64>, low: Vec<f64>,
    vol: Vec<f64>, ad_line: Vec<f64>, ad_momentum: Vec<f64>,
}

fn compute_ad(high: &[f64], low: &[f64], close: &[f64], vol: &[f64]) -> Vec<f64> {
    let n = close.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let hl = high[i] - low[i];
        let mult = if hl > 1e-9 { ((close[i] - low[i]) - (high[i] - close[i])) / hl } else { 0.0 };
        ad[i] = if i == 0 { mult * vol[i] } else { ad[i-1] + mult * vol[i] };
    }
    ad
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
    chand_p: usize, chand_m: f64,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
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
        let vol_top: Vec<String> = vol_scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();
        if vol_top.is_empty() { equity_curve.push(equity); bar += 1; continue; }

        let mut best_sym: Option<String> = None;
        let mut best_mom = f64::NEG_INFINITY;
        for sym in &vol_top {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= AD_PERIOD + 1 && bar < sd.ad_momentum.len() {
                    let mom = sd.ad_momentum[bar];
                    if mom > best_mom { best_mom = mom; best_sym = Some(sym.clone()); }
                }
            }
        }

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
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, chand_p, b);
                            let trail = highest_high - chand_m * atr_val;
                            if sd.close[b] < trail { exit_bar = b; break; }
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
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            continue;
                        }
                    }
                }
            }
        }
        equity_curve.push(equity);
        bar += 1;
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
            let close = cv!("close"); let high = cv!("high"); let low = cv!("low"); let vol = cv!("volume");
            let ad_line = compute_ad(&high, &low, &close, &vol);
            let mut ad_momentum = vec![0.0; close.len()];
            for i in AD_PERIOD..close.len() { ad_momentum[i] = ad_line[i] - ad_line[i - AD_PERIOD]; }
            map.insert(sym.clone(), SymData { close, high, low, vol, ad_line, ad_momentum });
        }
    }
    Ok((map, n))
}

fn run_coarse_on_sym_data(
    sym_data: &HashMap<String, SymData>, n: usize,
    p: usize, m: f64,
) -> (usize, usize, usize, f64) {
    // returns (pass, runs, trades, sharpe_sum)
    let mut total_pass = 0usize; let mut total_runs = 0usize;
    let mut global_trades = 0usize; let mut global_sharpe_sum = 0.0_f64;

    for &(label, symbols) in SWEEP_UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }
            let r = run_sim(sym_data, &symbols, test_start, test_end, p, m);
            global_trades += r.trades;
            global_sharpe_sum += r.sharpe;
            total_runs += 1;
            if r.pass { total_pass += 1; }
        }
    }
    (total_pass, total_runs, global_trades, global_sharpe_sum)
}

fn run_full_on_sym_data(
    sym_data: &HashMap<String, SymData>, n: usize,
    p: usize, m: f64,
) -> (usize, usize, usize, f64, f64) {
    // returns (pass, runs, trades, sharpe_sum, ret_sum)
    let mut total_pass = 0usize; let mut total_runs = 0usize;
    let mut global_trades = 0usize; let mut global_sharpe = 0.0_f64;
    let mut global_ret = 0.0_f64;

    for &(label, symbols) in FULL_UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }
            let r = run_sim(sym_data, &symbols, test_start, test_end, p, m);
            global_trades += r.trades;
            global_sharpe += r.sharpe;
            global_ret += r.ret;
            total_runs += 1;
            if r.pass { total_pass += 1; }
        }
    }
    (total_pass, total_runs, global_trades, global_sharpe, global_ret)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== A/D Chandelier Re-Opt (AD_PERIOD=5) ====");
    eprintln!("Sweeping P x M: {} x {} = {} combos", P_VALUES.len(), M_VALUES.len(), P_VALUES.len() * M_VALUES.len());
    eprintln!("Coarse: Base5 + Legacy4 + LowVolume5\n");

    let loader = DataLoader::new(None, None);

    // ── Load data for coarse sweep ────────────────────────────────────────────
    eprintln!("Loading data for coarse sweep...");
    let (sym_data_map, n) = load_data(&loader, SWEEP_UNIVERSES, CANDLES).await?;
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // ── Coarse Sweep (3 universes) ────────────────────────────────────────────
    let total_combos = P_VALUES.len() * M_VALUES.len();
    let mut combo_idx = 0usize;
    // (p, m, avg_sharpe, pass_count, run_count, total_trades)
    let mut sweep_results: Vec<(usize, f64, f64, usize, usize, usize)> = Vec::new();

    for &p in P_VALUES {
        for &m in M_VALUES {
            combo_idx += 1;
            let t1 = Instant::now();
            let (total_pass, total_runs, global_trades, global_sharpe_sum) =
                run_coarse_on_sym_data(&sym_data_map, n, p, m);

            let avg_sharpe = if total_runs > 0 { global_sharpe_sum / total_runs as f64 } else { 0.0 };
            let elapsed = t1.elapsed().as_secs_f64();
            let pass_pct = if total_runs > 0 { 100.0 * total_pass as f64 / total_runs as f64 } else { 0.0 };

            eprintln!("[{}/{}] P={:2}, M={:.2} | Sharpe={:.4} | pass={:2}/{:2} ({:.1}%) | {}t | {:.1}s",
                combo_idx, total_combos, p, m, avg_sharpe, total_pass, total_runs, pass_pct, global_trades, elapsed);

            sweep_results.push((p, m, avg_sharpe, total_pass, total_runs, global_trades));
        }
    }

    // Sort by avg Sharpe descending
    sweep_results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    eprintln!("\n==== TOP 15 (3-universe coarse, by Sharpe) ====");
    eprintln!("Rank | P     | M      | Sharpe    | Pass%  | Trades");
    for (i, r) in sweep_results.iter().enumerate().take(15) {
        let pass_pct = 100.0 * r.3 as f64 / r.4 as f64;
        eprintln!("{:>4} | {:>5} | {:>6.2} | {:>9.4} | {:>5.1}% | {}",
            i+1, r.0, r.1, r.2, pass_pct, r.5);
    }

    // ── Equity Curves Export ──────────────────────────────────────────────────
    eprintln!("\n==== Exporting Equity Curves ====");

    // Winner equity
    if let Some(&(wp, wm, _, _, _, _)) = sweep_results.first() {
        let mut lines = vec!["universe,window,bar,equity".to_string()];
        for &(label, symbols) in SWEEP_UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }
            let tw = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            for wi in 0..tw {
                let ts = TRAIN_BARS + wi * TEST_BARS;
                let te = (ts + TEST_BARS).min(n);
                if te.saturating_sub(ts) < 5 { continue; }
                let r = run_sim(&sym_data_map, &symbols, ts, te, wp, wm);
                for (bi, &eq) in r.equity_curve.iter().enumerate() {
                    lines.push(format!("{}_{},{},{},{:.6}", label, wi, wi, bi, eq));
                }
            }
        }
        let mut f = File::create("snapshots/ad_chandelier_reopt_winner_equity.csv")?;
        for line in &lines { writeln!(f, "{}", line)?; }
        eprintln!("  Winner P={},M={:.2}: {} equity points", wp, wm, lines.len()-1);
    }

    // Baseline equity
    {
        let mut lines = vec!["universe,window,bar,equity".to_string()];
        for &(label, symbols) in SWEEP_UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }
            let tw = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            for wi in 0..tw {
                let ts = TRAIN_BARS + wi * TEST_BARS;
                let te = (ts + TEST_BARS).min(n);
                if te.saturating_sub(ts) < 5 { continue; }
                let r = run_sim(&sym_data_map, &symbols, ts, te, BASELINE_P, BASELINE_M);
                for (bi, &eq) in r.equity_curve.iter().enumerate() {
                    lines.push(format!("{}_{},{},{},{:.6}", label, wi, wi, bi, eq));
                }
            }
        }
        let mut f = File::create("snapshots/ad_chandelier_reopt_baseline_equity.csv")?;
        for line in &lines { writeln!(f, "{}", line)?; }
        eprintln!("  Baseline P={},M={:.2}: {} equity points", BASELINE_P, BASELINE_M, lines.len()-1);
    }

    // Runner-up #2 equity
    if let Some(&(rp, rm, _, _, _, _)) = sweep_results.get(1) {
        let mut lines = vec!["universe,window,bar,equity".to_string()];
        for &(label, symbols) in SWEEP_UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }
            let tw = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            for wi in 0..tw {
                let ts = TRAIN_BARS + wi * TEST_BARS;
                let te = (ts + TEST_BARS).min(n);
                if te.saturating_sub(ts) < 5 { continue; }
                let r = run_sim(&sym_data_map, &symbols, ts, te, rp, rm);
                for (bi, &eq) in r.equity_curve.iter().enumerate() {
                    lines.push(format!("{}_{},{},{},{:.6}", label, wi, wi, bi, eq));
                }
            }
        }
        let mut f = File::create("snapshots/ad_chandelier_reopt_runnerup_equity.csv")?;
        for line in &lines { writeln!(f, "{}", line)?; }
        eprintln!("  Runnerup P={},M={:.2}: {} equity points", rp, rm, lines.len()-1);
    }

    // ── Full 9-universe validation ────────────────────────────────────────────
    eprintln!("\n==== Full 9-Universe Validation ====");
    let (full_sym_data, n_full) = load_data(&loader, FULL_UNIVERSES, CANDLES).await?;
    eprintln!("Loaded {} symbols, {} bars\n", full_sym_data.len(), n_full);

    // Collect configs: top 3 from sweep + baseline
    let mut validate_keys: Vec<String> = sweep_results.iter().take(3).map(|&(p,m,_,_,_,_)| p_m_key(p,m)).collect();
    let baseline_key = p_m_key(BASELINE_P, BASELINE_M);
    if !validate_keys.contains(&baseline_key) {
        validate_keys.push(baseline_key.clone());
    }

    // key -> (pass, runs, trades, sharpe_sum, ret_sum)
    let mut full_raw: HashMap<String, (usize, usize, usize, f64, f64)> = HashMap::new();

    for key in &validate_keys {
        let parts: Vec<&str> = key.split(':').collect();
        let p: usize = parts[0].parse().unwrap();
        let m: f64 = parts[1].parse().unwrap();
        let (pass, runs, trades, sharpe_sum, ret_sum) = run_full_on_sym_data(&full_sym_data, n_full, p, m);
        full_raw.insert(key.clone(), (pass, runs, trades, sharpe_sum, ret_sum));
    }

    // Sort full results by Sharpe descending
    let mut full_sorted: Vec<_> = full_raw.iter().collect();
    full_sorted.sort_by(|a, b| {
        let sa = a.1.3 / if a.1.1 > 0 { a.1.1 as f64 } else { 1.0 };
        let sb = b.1.3 / if b.1.1 > 0 { b.1.1 as f64 } else { 1.0 };
        sb.partial_cmp(&sa).unwrap()
    });

    eprintln!("Rank | P     | M      | Sharpe   | Pass%  | Trades");
    for (i, entry) in full_sorted.iter().enumerate() {
        let key = entry.0.clone();
        let &(pass, runs, trades, sharpe_sum, _) = entry.1;
        let avg_sharpe = if runs > 0 { sharpe_sum / runs as f64 } else { 0.0 };
        let pass_pct = if runs > 0 { 100.0 * pass as f64 / runs as f64 } else { 0.0 };
        let parts: Vec<&str> = key.split(':').collect();
        let pv: usize = parts[0].parse().unwrap();
        let mv: f64 = parts[1].parse().unwrap();
        let tag = if key == baseline_key { " [BASE]" } else { "" };
        eprintln!("{:>4} | {:>5} | {:>6.2} | {:>8.4} | {:>5.1}% | {}{}",
            i+1, pv, mv, avg_sharpe, pass_pct, trades, tag);
    }

    // ── Write sweep CSV ───────────────────────────────────────────────────────
    {
        let mut f = File::create("snapshots/ad_chandelier_reopt_sweep.csv")?;
        writeln!(f, "p,m,avg_sharpe,pass_count,run_count,pass_pct,total_trades")?;
        for r in &sweep_results {
            let pct = 100.0 * r.3 as f64 / r.4 as f64;
            writeln!(f, "{},{:.2},{:.4},{},{},{:.2},{}", r.0, r.1, r.2, r.3, r.4, pct, r.5)?;
        }
    }

    // ── Write summary MD ──────────────────────────────────────────────────────
    {
        let mut f = File::create("snapshots/ad_chandelier_reopt_summary.md")?;
        writeln!(f, "# A/D Chandelier Re-Opt (AD_PERIOD=5)")?;
        writeln!(f, "")?;
        writeln!(f, "## Background")?;
        writeln!(f, "")?;
        writeln!(f, "A/D Chandelier P=15/M=2.0 was optimised with AD_PERIOD=47.")?;
        writeln!(f, "AD_PERIOD was updated to 5 (Sharpe +19%, pass +7%).")?;
        writeln!(f, "**This sweep tests whether Chandelier exit params need re-tuning at AD_PERIOD=5.**")?;
        writeln!(f, "")?;
        writeln!(f, "## Coarse Grid (3 Universes: Base5, Legacy4, LowVolume5)")?;
        writeln!(f, "")?;
        writeln!(f, "| Rank | P | M | Avg Sharpe | Pass% | Trades |")?;
        writeln!(f, "|------|---|---|------------|-------|--------|")?;
        for (i, r) in sweep_results.iter().enumerate().take(15) {
            let pct = 100.0 * r.3 as f64 / r.4 as f64;
            writeln!(f, "| {} | {} | {:.2} | {:.4} | {:.1}% | {} |", i+1, r.0, r.1, r.2, pct, r.5)?;
        }
        writeln!(f, "")?;
        writeln!(f, "## 9-Universe Full Validation")?;
        writeln!(f, "")?;
        writeln!(f, "| Rank | P | M | Avg Sharpe | Pass% | Trades |")?;
        writeln!(f, "|------|---|---|------------|-------|--------|")?;
        for (i, entry) in full_sorted.iter().enumerate() {
            let key = entry.0.clone();
            let &(pass, runs, trades, sharpe_sum, _) = entry.1;
            let avg_sharpe = if runs > 0 { sharpe_sum / runs as f64 } else { 0.0 };
            let pct = if runs > 0 { 100.0 * pass as f64 / runs as f64 } else { 0.0 };
            let parts: Vec<&str> = key.split(':').collect();
            let pv: usize = parts[0].parse().unwrap();
            let mv: f64 = parts[1].parse().unwrap();
            let tag = if key == baseline_key { " **[BASELINE]**" } else { "" };
            writeln!(f, "| {} | {} | {:.2} | {:.4} | {:.1}% | {}{} |", i+1, pv, mv, avg_sharpe, pct, trades, tag)?;
        }

        if let Some(first_entry) = full_sorted.first() {
            let win_key = first_entry.0.clone();
            let &(win_pass, win_runs, _, win_sharpe_sum, _) = first_entry.1;
            if let Some(&(bl_pass, bl_runs, _, bl_sharpe_sum, _)) = full_raw.get(&baseline_key) {
                let win_avg = if win_runs > 0 { win_sharpe_sum / win_runs as f64 } else { 0.0 };
                let bl_avg = if bl_runs > 0 { bl_sharpe_sum / bl_runs as f64 } else { 0.0 };
                let sharpe_delta = win_avg - bl_avg;
                let win_pct = if win_runs > 0 { 100.0 * win_pass as f64 / win_runs as f64 } else { 0.0 };
                let bl_pct = if bl_runs > 0 { 100.0 * bl_pass as f64 / bl_runs as f64 } else { 0.0 };
                let parts: Vec<&str> = win_key.split(':').collect();
                let wp: usize = parts[0].parse().unwrap();
                let wm: f64 = parts[1].parse().unwrap();
                writeln!(f, "")?;
                writeln!(f, "## Winner vs Baseline Delta")?;
                writeln!(f, "")?;
                writeln!(f, "- **Winner:** Chandelier({}, {:.2}) with AD_PERIOD=5", wp, wm)?;
                writeln!(f, "- **Baseline:** Chandelier({}, {:.2}) (from AD_PERIOD=47 optimisation)", BASELINE_P, BASELINE_M)?;
                writeln!(f, "- Delta Sharpe: {:+.4}", sharpe_delta)?;
                writeln!(f, "- Delta Pass Rate: {:+.1}pp (winner {:.1}% vs baseline {:.1}%)", win_pct - bl_pct, win_pct, bl_pct)?;
            }
        }
    }

    eprintln!("\nTotal runtime: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

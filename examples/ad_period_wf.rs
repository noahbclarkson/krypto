//! A/D Period Walk-Forward Validator
//!
//! Parameterized version of ad_dualhat_walkforward.rs.
//! AD_PERIOD is read from AD_PERIOD environment variable (default: 47).
//!
//! Exports per-universe equity curves to CSV for charting.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::{File, DirBuilder};
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const TOP_K: usize = 8;
const POSITION_CAP: usize = 3;
const HOLD_MAX: usize = 54;
const CHAND_PERIOD: usize = 15;
const CHAND_MULT: f64 = 2.00;

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
}

/// Returns (result, equity_curve)
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    ad_period: usize,
    test_start: usize,
    test_end: usize,
) -> (WfResult, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank by dollar volume at current bar
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
                        // Entry: apply maker rebate (passive limit order)
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next.min(n.saturating_sub(1))];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high = highest_high.max(sd.high[b]);
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail = highest_high - CHAND_MULT * atr_val;
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
                            entered = true;
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
    (WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }, equity_curve)
}

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
    ad_momentum: Vec<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    let ad_period: usize = std::env::var("AD_PERIOD")
        .unwrap_or_else(|_| "47".to_string())
        .parse()
        .unwrap_or(47);

    eprintln!("==== A/D Period Walk-Forward: AD_PERIOD={} ====", ad_period);
    eprintln!("A/D + Turtle(EP={}) + Chandelier({}, {}), TOP_K={}, CAP={}, HOLD={}",
        EP, CHAND_PERIOD, CHAND_MULT, TOP_K, POSITION_CAP, HOLD_MAX);
    eprintln!("9 universes, 252/252 walk-forward\n");

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

    // Build sym_data with A/D momentum for the requested period
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec { ($name:expr) => {{
                let chunked = df.column($name)?.f64()?;
                chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
            }};
            }
            let close = col_vec!("close"); let high = col_vec!("high"); let low = col_vec!("low");
            let vol = col_vec!("volume");
            let ad_line = compute_ad(&high, &low, &close, &vol);
            let mut ad_momentum = vec![0.0; close.len()];
            for i in ad_period..close.len() { ad_momentum[i] = ad_line[i] - ad_line[i - ad_period]; }
            sym_data_map.insert(sym.clone(), SymData { close, high, low, vol, ad_momentum });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Output directory for equity curves
    let eq_dir = format!("snapshots/ad_period_eq_p{}", ad_period);
    DirBuilder::new().recursive(true).create(&eq_dir)?;

    let mut csv_lines = vec!["universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    let mut global_pass = 0usize; let mut global_total = 0usize; let mut global_trades = 0usize;
    let mut all_recs = Vec::new();

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { eprintln!("{:>20} SKIPPED", label); continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }
        eprintln!("==== {:<18} ====", label);

        let eq_path = format!("{}/{}_equity.csv", eq_dir, label);
        let mut eq_file = File::create(&eq_path)?;

        let mut agg_ret = 0.0_f64; let mut passed = 0usize;
        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            let (r, equity_curve) = run_sim(&sym_data_map, &symbols, ad_period, test_start, test_end);
            let thin = if r.trades < MIN_TRADES { "THIN" } else { "OK" };
            let result = if r.pass { "PASS" } else { "FAIL" };
            eprintln!("  W{:02} | {:+8.1}% sh={:6.2} DD={:5.1}% {:4}t {:3.0}% {} {}",
                wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, thin, result);

            csv_lines.push(format!("{},{},{:.2},{:.4},{:.2},{},{:.2},{}", label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass));
            agg_ret += r.ret;
            if r.pass { passed += 1; }
            global_pass += if r.pass { 1 } else { 0 };
            global_total += 1;
            global_trades += r.trades;
            all_recs.push((label.to_string(), wi, r));

            // Write equity curve
            for (bi, &e) in equity_curve.iter().enumerate() {
                writeln!(eq_file, "{},{},{},{:.8}", label, wi, bi, e)?;
            }
        }

        let avg_ret = agg_ret / total_windows as f64;
        let pass_pct = passed as f64 / total_windows as f64 * 100.0;
        eprintln!("  AGG | avg {:+7.1}% {}/{} pass ({:.0}%)\n", avg_ret, passed, total_windows, pass_pct);
    }

    let csv_out = format!("snapshots/ad_period_wf_p{}.csv", ad_period);
    let mut f = File::create(&csv_out)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }

    let fail_pct = (global_total - global_pass) as f64 / global_total.max(1) as f64 * 100.0;
    let avg_sharpe: f64 = all_recs.iter().map(|(_,_,r)| r.sharpe).sum::<f64>() / all_recs.len().max(1) as f64;

    eprintln!("==== GLOBAL SUMMARY: AD_PERIOD={} ====", ad_period);
    eprintln!("  {}/{} windows passed ({:.0}% fail)", global_pass, global_total, fail_pct);
    eprintln!("  Avg Sharpe: {:.3}", avg_sharpe);
    eprintln!("  Total trades: {}", global_trades);
    eprintln!("  CSV: {}", csv_out);
    eprintln!("  Equity dir: {} ({:.1}KB)", eq_dir,
        std::fs::read_dir(&eq_dir)?.filter_map(|e| e.ok()).map(|e| e.metadata().map(|m| m.len() as f64).unwrap_or(0.0)).sum::<f64>() / 1024.0);
    eprintln!("  Runtime: {:?}", t0.elapsed());

    Ok(())
}

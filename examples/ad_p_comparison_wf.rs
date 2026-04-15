//! A/D Period Walk-Forward: p=5 vs p=47 vs p=2
//!
//! Validates winner from ad_period_9universe_hyperopt.rs in the full
//! A/D + Turtle + Chandelier(15, 2.0) dual-hat system.
//!
//! Walk-forward: 252-bar train / 252-bar test
//! Fees: 0.1% taker each side
//! Exports equity curves per universe+window for chart generation.

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
const CHAND_PERIOD: usize = 15;
const CHAND_MULT: f64 = 2.00;
const MIN_TRADES: usize = 3;
const POSITION_CAP: usize = 3;
const HOLD_MAX: usize = 45;
const EP: usize = 21;
const TOP_K: usize = 8;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",       &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",       &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

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
}

/// A/D + Turtle + Chandelier dual-hat strategy
/// AD_PERIOD is set externally via SymData pre-computation
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
        // Rank by dollar volume
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
                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next.min(n.saturating_sub(1))];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high = highest_high.max(sd.high[b]);
                            let atr = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail = highest_high - CHAND_MULT * atr;
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
    (WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }, equity_curve)
}

fn build_sym_data(ad_period: usize) -> impl Fn(&DataFrame, usize) -> Option<SymData> {
    move |df: &DataFrame, n: usize| {
        let n_min: usize = df.height().min(n);
        macro_rules! col_vec { ($name:expr) => {{
            let chunked = df.column($name).ok()?.f64().ok()?;
            chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
        }};
        }
        let close = col_vec!("close"); let high = col_vec!("high");
        let low = col_vec!("low"); let vol = col_vec!("volume");
        let ad_line = compute_ad(&high, &low, &close, &vol);
        let mut ad_momentum = vec![0.0; ad_line.len()];
        for i in ad_period..close.len() { ad_momentum[i] = ad_line[i] - ad_line[i - ad_period]; }
        Some(SymData { close, high, low, vol, ad_line, ad_momentum })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let ad_periods = vec![5, 47, 2];

    eprintln!("==== A/D Period Walk-Forward Validation: p ∈ {:?} ====", ad_periods);
    eprintln!("A/D + Turtle(EP={}) + Chandelier({}, {}), TOP_K={}, CAP={}, HM={}", EP, CHAND_PERIOD, CHAND_MULT, TOP_K, POSITION_CAP, HOLD_MAX);
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
    eprintln!("Loaded {} symbols, {} bars\n", raw_cache.len(), n);

    let eq_dir = "snapshots/ad_p_comparison";
    std::fs::create_dir_all(eq_dir)?;

    // Run each AD period
    for &ad_period in &ad_periods {
        let label = format!("p{}", ad_period);
        eprintln!("\n==== AD_PERIOD={} ({}) ====", ad_period, label);

        // Build sym_data with pre-computed A/D momentum for this period
        let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
        let builder = build_sym_data(ad_period);
        for sym in &all_syms {
            if let Some(df) = raw_cache.get(sym) {
                if let Some(sd) = builder(df, n) {
                    sym_data_map.insert(sym.clone(), sd);
                }
            }
        }

        let mut csv_lines = vec![format!("universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass,label"), format!("{}_results", label)];
        let eq_csv_path = format!("{}/{}_equity.csv", eq_dir, label);
        let mut eq_file = File::create(&eq_csv_path)?;
        writeln!(eq_file, "universe,window,bar,equity")?;

        let mut global_pass = 0usize;
        let mut global_total = 0usize;
        let mut all_sharpes = Vec::new();

        for &(uni_label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            let mut agg_ret = 0.0_f64;
            let mut passed = 0usize;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let (r, eq) = run_sim(&sym_data_map, &symbols, ad_period, test_start, test_end);
                let thin = if r.trades < MIN_TRADES { "THIN" } else { "OK" };
                let result = if r.pass { "PASS" } else { "FAIL" };
                eprintln!("  {:<18} W{:02} | {:+8.1}% sh={:6.2} DD={:5.1}% {:4}t {:3.0}% {} {}",
                    uni_label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, thin, result);

                csv_lines.push(format!("{},{},{:.2},{:.4},{:.2},{},{:.2},{},{}",
                    uni_label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass, label));
                agg_ret += r.ret;
                if r.pass { passed += 1; }
                global_pass += if r.pass { 1 } else { 0 };
                global_total += 1;
                all_sharpes.push(r.sharpe);

                for (bi, &e) in eq.iter().enumerate() {
                    writeln!(eq_file, "{}_{:02},{},{},{:.8}", uni_label, wi, wi, bi, e)?;
                }
            }

            let avg_ret = agg_ret / total_windows as f64;
            let pass_pct = passed as f64 / total_windows as f64 * 100.0;
            eprintln!("  AGG | {:<18} avg {:+7.1}% {}/{} pass ({:.0}%)", uni_label, avg_ret, passed, total_windows, pass_pct);
        }

        let fail_pct = (global_total - global_pass) as f64 / global_total.max(1) as f64 * 100.0;
        let avg_sharpe: f64 = all_sharpes.iter().sum::<f64>() / all_sharpes.len().max(1) as f64;
        eprintln!("\n  GLOBAL: {}/{} passed ({:.0}% fail), avg Sharpe {:.3}", global_pass, global_total, fail_pct, avg_sharpe);

        let csv_path = format!("{}/{}_wf.csv", eq_dir, label);
        let mut f = File::create(&csv_path)?;
        for line in &csv_lines { writeln!(f, "{}", line)?; }
        eprintln!("  CSV: {} ({:.1}KB)", csv_path, std::fs::metadata(&csv_path)?.len() as f64 / 1024.0);
        eprintln!("  Equity: {} ({:.1}KB)", eq_csv_path, std::fs::metadata(&eq_csv_path)?.len() as f64 / 1024.0);
    }

    eprintln!("\nTotal runtime: {:?}", t0.elapsed());
    Ok(())
}

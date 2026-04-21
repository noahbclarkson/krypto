//! HOLD_MAX Hyperopt with PRODUCTION params (P=11/M=2.25/EP=24)
//!
//! Re-sweep HOLD_MAX using the CURRENT production Chandelier params
//! to properly evaluate HM=15 vs baseline HM=45 against the correct engine.
//!
//! Current production:  CHAND(11, 2.25), TURTLE_ENTRY=24, TURTLE_ATR(24, 2.0)
//! vs stale harness:    CHAND(15, 1.50), TURTLE_ENTRY=21
//!
//! Scope: 19 HM values × 9 universes × ~6 windows = ~1026 window-runs
//! Output: snapshots/hold_max_prod.csv          (per-window metrics)
//!         snapshots/hold_max_prod_equity.csv    (equity curves per HM)
//!         snapshots/hold_max_prod_summary.csv   (global aggregate per HM)

#![allow(unused)]

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

// CURRENT production params (confirmed 2026-04-20)
const CHAND_PERIOD: usize = 11;   // hyperopt 2026-04-20: CP=11 wins global Sharpe
const CHAND_MULT: f64 = 2.25;     // hyperopt 2026-04-20: M=2.25 wins +47% vs M=1.50
const TURTLE_ENTRY: usize = 24;   // hyperopt 2026-04-20 re-opt: EP=24 wins 45/54
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

// Extended HOLD_MAX sweep — 19 values covering full logical integer range
const HOLD_MAX_VALUES: &[usize] = &[
    5, 8, 10, 12, 15, 18, 20, 22, 25, 30,
    35, 40, 45, 50, 60, 75, 90, 120, 180,
];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

const CSV_METRICS: &str = "snapshots/hold_max_prod.csv";
const CSV_EQUITY:  &str = "snapshots/hold_max_prod_equity.csv";
const CSV_SUMMARY: &str = "snapshots/hold_max_prod_summary.csv";

struct SymData {
    close: Vec<f64>,
    high:  Vec<f64>,
    low:   Vec<f64>,
    vol:   Vec<f64>,
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
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).map_or(false, |&c| c >= max_close)
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
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
    equity_final: f64,
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hold_max: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Dollar-volume ranking
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

        // Entry check for top symbols
        let mut entry_sym = None;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                    entry_sym = Some(sym.clone());
                    break;
                }
            }
        }

        let entry_price = entry_sym.as_ref().and_then(|s| sym_data.get(s)?.close.get(bar).copied());

        if let Some(sym) = entry_sym {
            let sd = sym_data.get(&sym).unwrap();
            let mut highest_high = sd.high.get(bar).copied().unwrap_or(entry_price.unwrap_or(0.0));
            let mut lowest_low = sd.low.get(bar).copied().unwrap_or(entry_price.unwrap_or(0.0));

            let mut in_pos = true;
            let mut bars_held = 0usize;
            let mut exit_bar: Option<usize> = None;

            let mut b = bar + 1;
            while b < test_end && in_pos {
                bars_held += 1;
                if let Some(h) = sd.high.get(b) { highest_high = highest_high.max(*h); }
                if let Some(l) = sd.low.get(b) { lowest_low = lowest_low.min(*l); }

                let atr = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                if atr > 0.0 {
                    let chand_stop = highest_high - CHAND_MULT * atr;
                    let turtle_stop = lowest_low - TURTLE_ATR_MULT * atr;
                    let stop = chand_stop.max(turtle_stop);

                    if let Some(lo) = sd.low.get(b) {
                        if *lo <= stop {
                            in_pos = false;
                            exit_bar = Some(b);
                        }
                    }
                }

                if bars_held >= hold_max {
                    in_pos = false;
                    exit_bar = Some(b);
                }

                b += 1;
            }

            if let (Some(ex_bar), Some(en_p)) = (exit_bar, entry_price) {
                let exit_price = sym_data.get(&sym).and_then(|s| s.close.get(ex_bar)).copied().unwrap_or(en_p);
                let gross_ret = (exit_price - en_p) / en_p;
                let net_ret = gross_ret - TAKER_FEE * 2.0;
                equity *= 1.0 + net_ret;
                total_trades += 1;
                if net_ret > 0.0 { wins += 1; }
                daily_rets.push(net_ret);
            }
        }

        if equity > peak { peak = equity; }
        equity_curve.push(equity);
        bar += 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && equity > 0.0 && equity <= 100.0;
    WfResult { ret, sharpe: annualised_sharpe(&daily_rets), max_dd: max_dd_from(&equity_curve), trades: total_trades, win_rate, pass, equity_final: equity, equity_curve }
}

fn write_csv_lines(path: &str, lines: &[String]) {
    let mut f = File::create(path).unwrap_or_else(|e| panic!("Cannot create {}: {}", path, e));
    for line in lines { writeln!(f, "{}", line).unwrap(); }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("HOLD_MAX Hyperopt — PRODUCTION params");
    println!("CHAND({}, {}), EP={}, ATR={}", CHAND_PERIOD, CHAND_MULT, TURTLE_ENTRY, TURTLE_ATR_PERIOD);
    println!("{} HM values × 9 universes × 252/252 walk-forward", HOLD_MAX_VALUES.len());

    let loader = DataLoader::new(None, None);

    // Pre-load all symbol data (async, cached)
    let mut all_syms: HashSet<String> = HashSet::new();
    for &(_, syms) in UNIVERSES {
        for &s in syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, _> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let len = df.height();
                min_len = min_len.min(len);
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

    // ── Metric CSV
    let mut metric_lines = vec!["hm,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass,equity_final".to_string()];
    // ── Equity CSV
    let mut equity_lines = vec!["hm,universe,window,step,equity".to_string()];
    // ── Summary CSV
    let mut summary_lines = vec!["hm,global_pass,global_total,pass_rate,avg_sharpe,avg_ret,avg_dd,total_trades,avg_win_rate".to_string()];

    let mut hm_global_pass: HashMap<usize, usize> = HashMap::new();
    let mut hm_global_total: HashMap<usize, usize> = HashMap::new();
    let mut hm_global_trades: HashMap<usize, usize> = HashMap::new();
    let mut hm_sharpe_sum: HashMap<usize, f64> = HashMap::new();
    let mut hm_ret_sum: HashMap<usize, f64> = HashMap::new();
    let mut hm_dd_sum: HashMap<usize, f64> = HashMap::new();
    let mut hm_winrate_sum: HashMap<usize, f64> = HashMap::new();
    let mut hm_window_count: HashMap<usize, usize> = HashMap::new();

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

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            for &hm in HOLD_MAX_VALUES {
                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, hm);

                metric_lines.push(format!(
                    "{},{},{},{:.2},{:.4},{:.2},{},{:.2},{},{:.6}",
                    hm, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass, r.equity_final
                ));

                for (step_idx, &eq) in r.equity_curve.iter().enumerate() {
                    equity_lines.push(format!("{},{},{},{},{:.6}", hm, label, wi, step_idx, eq));
                }

                *hm_global_pass.entry(hm).or_insert(0) += if r.pass { 1 } else { 0 };
                *hm_global_total.entry(hm).or_insert(0) += 1;
                *hm_global_trades.entry(hm).or_insert(0) += r.trades;
                *hm_sharpe_sum.entry(hm).or_insert(0.0) += r.sharpe;
                *hm_ret_sum.entry(hm).or_insert(0.0) += r.ret;
                *hm_dd_sum.entry(hm).or_insert(0.0) += r.max_dd;
                *hm_winrate_sum.entry(hm).or_insert(0.0) += r.win_rate;
                *hm_window_count.entry(hm).or_insert(0) += 1;
            }
        }
    }

    // ── Write Summary CSV
    for &hm in HOLD_MAX_VALUES {
        let gp = *hm_global_pass.get(&hm).unwrap_or(&0);
        let gt = *hm_global_total.get(&hm).unwrap_or(&0);
        let wc = *hm_window_count.get(&hm).unwrap_or(&0);
        let pass_rate = if gt > 0 { gp as f64 / gt as f64 * 100.0 } else { 0.0 };
        let avg_sharpe = if wc > 0 { hm_sharpe_sum[&hm] / wc as f64 } else { 0.0 };
        let avg_ret = if wc > 0 { hm_ret_sum[&hm] / wc as f64 } else { 0.0 };
        let avg_dd = if wc > 0 { hm_dd_sum[&hm] / wc as f64 } else { 0.0 };
        let avg_winrate = if gt > 0 { hm_winrate_sum[&hm] / gt as f64 } else { 0.0 };
        summary_lines.push(format!("{},{},{},{:.2},{:.4},{:.2},{:.2},{:.2}", hm, gp, gt, pass_rate, avg_sharpe, avg_ret, avg_dd, avg_winrate));
    }

    write_csv_lines(CSV_METRICS, &metric_lines);
    write_csv_lines(CSV_EQUITY, &equity_lines);
    write_csv_lines(CSV_SUMMARY, &summary_lines);

    println!("\n==== GLOBAL SUMMARY ====");
    let mut summary_rows: Vec<(usize, f64, f64, f64, usize, usize)> = Vec::new();
    for &hm in HOLD_MAX_VALUES {
        let gp = *hm_global_pass.get(&hm).unwrap_or(&0);
        let gt = *hm_global_total.get(&hm).unwrap_or(&0);
        let wc = *hm_window_count.get(&hm).unwrap_or(&0);
        let avg_sharpe = if wc > 0 { hm_sharpe_sum[&hm] / wc as f64 } else { 0.0 };
        let avg_ret = if wc > 0 { hm_ret_sum[&hm] / wc as f64 } else { 0.0 };
        summary_rows.push((hm, gp as f64 / gt as f64 * 100.0, avg_sharpe, avg_ret, gp, gt));
    }
    summary_rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    for (hm, pr, sh, ar, gp, gt) in &summary_rows {
        let dd = hm_dd_sum.get(hm).copied().unwrap_or(0.0) / hm_window_count.get(hm).copied().unwrap_or(1) as f64;
        println!("  HM={:3} | Pass {}/{} ({:.1}%) | Sharpe {:.3} | Ret {:.1}% | DD {:.1}%",
                 hm, gp, gt, pr, sh, ar, dd);
    }

    println!("\n  CSV metrics: {}", CSV_METRICS);
    println!("  CSV equity:  {}", CSV_EQUITY);
    println!("  CSV summary: {}", CSV_SUMMARY);
    println!("  Runtime: {:?}", t0.elapsed());

    Ok(())
}
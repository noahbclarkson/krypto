//! VOL_LOOKBACK Hyperopt — Production params with ATR_ENTRY_MULT=0.90
//!
//! PURPOSE: Validate (or improve) VOL_LOOKBACK=2 default which was reverted from
//! VL=55 (global winner) based on held-out W04/W05 performance on STALE params.
//!
//! Prior: VL=55 won in 1-100 step1 sweep on CHAND(28,2.0)/EP=21.
//! Reverted to VL=2 because VL=55 underperformed W04/W05 (held-out).
//! But: W04/W05 were held-out under stale params. We need a proper test
//! on CURRENT production params: CHAND(11,2.25)/EP=24/ATR_ENTRY_MULT=0.90.
//!
//! HYPOTHESIS: VL=55 was overfitting non-held-out windows. VL=2 is more robust.
//! If VL=2 still wins on current params + all 9 universes → confirmed stable default.
//! If VL={5,10,15} wins → new better default found.
//!
//! Scope: 13 VL values × 9 universes × ~6 windows ≈ 702 window-runs
//! Output: snapshots/vol_lookback_sweep.csv     (per-window metrics)
//!         snapshots/vol_lookback_sweep_equity.csv  (equity curves per VL)
//!         snapshots/vol_lookback_sweep_summary.csv (global aggregate per VL)

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
const HOLD_MAX: usize = 12; // hyperopt 2026-04-21: HM=12 wins

// CURRENT production params (frozen 2026-04-21)
const CHAND_PERIOD: usize = 11;
const CHAND_MULT: f64 = 2.25;
const TURTLE_ENTRY: usize = 24;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.90; // live bot default; walk-forward validates

// VOL_LOOKBACK sweep values — full integer range 1-100, denser near typical values
const VOL_LOOKBACK_VALUES: &[usize] = &[
    1, 2, 3, 5, 7, 10,
    15, 20, 25, 30, 40,
    55, 75, 100,
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

const CSV_METRICS: &str = "snapshots/vol_lookback_sweep.csv";
const CSV_EQUITY:  &str = "snapshots/vol_lookback_sweep_equity.csv";
const CSV_SUMMARY: &str = "snapshots/vol_lookback_sweep_summary.csv";

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

/// Turtle signal WITH ATR_ENTRY_MULT filter (matches live bot: src/live/bot.rs)
fn turtle_signal(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    entry_period: usize,
    atr_period: usize,
    atr_mult: f64,
    idx: usize,
) -> bool {
    if idx < entry_period + 1 {
        return false;
    }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) {
            max_close = max_close.max(c);
        }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
    } else {
        false
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 {
        return 0.0;
    }
    let mn = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter()
        .map(|x| (x - mn).powi(2))
        .sum::<f64>() / daily_rets.len() as f64)
    .sqrt();
    if sd == 0.0 {
        return 0.0;
    }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak {
            peak = e;
        }
        let dd = (peak - e) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
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

/// Run one walk-forward window for a given VOL_LOOKBACK value.
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    vol_lookback: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Dollar-volume ranking using vol_lookback
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() {
                    continue;
                }
                let rol_vol = rolling_avg(&sd.vol, vol_lookback, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores
            .into_iter()
            .take(POSITION_CAP)
            .map(|(s, _)| s.to_string())
            .collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Entry check for top symbols
        let mut entry_sym = None;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if turtle_signal(
                    &sd.close,
                    &sd.high,
                    &sd.low,
                    TURTLE_ENTRY,
                    TURTLE_ATR_PERIOD,
                    ATR_ENTRY_MULT,
                    bar,
                ) {
                    entry_sym = Some(sym.clone());
                    break;
                }
            }
        }

        let entry_price = entry_sym
            .as_ref()
            .and_then(|s| sym_data.get(s)?.close.get(bar).copied());

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
                if let Some(h) = sd.high.get(b) {
                    highest_high = highest_high.max(*h);
                }
                if let Some(l) = sd.low.get(b) {
                    lowest_low = lowest_low.min(*l);
                }

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

                if bars_held >= HOLD_MAX {
                    in_pos = false;
                    exit_bar = Some(b);
                }

                b += 1;
            }

            if let (Some(ex_bar), Some(en_p)) = (exit_bar, entry_price) {
                let exit_price = sym_data
                    .get(&sym)
                    .and_then(|s| s.close.get(ex_bar))
                    .copied()
                    .unwrap_or(en_p);
                let gross_ret = (exit_price - en_p) / en_p;
                let net_ret = gross_ret - TAKER_FEE * 2.0;
                equity *= 1.0 + net_ret;
                total_trades += 1;
                if net_ret > 0.0 {
                    wins += 1;
                }
                daily_rets.push(net_ret);
            }
        }

        if equity > peak {
            peak = equity;
        }
        equity_curve.push(equity);
        bar += 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let win_rate = if total_trades > 0 {
        wins as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };
    let pass = total_trades >= MIN_TRADES && equity > 0.0 && equity <= 100.0;
    WfResult {
        ret,
        sharpe: annualised_sharpe(&daily_rets),
        max_dd: max_dd_from(&equity_curve),
        trades: total_trades,
        win_rate,
        pass,
        equity_final: equity,
        equity_curve,
    }
}

fn write_csv_lines(path: &str, lines: &[String]) {
    let mut f = File::create(path).unwrap_or_else(|e| panic!("Cannot create {}: {}", path, e));
    for line in lines {
        writeln!(f, "{}", line).unwrap();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("VOL_LOOKBACK Hyperopt — Production params");
    println!(
        "CHAND({}, {}), EP={}, ATR({}), ATR_ENTRY_MULT={}, HM={}",
        CHAND_PERIOD, CHAND_MULT, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, HOLD_MAX
    );
    println!(
        "{} VOL_LOOKBACK values × 9 universes × 252/252 walk-forward",
        VOL_LOOKBACK_VALUES.len()
    );

    let loader = DataLoader::new(None, None);

    // Pre-load all symbol data (async, cached)
    let mut all_syms: HashSet<String> = HashSet::new();
    for &(_, syms) in UNIVERSES {
        for &s in syms {
            all_syms.insert(s.to_string());
        }
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
            Err(e) => {
                eprintln!("  WARNING: {} load failed: {}", sym, e);
            }
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
            sym_data_map.insert(
                sym.clone(),
                SymData {
                    close: col_vec!("close"),
                    high: col_vec!("high"),
                    low: col_vec!("low"),
                    vol: col_vec!("volume"),
                },
            );
        }
    }
    println!(
        "Loaded {} symbols, {} bars\n",
        sym_data_map.len(),
        n
    );

    // ── Metric CSV
    let mut metric_lines =
        vec!["vl,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass,equity_final".to_string()];
    // ── Equity CSV
    let mut equity_lines = vec!["vl,universe,window,step,equity".to_string()];
    // ── Summary CSV
    let mut summary_lines =
        vec!["vl,global_pass,global_total,pass_rate,avg_sharpe,avg_ret,avg_dd,total_trades,avg_win_rate".to_string()];

    let mut vl_global_pass: HashMap<usize, usize> = HashMap::new();
    let mut vl_global_total: HashMap<usize, usize> = HashMap::new();
    let mut vl_global_trades: HashMap<usize, usize> = HashMap::new();
    let mut vl_sharpe_sum: HashMap<usize, f64> = HashMap::new();
    let mut vl_ret_sum: HashMap<usize, f64> = HashMap::new();
    let mut vl_dd_sum: HashMap<usize, f64> = HashMap::new();
    let mut vl_winrate_sum: HashMap<usize, f64> = HashMap::new();
    let mut vl_window_count: HashMap<usize, usize> = HashMap::new();

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
            if test_end.saturating_sub(test_start) < 5 {
                continue;
            }

            for &vl in VOL_LOOKBACK_VALUES {
                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, vl);

                metric_lines.push(format!(
                    "{},{},{},{:.2},{:.4},{:.2},{},{:.2},{},{:.6}",
                    vl, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass, r.equity_final
                ));

                for (step_idx, &eq) in r.equity_curve.iter().enumerate() {
                    equity_lines
                        .push(format!("{},{},{},{},{:.6}", vl, label, wi, step_idx, eq));
                }

                *vl_global_pass.entry(vl).or_insert(0) += if r.pass { 1 } else { 0 };
                *vl_global_total.entry(vl).or_insert(0) += 1;
                *vl_global_trades.entry(vl).or_insert(0) += r.trades;
                *vl_sharpe_sum.entry(vl).or_insert(0.0) += r.sharpe;
                *vl_ret_sum.entry(vl).or_insert(0.0) += r.ret;
                *vl_dd_sum.entry(vl).or_insert(0.0) += r.max_dd;
                *vl_winrate_sum.entry(vl).or_insert(0.0) += r.win_rate;
                *vl_window_count.entry(vl).or_insert(0) += 1;
            }
        }
    }

    // ── Write Summary CSV
    for &vl in VOL_LOOKBACK_VALUES {
        let gp = *vl_global_pass.get(&vl).unwrap_or(&0);
        let gt = *vl_global_total.get(&vl).unwrap_or(&0);
        let wc = *vl_window_count.get(&vl).unwrap_or(&0);
        let pass_rate = if gt > 0 { gp as f64 / gt as f64 * 100.0 } else { 0.0 };
        let avg_sharpe = if wc > 0 { vl_sharpe_sum[&vl] / wc as f64 } else { 0.0 };
        let avg_ret = if wc > 0 { vl_ret_sum[&vl] / wc as f64 } else { 0.0 };
        let avg_dd = if wc > 0 { vl_dd_sum[&vl] / wc as f64 } else { 0.0 };
        let avg_winrate = if gt > 0 { vl_winrate_sum[&vl] / gt as f64 } else { 0.0 };
        summary_lines.push(format!(
            "{},{},{},{:.2},{:.4},{:.2},{:.2},{},{:.2}",
            vl, gp, gt, pass_rate, avg_sharpe, avg_ret, avg_dd,
            vl_global_trades.get(&vl).unwrap_or(&0), avg_winrate
        ));
    }

    write_csv_lines(CSV_METRICS, &metric_lines);
    write_csv_lines(CSV_EQUITY, &equity_lines);
    write_csv_lines(CSV_SUMMARY, &summary_lines);

    println!("\n==== GLOBAL SUMMARY (sorted by avg Sharpe) ====");
    let mut summary_rows: Vec<(usize, f64, f64, f64, usize, usize, f64)> = Vec::new();
    for &vl in VOL_LOOKBACK_VALUES {
        let gp = *vl_global_pass.get(&vl).unwrap_or(&0);
        let gt = *vl_global_total.get(&vl).unwrap_or(&0);
        let wc = *vl_window_count.get(&vl).unwrap_or(&0);
        let avg_sharpe = if wc > 0 { vl_sharpe_sum[&vl] / wc as f64 } else { 0.0 };
        let avg_ret = if wc > 0 { vl_ret_sum[&vl] / wc as f64 } else { 0.0 };
        let avg_dd = if wc > 0 { vl_dd_sum[&vl] / wc as f64 } else { 0.0 };
        summary_rows.push((
            vl,
            gp as f64 / gt as f64 * 100.0,
            avg_sharpe,
            avg_ret,
            gp,
            gt,
            avg_dd,
        ));
    }
    summary_rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    for (vl, pr, sh, ar, gp, gt, dd) in &summary_rows {
        println!(
            "  VL={:3} | Pass {}/{} ({:.1}%) | Sharpe {:.3} | Ret {:.1}% | DD {:.1}%",
            vl, gp, gt, pr, sh, ar, dd
        );
    }

    println!("\n  CSV metrics: {}", CSV_METRICS);
    println!("  CSV equity:  {}", CSV_EQUITY);
    println!("  CSV summary: {}", CSV_SUMMARY);
    println!("  Runtime: {:?}", t0.elapsed());

    Ok(())
}

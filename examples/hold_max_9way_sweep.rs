//! HOLD_MAX 9-Universe Hyperopt — P=15/M=1.50
//!
//! Extended HOLD_MAX sweep across all 9 universes to validate (or reject)
//! the HM=15 candidate found in the 2026-04-19 Base5-only sweep.
//!
//! Prior: HM=45 was the validated default (CHAND_P=28/MULT=2.0).
//! New Chandelier: P=15/M=1.50 (tighter stop, exits ~bar 14).
//! Hypothesis: tighter Chandelier may mean looser HOLD_MAX is tolerable
//! (Chandelier fires first regardless, so HM acts as safety max).
//!
//! Scope: 14 HM values × 9 universes × ~6 windows = 756 window-runs.
//! Params: EP=21, CHAND(15,1.50), ATR(24,2.0), CAP=3, MIN_TRADES=3.
//!
//! Output: snapshots/hold_max_9way.csv (metrics)
//!         snapshots/hold_max_9way_equity.csv (per-window equity for charting)
//!         snapshots/hold_max_9way_summary.csv (global aggregate per HM)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

// Production Chandelier params (P=15/M=1.50 — updated 2026-04-19)
const CHAND_PERIOD: usize = 15;
const CHAND_MULT: f64 = 1.50;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

// Extended HOLD_MAX sweep — wide range to find true optimum
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

const CSV_METRICS: &str = "snapshots/hold_max_9way.csv";
const CSV_EQUITY:  &str = "snapshots/hold_max_9way_equity.csv";
const CSV_SUMMARY: &str = "snapshots/hold_max_9way_summary.csv";

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
    close.get(idx).map_or(false, |&c| c > max_close)
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

        // Turtle breakout entry
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + hold_max).min(n.saturating_sub(1));
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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_final: equity, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("==== HOLD_MAX 9-Universe Hyperopt ====");
    println!("Params: EP={}, Chandelier({}, {}), ATR({}, {}), CAP={}",
             TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, POSITION_CAP);
    println!("HM values: {:?}", HOLD_MAX_VALUES);
    println!("Universes: {}", UNIVERSES.len());

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
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

    // ── Metric CSV: hm,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass,equity_final
    let mut metric_lines = vec!["hm,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass,equity_final".to_string()];
    // ── Equity CSV: hm,universe,window,step,equity (step = bar index within window)
    let mut equity_lines = vec!["hm,universe,window,step,equity".to_string()];
    // ── Summary CSV: hm,global_pass,global_total,pass_rate,avg_sharpe,avg_ret,avg_dd,total_trades,avg_win_rate
    let mut summary_lines = vec!["hm,global_pass,global_total,pass_rate,avg_sharpe,avg_ret,avg_dd,total_trades,avg_win_rate".to_string()];

    // Per-HM global tracking
    let mut hm_global_pass: HashMap<usize, usize> = HashMap::new();
    let mut hm_global_total: HashMap<usize, usize> = HashMap::new();
    let mut hm_global_trades: HashMap<usize, usize> = HashMap::new();
    let mut hm_sharpe_sum: HashMap<usize, f64> = HashMap::new();
    let mut hm_ret_sum: HashMap<usize, f64> = HashMap::new();
    let mut hm_dd_sum: HashMap<usize, f64> = HashMap::new();
    let mut hm_winrate_sum: HashMap<usize, f64> = HashMap::new();
    let mut hm_window_count: HashMap<usize, usize> = HashMap::new();

    // HashMaps auto-initialized via .entry().or_insert() on first use

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

                // Equity curve: step within window → equity
                for (step_idx, &eq) in r.equity_curve.iter().enumerate() {
                    equity_lines.push(format!("{},{},{},{},{:.6}", hm, label, wi, step_idx, eq));
                }

                // Accumulate global stats for this HM
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
        let gp = hm_global_pass[&hm];
        let gt = hm_global_total[&hm];
        let gt_usize = gt;
        let wc = hm_window_count[&hm];
        let pass_rate = if gt_usize > 0 { gp as f64 / gt_usize as f64 * 100.0 } else { 0.0 };
        let avg_sharpe = if wc > 0 { hm_sharpe_sum[&hm] / wc as f64 } else { 0.0 };
        let avg_ret = if wc > 0 { hm_ret_sum[&hm] / wc as f64 } else { 0.0 };
        let avg_dd = if wc > 0 { hm_dd_sum[&hm] / wc as f64 } else { 0.0 };
        let avg_winrate = if gt_usize > 0 { hm_winrate_sum[&hm] / gt_usize as f64 } else { 0.0 };
        summary_lines.push(format!(
            "{},{},{},{:.2},{:.4},{:.2},{:.2},{:.2}",
            hm, gp, gt, pass_rate, avg_sharpe, avg_ret, avg_dd, avg_winrate
        ));
    }

    // Write files
    write_csv_lines(CSV_METRICS, &metric_lines);
    write_csv_lines(CSV_EQUITY, &equity_lines);
    write_csv_lines(CSV_SUMMARY, &summary_lines);

    println!("\n==== GLOBAL SUMMARY ====");
    let mut summary_rows: Vec<(usize, f64, f64, f64, usize, usize)> = Vec::new();
    for &hm in HOLD_MAX_VALUES {
        let gp = hm_global_pass[&hm];
        let gt = hm_global_total[&hm];
        let wc = hm_window_count[&hm];
        let pass_rate = if gt > 0 { gp as f64 / gt as f64 * 100.0 } else { 0.0 };
        let avg_sharpe = if wc > 0 { hm_sharpe_sum[&hm] / wc as f64 } else { 0.0 };
        let avg_ret = if wc > 0 { hm_ret_sum[&hm] / wc as f64 } else { 0.0 };
        let _avg_dd = if wc > 0 { hm_dd_sum[&hm] / wc as f64 } else { 0.0 };
        summary_rows.push((hm, pass_rate, avg_sharpe, avg_ret, gp, gt));
    }
    summary_rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    for (hm, pr, sh, ar, gp, gt) in &summary_rows {
        let dd = hm_dd_sum.get(hm).copied().unwrap_or(0.0) / *hm_window_count.get(hm).unwrap_or(&1) as f64;
        println!("  HM={:3} | Pass {}/{} ({:.1}%) | Sharpe {:.3} | Ret {:.1}% | DD {:.1}%",
                 hm, gp, gt, pr, sh, ar, dd);
    }

    println!("\n  CSV metrics: {}", CSV_METRICS);
    println!("  CSV equity:  {}", CSV_EQUITY);
    println!("  CSV summary: {}", CSV_SUMMARY);
    println!("  Runtime: {:?}", t0.elapsed());

    Ok(())
}

fn write_csv_lines(path: &str, lines: &[String]) {
    let mut f = File::create(path).unwrap_or_else(|e| panic!("Cannot create {}: {}", path, e));
    for line in lines {
        writeln!(f, "{}", line).unwrap();
    }
}

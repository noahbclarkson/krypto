//! vol_lookback hyperopt with CURRENT production params (2026-04-26)
//! CHAND_P=7, CHAND_M=2.30, EP=21, HM=12, ATR_P=24, ATR_M=2.0
//! Prior sweep (2026-04-21) used STALE CHAND(11,2.25)/EP=24 and found VL=1 winner.
//! This re-sweep verifies with the actual production config.
//! Sweep: VL ∈ {1,2,3,4,5,6,7,8,9,10,12,15,20} (13 values × 9 universes × 6 windows = 702 runs)

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
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const EP: usize = 21;
const ATR_PERIOD: usize = 24;
const ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.00;

const VOL_VALUES: &[usize] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 20];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",    &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
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

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
    } else { false }
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
    vol_lookback: usize,
    test_start: usize,
    test_end: usize,
) -> (f64, f64, f64, usize, f64, bool, Vec<f64>) {
    // returns: (ret, sharpe, max_dd, trades, win_rate, pass, equity_curve)
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, vol_lookback, bar);
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

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, EP, ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - ATR_MULT * atr_turtle;
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

    (ret, sharpe, max_dd, total_trades, win_rate, pass, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== vol_lookback hyperopt (current prod params) ====");
    eprintln!("CHAND(7, 2.30), EP=21, HM=12, ATR(24, 2.0)");
    eprintln!("VL sweep: {:?}", VOL_VALUES);

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
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // results[vL][universe][window] = result
    let mut results: HashMap<usize, HashMap<String, Vec<(f64,f64,f64,usize,f64,bool)>>> = HashMap::new();
    let mut equity_curves: HashMap<usize, HashMap<String, Vec<Vec<f64>>>> = HashMap::new();

    for &vl in VOL_VALUES {
        results.insert(vl, HashMap::new());
        equity_curves.insert(vl, HashMap::new());
    }

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { continue; }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        for &vl in VOL_VALUES {
            for wi in 0..total_windows {
                let test_start = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let (ret, sharpe, max_dd, trades, win_rate, pass, equity) =
                    run_sim(&sym_data_map, &symbols, vl, test_start, test_end);

                results.get_mut(&vl).unwrap().entry(label.to_string()).or_default()
                    .push((ret, sharpe, max_dd, trades, win_rate, pass));
                equity_curves.get_mut(&vl).unwrap().entry(label.to_string()).or_default()
                    .push(equity);
            }
        }
    }

    // Aggregate results
    let mut csv_lines = vec!["vl,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    let mut summary_lines = vec!["vl,pass_count,total,avg_sharpe,avg_ret,avg_dd,total_trades".to_string()];

    let mut vl_summary: Vec<(usize, usize, usize, f64, f64, f64, usize)> = Vec::new();

    for &vl in VOL_VALUES {
        let mut total_pass = 0usize;
        let mut total = 0usize;
        let mut sum_sharpe = 0.0_f64;
        let mut sum_ret = 0.0_f64;
        let mut sum_dd = 0.0_f64;
        let mut total_trades = 0usize;

        for (universe, window_results) in results.get(&vl).unwrap().iter() {
            for (wi, (ret, sharpe, max_dd, trades, win_rate, pass)) in window_results.iter().enumerate() {
                csv_lines.push(format!("{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    vl, universe, wi, ret, sharpe, max_dd, trades, win_rate, pass));
                sum_sharpe += sharpe;
                sum_ret += ret;
                sum_dd += max_dd;
                total_trades += trades;
                total += 1;
                if *pass { total_pass += 1; }
            }
        }

        let n_ = total.max(1) as f64;
        let avg_sh = sum_sharpe / n_;
        let avg_ret = sum_ret / n_;
        let avg_dd = sum_dd / n_;
        summary_lines.push(format!("{},{},{},{:.4},{:.2},{:.2},{}", vl, total_pass, total, avg_sh, avg_ret, avg_dd, total_trades));
        vl_summary.push((vl, total_pass, total, avg_sh, avg_ret, avg_dd, total_trades));

        eprintln!("VL={:2}: {:2}/{:2} pass, avg_sh={:.4}, avg_ret={:.2}%, avg_dd={:.2}%, trades={}",
            vl, total_pass, total, avg_sh, avg_ret, avg_dd, total_trades);
    }

    // Find winner
    let best_sh = vl_summary.iter().max_by(|x, y| x.3.partial_cmp(&y.3).unwrap()).unwrap();
    let best_pass = vl_summary.iter().max_by(|x, y| x.1.cmp(&y.1)).unwrap();
    eprintln!("\nBest Sharpe: VL={}, Sharpe={:.4}", best_sh.0, best_sh.3);
    eprintln!("Best Pass:   VL={}, Pass={}/{}", best_pass.0, best_pass.1, best_pass.2);

    let mut f = File::create("snapshots/vl_current_params_sweep.csv")?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    let mut g = File::create("snapshots/vl_current_params_summary.csv")?;
    for line in &summary_lines { writeln!(g, "{}", line)?; }

    // Write equity curves for: winner, VL=1 (baseline), VL=2 (runner-up), VL=8 (2nd best pass), VL=9 (best Base5)
    for &vl in &[best_sh.0, 1usize, 2usize, 8usize, 9usize] {
        let mut eq_lines = vec!["vl,universe,window,step,equity".to_string()];
        if let Some(universes) = equity_curves.get(&vl) {
            for (universe, windows) in universes.iter() {
                for (wi, equity) in windows.iter().enumerate() {
                    for (step, &eq) in equity.iter().enumerate() {
                        eq_lines.push(format!("{},{},{},{},{}", vl, universe, wi, step, eq));
                    }
                }
            }
        }
        let filename = format!("snapshots/vl_current_params_vl{}_equity.csv", vl);
        let mut ef = File::create(&filename)?;
        for line in &eq_lines { writeln!(ef, "{}", line)?; }
        eprintln!("Wrote {}", filename);
    }

    eprintln!("\nRuntime: {:?}", t0.elapsed());

    Ok(())
}
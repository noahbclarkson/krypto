//! Volume Ranking Lookback — DENSE sweep (1–60 step 1)
//! Tests whether volume smoothing window affects top-N selection robustness.
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
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 25;
const TURTLE_ATR_MULT: f64 = 2.00;
const MIN_TRADES: usize = 3;

// Dense sweep: 1 to 60 step 1
const VOL_LOOKBACK_VALS: &[usize] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    11,12,13,14,15,16,17,18,19,20,
    21,22,23,24,25,26,27,28,29,30,
    31,32,33,34,35,36,37,38,39,40,
    41,42,43,44,45,46,47,48,49,50,
    51,52,53,54,55,56,57,58,59,60,
];

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

struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64> }

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx + 1 < window { return vals[idx]; }
    let start = idx + 1 - window;
    let slice = &vals[start..=idx];
    slice.iter().sum::<f64>() / window as f64
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
    close.get(idx).map(|&c| c > max_close).unwrap_or(false)
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

struct WfResult {
    ret: f64, sharpe: f64, max_dd: f64, trades: usize, win_rate: f64, pass: bool,
}

fn run_sim(sym_data: &HashMap<String, SymData>, symbols: &[String],
           test_start: usize, test_end: usize, vol_lookback: usize) -> WfResult {
    let mut equity = 1.0_f64; let mut peak = equity;
    let mut wins = 0usize; let mut total_trades = 0usize;
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
        if top_syms.is_empty() { bar += 1; continue; }
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b; break;
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
                            bar = exit_bar + 1; entered = true; break;
                        }
                    }
                }
            }
        }
        if !entered { bar += 1; }
    }
    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let mut peak_eq = f64::NEG_INFINITY; let mut max_dd = 0.0_f64;
    let mut cur_peak = 1.0_f64;
    for _ in 0..1000 {
        let e = equity; // simplified
        break;
    }
    // Recompute equity curve max dd from daily rets
    peak_eq = 1.0_f64; max_dd = 0.0_f64;
    let mut running_eq = 1.0_f64;
    for dr in &daily_rets {
        running_eq *= 1.0 + dr;
        if running_eq > peak_eq { peak_eq = running_eq; }
        let dd = (peak_eq - running_eq) / peak_eq;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd *= 100.0;
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("VOL_LOOKBACK Dense Sweep: 60 values (1–60 step 1)");
    eprintln!("9 universes × 6 windows = 54 windows × 60 values = 3240 sims\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES { for &s in *syms { all_syms.insert(s.to_string()); } }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
        }
    }
    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = n.min(df.height());
            let close_col = df.column("close")?.f64()?;
            let high_col  = df.column("high")?.f64()?;
            let low_col   = df.column("low")?.f64()?;
            let vol_col   = df.column("volume")?.f64()?;
            sym_data_map.insert(sym.clone(), SymData {
                close: close_col.into_iter().filter_map(|x| x).take(n_min).collect(),
                high:  high_col.into_iter().filter_map(|x| x).take(n_min).collect(),
                low:   low_col.into_iter().filter_map(|x| x).take(n_min).collect(),
                vol:   vol_col.into_iter().filter_map(|x| x).take(n_min).collect(),
            });
        }
    }
    eprintln!("Loaded {} syms, {} bars\n", sym_data_map.len(), n);

    let csv_path = "snapshots/vol_lookback_dense.csv";
    let mut csv_lines = vec!["vol_lookback,universe,window,ret_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    let mut vb_stats: HashMap<usize, (Vec<f64>, Vec<f64>, usize, usize, f64)> = HashMap::new();
    for &vb in VOL_LOOKBACK_VALS { vb_stats.insert(vb, (vec![], vec![], 0, 0, 0.0)); }

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }
        for wi in 0..total_windows {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }
            for &vb in VOL_LOOKBACK_VALS {
                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, vb);
                csv_lines.push(format!("{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    vb, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass));
                let (sharpes, rets, pass_count, total_count, _) = vb_stats.get_mut(&vb).unwrap();
                sharpes.push(r.sharpe);
                rets.push(r.ret);
                *pass_count += if r.pass { 1 } else { 0 };
                *total_count += 1;
            }
        }
    }

    let mut f = File::create(csv_path)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("Metrics: {}\n", csv_path);

    eprintln!("{}", "=".repeat(65));
    eprintln!("{:>4} | {:>7} | {:>8} | {:>9} | {:>9} | {}", "VB", "PassRate", "AvgSharpe", "AvgRet%", "WorstDD%", "Pass/Total");
    eprintln!("{}", "-".repeat(65));
    let mut best_vb = 1usize; let mut best_pr = 0.0f64; let mut best_sh = f64::NEG_INFINITY;
    for &vb in VOL_LOOKBACK_VALS {
        let (sharpes, rets, pass_count, total_count, _) = vb_stats.get(&vb).unwrap();
        let avg_s = if sharpes.is_empty() { 0.0 } else { sharpes.iter().sum::<f64>() / sharpes.len() as f64 };
        let avg_r = if rets.is_empty()    { 0.0 } else { rets.iter().sum::<f64>() / rets.len() as f64 };
        let pr = if *total_count == 0 { 0.0 } else { *pass_count as f64 / *total_count as f64 * 100.0 };
        eprintln!("{:>4} | {:>6.1}% | {:>8.4} | {:>+8.1}% | {:>8.1}% | {}/{}", vb, pr, avg_s, avg_r, 0.0, *pass_count, *total_count);
        if pr > best_pr || (pr == best_pr && avg_s > best_sh) { best_pr = pr; best_vb = vb; best_sh = avg_s; }
    }
    eprintln!("\nBEST: VB={} (pass={:.1}%, Sharpe={:.4})", best_vb, best_pr, best_sh);
    eprintln!("Runtime: {:?}", t0.elapsed());
    Ok(())
}

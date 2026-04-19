//! ============================================================
//! TURTLE ENTRY FILTER SWEEP — ATR Entry Filter + Volume Confirmation
//! ============================================================
//!
//! TARGET: Two untested ideas with CURRENT production params (P=15/M=1.50):
//!
//! 1. ATR ENTRY FILTER — tested with STALE params (P=28/M=2.0) — mult=0.0 won.
//!    But with tighter Chandelier (P=15 vs P=28), the ATR entry filter
//!    interaction may differ.
//!    Values: {0.0, 0.1, 0.2, 0.3, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0}
//!    Fine grid around 0.0 (step 0.1) since prior coarse sweep
//!    only found mult=0.0 — we want to catch subtle interactions with tight stops.
//!
//! 2. VOLUME CONFIRMATION — untested in walk-forward with Turtle+Chandelier.
//!    Entry requires: vol_today >= SMA(vol, N) × threshold
//!    Values: none, SMA20×1.0, SMA20×1.25, SMA10×1.0
//!
//! Production params (P=15/M=1.50, frozen 2026-04-19):
//!   EP=21, CHAND(15, 1.50), TURTLE_ATR(24, 2.0), HOLD_MAX=45, CAP=3

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
// Current production params (P=15/M=1.50, validated 2026-04-19)
const EP: usize = 21;
const CHAND_PERIOD: usize = 15;
const CHAND_MULT: f64 = 1.50;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

// ATR entry filter sweep values
const ATR_FILT_VALS: &[f64] = &[0.0, 0.1, 0.2, 0.3, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];
const N_ATR: usize = 10;

// Volume confirmation: (sma_lookback, multiplier) — (0,0)=none
const VOL_CONFIRM_TYPES: [(usize, f64); 4] = [
    (0, 0.0),   // none
    (20, 1.0),  // SMA20 × 1.0
    (20, 1.25), // SMA20 × 1.25
    (10, 1.0),  // SMA10 × 1.0
];
const N_VOL: usize = 4;

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

const CSV_RESULTS: &str = "snapshots/turtle_entry_filter_results.csv";
const CSV_EQUITY:  &str = "snapshots/turtle_entry_filter_equity.csv";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

#[derive(Clone)]
struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn true_range(h: f64, l: f64, pc: f64) -> f64 {
    (h - l).max((h - pc).abs()).max((l - pc).abs())
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push(true_range(h, l, c0));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return vals[idx.min(vals.len().saturating_sub(1))]; }
    vals[idx - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(
    close: &[f64], high: &[f64], low: &[f64], vol: &[f64],
    entry_period: usize, atr_entry_mult: f64, atr_period: usize,
    vol_lb: usize, vol_mult: f64,
    idx: usize,
) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        if curr_close <= max_close { return false; }

        // ATR entry filter
        if atr_entry_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            if curr_close < max_close + atr_entry_mult * atr_val { return false; }
        }

        // Volume confirmation
        if vol_mult > 0.0 && vol_lb > 0 {
            let vol_today = vol.get(idx).copied().unwrap_or(0.0);
            let vol_sma = rolling_avg(vol, vol_lb, idx);
            if vol_today < vol_sma * vol_mult { return false; }
        }

        true
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

fn vol_confirm_label(lb: usize, mult: f64) -> String {
    if mult == 0.0 { "none".to_string() }
    else { format!("SMA{}_{:.2}", lb, mult) }
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_mult: f64,
    vol_lb: usize,
    vol_mult: f64,
) -> (WfResult, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank by dollar volume (VL=2 for ranking)
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, 2, bar);
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

        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, &sd.vol,
                                     EP, atr_mult, TURTLE_ATR_PERIOD, vol_lb, vol_mult, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        let mut exit_price = entry;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);

                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;

                            if sd.low[b] <= trail_chand {
                                exit_bar = b; exit_price = trail_chand; break;
                            }
                            if sd.low[b] <= trail_turtle {
                                exit_bar = b; exit_price = trail_turtle; break;
                            }
                        }

                        let exit = exit_price * (1.0 - TAKER_FEE);
                        let gross_ret = exit / entry - 1.0;
                        let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                        if bars_held < MIN_TRADES { continue; }

                        wins += if gross_ret > 0.0 { 1 } else { 0 };
                        total_trades += 1;
                        equity *= 1.0 + gross_ret;
                    }
                }
            }
        }

        equity_curve.push(equity);
        bar += 1;
    }

    for i in 1..equity_curve.len() {
        let dr = (equity_curve[i] - equity_curve[i-1]) / equity_curve[i-1];
        daily_rets.push(dr);
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && sharpe > 0.0;

    (WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Turtle Entry Filter Sweep: ATR × Vol Confirmation ====");
    eprintln!("Params: EP={}, CHAND({},{}), ATR({},{}), HM={}, CAP={}\n",
             EP, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP);

    let loader = DataLoader::new(None, None);

    // Collect all unique symbols
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    // Load all data
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
    eprintln!("Loaded {} symbols, {} bars\n", raw_cache.len(), n);

    // CSV output
    let mut result_lines = vec!["universe,window,atr_mult,vol_lb,vol_mult,vol_label,ret_pct,sharpe,max_dd,trades,win_rate,pass".to_string()];
    let mut equity_lines = vec!["universe,window,atr_mult,vol_label,bar_idx,equity".to_string()];

    // Global summary: (atr_mult, vol_label) -> accumulators
    let mut summary: HashMap<(String, String), (usize, usize, f64, f64, usize, f64, f64)> = HashMap::new();

    for (uni_name, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();

        // Build SymData map for this universe
        let mut sd_map: HashMap<String, SymData> = HashMap::new();
        for sym in &symbols {
            if let Some(df) = raw_cache.get(sym) {
                let n_min = df.height().min(n);
                macro_rules! col_vec {
                    ($name:expr) => {{
                        let chunked = df.column($name)?.f64()?;
                        chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                    }};
                }
                sd_map.insert(sym.clone(), SymData {
                    close: col_vec!("close"),
                    high:  col_vec!("high"),
                    low:   col_vec!("low"),
                    vol:   col_vec!("volume"),
                });
            }
        }

        let uni_n = sd_map.values().map(|sd| sd.close.len()).min().unwrap_or(0);
        if uni_n < TRAIN_BARS + TEST_BARS + 10 {
            eprintln!("[{}] SKIP — insufficient data ({} bars)", uni_name, uni_n);
            continue;
        }

        let n_windows = (uni_n - TRAIN_BARS) / TEST_BARS;
        eprintln!("[{}] {} windows, {} bars", uni_name, n_windows, uni_n);

        for w in 0..n_windows {
            let train_end = TRAIN_BARS + w * TEST_BARS;
            let test_start = train_end;
            let test_end = (train_end + TEST_BARS).min(uni_n.saturating_sub(1));
            if test_end <= test_start + MIN_TRADES { break; }

            for (ai, &am) in ATR_FILT_VALS.iter().enumerate() {
                for (vi, &(vol_lb, vol_mult)) in VOL_CONFIRM_TYPES.iter().enumerate() {
                    let vol_label = vol_confirm_label(vol_lb, vol_mult);
                    let (res, eq_curve) = run_sim(&sd_map, &symbols, test_start, test_end, am, vol_lb, vol_mult);

                    result_lines.push(format!(
                        "{},W{},{:.2},{},{:.2},{},{:.2},{:.4},{:.2},{},{:.1},{}",
                        uni_name, w, am, vol_lb, vol_mult, vol_label,
                        res.ret, res.sharpe, res.max_dd, res.trades, res.win_rate, res.pass
                    ));

                    // Equity curve: every 10 bars
                    for (bi, &eq) in eq_curve.iter().enumerate() {
                        if bi % 10 == 0 {
                            equity_lines.push(format!("{},W{},{:.2},{},{},{:.6}",
                                uni_name, w, am, vol_label, bi, eq));
                        }
                    }

                    // Accumulate global summary
                    let key = (format!("{:.2}", am), vol_label.clone());
                    let e = summary.entry(key).or_insert((0, 0, 0.0, 0.0, 0, 0.0, 0.0));
                    e.0 += if res.pass { 1 } else { 0 };
                    e.1 += 1;
                    e.2 += res.sharpe;
                    e.3 += res.ret;
                    e.4 += res.trades;
                    e.5 += res.win_rate;
                    e.6 += res.max_dd;
                }
            }
        }
    }

    // Write CSVs
    let mut f = File::create(CSV_RESULTS)?;
    for l in &result_lines { writeln!(f, "{}", l)?; }
    eprintln!("Wrote {}", CSV_RESULTS);

    let mut f = File::create(CSV_EQUITY)?;
    for l in &equity_lines { writeln!(f, "{}", l)?; }
    eprintln!("Wrote {}", CSV_EQUITY);

    // Print global summary
    let mut sorted: Vec<_> = summary.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    sorted.sort_by(|a, b| {
        let ra = a.1.0 as f64 / a.1.1.max(1) as f64;
        let rb = b.1.0 as f64 / b.1.1.max(1) as f64;
        let cmp = rb.partial_cmp(&ra).unwrap();
        if cmp != std::cmp::Ordering::Equal { return cmp; }
        let sa = a.1.2 / a.1.1.max(1) as f64;
        let sb = b.1.2 / b.1.1.max(1) as f64;
        sb.partial_cmp(&sa).unwrap()
    });

    println!("\n=== GLOBAL SUMMARY (pass_rate DESC, sharpe DESC) ===");
    println!("{:>8} {:>14} {:>6} {:>7} {:>10} {:>10} {:>10} {:>8}",
             "ATR_mult", "vol_confirm", "pass_n", "total", "pass_rt", "avg_sharpe", "avg_ret", "trades");
    println!("{}", "-".repeat(75));
    for ((am, vn), (pos, tot, ss, sr, trades, swr, sdd)) in sorted {
        let pass_rt = pos as f64 / tot.max(1) as f64 * 100.0;
        let avg_s = ss / tot.max(1) as f64;
        let avg_r = sr / tot.max(1) as f64;
        println!("{:>8.2} {:>14} {:>6} {:>7} {:>10.1}% {:>10.4} {:>10.2} {:>8}",
                 am, vn, pos, tot, pass_rt, avg_s, avg_r, trades);
    }

    println!("\nRuntime: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

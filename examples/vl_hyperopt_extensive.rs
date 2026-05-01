//! VOL_LOOKBACK Extensive Hyperopt: 1..=100 × 9 Universes × 6 WF Windows
//!
//! Parameter: VOL_LOOKBACK — dollar-volume smoothing window for top-N ranking.
//! Production default: 8 (set 2026-04-28; confirmed 2026-04-29 on current params)
//!
//! This run re-audits with CURRENT production params and FULL integer range.
//! Strategy: Turtle+Chandelier(7,2.30) / TurtleATR(24,2.0) dual exit
//! Fee: correct (1+fee entry / 1-fee exit)
//!
//! Sweep: 100 values × 9 universes × 6 windows = 5,400 window-runs total.

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
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const ATR_ENTRY_MULT: f64 = 0.00;
const ATR_EMA_PERIOD: usize = 1;
const TURTLE_ATR_MULT: f64 = 2.00;

// Base production params (frozen)
const EP: usize = 21;
const MIN_VL: usize = 1;
const MAX_VL: usize = 100;
const VL_STEP: usize = 1;

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

const SUMMARY_OUT: &str = "snapshots/vl_hyperopt_summary.csv";
const EQUITY_OUT: &str = "snapshots/vl_hyperopt_equity.csv";
const SELECTED_OUT: &str = "snapshots/vl_hyperopt_selected.csv";

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

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    vol_lookback: usize,
    test_start: usize,
    test_end: usize,
) -> WfResult {
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
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 + TAKER_FEE);
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
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== VOL_LOOKBACK Hyperopt: {}..={} × 9 Universes × 6 WF ====", MIN_VL, MAX_VL);
    eprintln!("Strategy: Turtle+Chandelier({}, {}) / TurtleATR({}, {})", CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT);
    eprintln!("Fixed params: EP={}, HM={}, CAP={}, TAKER_FEE={}\n", EP, HOLD_MAX, POSITION_CAP, TAKER_FEE);

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
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

    // ── Per-value results ────────────────────────────────────────────────────
    let mut summary_lines = vec!["vl,pass_count,total_windows,pass_pct,avg_sharpe,avg_ret,avg_dd,trades,pos_universes".to_string()];
    let mut all_summary: Vec<(usize, usize, usize, f64, f64, f64, f64, usize, usize)> = Vec::new();

    // ── Per-value equity curves (write in streaming fashion) ────────────────
    let mut equity_csv = File::create(EQUITY_OUT)?;
    writeln!(equity_csv, "vl,universe,window,bar_idx,equity")?;

    // ── Selected equity snapshots (baseline + winner + runner-ups) ───────────
    let mut selected_csv = File::create(SELECTED_OUT)?;
    writeln!(selected_csv, "vl,universe,window,bar_idx,equity")?;

    for vl in (MIN_VL..=MAX_VL).step_by(VL_STEP) {
        let mut global_pass = 0usize;
        let mut global_total = 0usize;
        let mut global_trades = 0usize;
        let mut global_sharpe_sum = 0.0_f64;
        let mut global_ret_sum = 0.0_f64;
        let mut global_dd_sum = 0.0_f64;
        let mut pos_universes = 0usize;

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, vl, test_start, test_end);

                global_pass += if r.pass { 1 } else { 0 };
                global_total += 1;
                global_trades += r.trades;
                global_sharpe_sum += r.sharpe;
                global_ret_sum += r.ret;
                global_dd_sum += r.max_dd;

                // Equity CSV: write every N bars to keep file manageable
                // Write at bar_idx 0 and then every 5th bar
                for (ei, &eq) in r.equity_curve.iter().enumerate() {
                    if ei == 0 || ei % 5 == 0 || ei == r.equity_curve.len() - 1 {
                        writeln!(equity_csv, "{},{},{},{},{:.6}", vl, label, wi, ei, eq)?;
                        // Also write selected snapshots (baseline=8, winner, runner-ups at 5, 20, 50)
                        if vl == 8 || vl == 20 || vl == 50 {
                            writeln!(selected_csv, "{},{},{},{},{:.6}", vl, label, wi, ei, eq)?;
                        }
                    }
                }
            }

            // Count positive universes
            if global_ret_sum > 0.0 {
                pos_universes += 1;
            }
        }

        let pass_pct = if global_total > 0 { global_pass as f64 / global_total as f64 * 100.0 } else { 0.0 };
        let avg_sharpe = if global_total > 0 { global_sharpe_sum / global_total as f64 } else { 0.0 };
        let avg_ret = if global_total > 0 { global_ret_sum / global_total as f64 } else { 0.0 };
        let avg_dd = if global_total > 0 { global_dd_sum / global_total as f64 } else { 0.0 };

        summary_lines.push(format!("{},{},{},{:.2},{:.4},{:.2},{:.2},{},{}",
            vl, global_pass, global_total, pass_pct, avg_sharpe, avg_ret, avg_dd, global_trades, pos_universes));

        all_summary.push((vl, global_pass, global_total, pass_pct, avg_sharpe, avg_ret, avg_dd, global_trades, pos_universes));

        eprintln!("  VL={} | {}/{} pass ({:.1}) | sh={:.3} | ret={:.1} | DD={:.1} | {}t | +Uni={}",
            vl, global_pass, global_total, pass_pct, avg_sharpe, avg_ret, avg_dd, global_trades, pos_universes);
    }

    // Write summary CSV
    let mut sf = File::create(SUMMARY_OUT)?;
    for line in &summary_lines { writeln!(sf, "{}", line)?; }

    // ── Analysis: find robustness winner ────────────────────────────────────
    // Primary: pass_pct (desc), then avg_sharpe (desc), then pos_universes (desc)
    all_summary.sort_by(|a, b| {
        b.3.partial_cmp(&a.3).unwrap().then_with(|| b.4.partial_cmp(&a.4).unwrap()).then_with(|| b.8.cmp(&a.8))
    });

    eprintln!("\n==== TOP 10 BY ROBUSTNESS (pass_pct desc → sharpe desc → pos_universes) ====");
    for (i, &(vl, pass, tot, pp, sh, ret, dd, trades, pu)) in all_summary.iter().take(10).enumerate() {
        eprintln!("  #{} VL={} | {}/{} ({:.1}) | sh={:.3} | ret={:.1} | DD={:.1} | {}t | +Uni={}",
            i+1, vl, pass, tot, pp, sh, ret, dd, trades, pu);
    }

    let winner_vl = all_summary[0].0;
    let winner = all_summary.iter().find(|x| x.0 == winner_vl).unwrap();
    eprintln!("\n  *** ROBUSTNESS WINNER: VL={} ***", winner_vl);
    eprintln!("      Pass: {}/{} ({}) | Sharpe: {} | Ret: {} | DD: {} | +Uni: {}/9",
        winner.1, winner.2, winner.3, winner.4, winner.5, winner.6, winner.8);

    eprintln!("\n  Runtime: {:?}", t0.elapsed());
    eprintln!("  Summary: {}", SUMMARY_OUT);
    eprintln!("  Equity:  {}", EQUITY_OUT);
    eprintln!("  Selected equity: {}", SELECTED_OUT);

    Ok(())
}

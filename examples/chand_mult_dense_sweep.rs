//! CHAND_MULT Dense Sweep — 71 values, step=0.05 across [1.50..5.00]
//!
//! Purpose: Validate CHAND_MULT=2.25 is genuinely optimal, or if finer step reveals a better value.
//! Prior sweep (step=0.25, 19 values) found M=2.25 (+47% vs M=1.50).
//! Dense sweep confirms robustness at 4x resolution.
//!
//! Exports:
//!   snapshots/chand_mult_dense_sweep.csv   — per-M aggregated metrics
//!   snapshots/chand_mult_dense_top5_equity.csv — equity curves for winner + 4 runners-up
//!   snapshots/chand_mult_dense_top5_detail.csv — per-universe per-window for top 5

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
const TURTLE_ENTRY: usize = 24;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.85;
const VOL_LOOKBACK: usize = 1;

// CHAND_MULT values: step=0.05 from 1.50 to 5.00 inclusive
const CHAND_MULTS: &[f64] = &[
    1.50, 1.55, 1.60, 1.65, 1.70, 1.75, 1.80, 1.85, 1.90, 1.95,
    2.00, 2.05, 2.10, 2.15, 2.20, 2.25, 2.30, 2.35, 2.40, 2.45,
    2.50, 2.55, 2.60, 2.65, 2.70, 2.75, 2.80, 2.85, 2.90, 2.95,
    3.00, 3.05, 3.10, 3.15, 3.20, 3.25, 3.30, 3.35, 3.40, 3.45,
    3.50, 3.55, 3.60, 3.65, 3.70, 3.75, 3.80, 3.85, 3.90, 3.95,
    4.00, 4.05, 4.10, 4.15, 4.20, 4.25, 4.30, 4.35, 4.40, 4.45,
    4.50, 4.55, 4.60, 4.65, 4.70, 4.75, 4.80, 4.85, 4.90, 4.95,
    5.00,
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

const OUT_SUMMARY: &str = "snapshots/chand_mult_dense_sweep.csv";
const OUT_EQUITY: &str = "snapshots/chand_mult_dense_equity_curves.csv";
const OUT_TOP5_DETAIL: &str = "snapshots/chand_mult_dense_top5_detail.csv";

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

struct SimResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    /// Per-bar equity curve (relative to 1.0)
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    chand_mult: f64,
) -> SimResult {
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

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=(entry_bar_next + HOLD_MAX).min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - chand_mult * atr_chand;

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

    SimResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== CHAND_MULT Dense Sweep: {} values [1.50..5.00] step 0.05 ====", CHAND_MULTS.len());

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

    // ── Per-M summary results ────────────────────────────────────────────────
    let mut summary_lines = vec!["chand_mult,avg_return,avg_sharpe,avg_max_dd,avg_trades,avg_win_rate,pass_rate,windows_passed,total_windows".to_string()];

    // ── Equity curves: mean across all windows (written after finding top 5) ──
    // Structure: chand_mult,bar,mean_equity
    let mut equity_by_m: HashMap<usize, Vec<Vec<f64>>> = HashMap::new(); // m_idx -> windows -> bars

    // Per-universe per-window detail for top 5
    let mut top5_detail: HashMap<usize, Vec<(String, usize, f64, f64, f64, usize, f64, bool)>> = HashMap::new();

    // First pass: run all M values
    for &m in CHAND_MULTS {
        let m_idx = CHAND_MULTS.iter().position(|&x| x == m).unwrap();

        let mut m_passed = 0usize;
        let mut m_total = 0usize;
        let mut m_sharpe_sum = 0.0_f64;
        let mut m_ret_sum = 0.0_f64;
        let mut m_dd_sum = 0.0_f64;
        let mut m_trades_sum = 0usize;
        let mut m_wr_sum = 0.0_f64;
        let mut all_equity_curves: Vec<Vec<f64>> = Vec::new();

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, m);

                m_passed += if r.pass { 1 } else { 0 };
                m_total += 1;
                m_sharpe_sum += r.sharpe;
                m_ret_sum += r.ret;
                m_dd_sum += r.max_dd;
                m_trades_sum += r.trades;
                m_wr_sum += r.win_rate;
                all_equity_curves.push(r.equity_curve);

                // Store detail for top 5
                if top5_detail.get(&m_idx).map_or(true, |v| v.len() < 5 * 9 * 6) {
                    top5_detail.entry(m_idx).or_default().push((label.to_string(), wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass));
                }
            }
        }

        let n_win = m_total.max(1) as f64;
        let avg_sharpe = m_sharpe_sum / n_win;
        let avg_ret = m_ret_sum / n_win;
        let avg_dd = m_dd_sum / n_win;
        let avg_trades = m_trades_sum as f64 / n_win;
        let avg_wr = m_wr_sum / n_win;
        let pass_rate = m_passed as f64 / n_win.max(1.0) * 100.0;

        summary_lines.push(format!(
            "{:.2},{:.2},{:.4},{:.2},{:.1},{:.2},{:.1},{},{}",
            m, avg_ret, avg_sharpe, avg_dd, avg_trades, avg_wr, pass_rate,
            m_passed, m_total
        ));

        // Store equity curves for this M
        equity_by_m.insert(m_idx, all_equity_curves);

        eprintln!("M={:.2}  sh={:.3}  ret={:.1}%  DD={:.1}%  pass={}/{} ({:.0}%)",
            m, avg_sharpe, avg_ret, avg_dd, m_passed, m_total, pass_rate);
    }

    // Write summary CSV
    let mut f = File::create(OUT_SUMMARY)?;
    for line in &summary_lines { writeln!(f, "{}", line)?; }
    eprintln!("\nWrote {}", OUT_SUMMARY);

    // ── Compute mean equity per bar across all windows for each M ────────────
    // Find top 5 by Sharpe
    let mut m_results: Vec<(usize, f64)> = Vec::new();
    for (i, line) in summary_lines.iter().enumerate().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 3 {
            if let (Ok(sh), Ok(_)) = (parts[2].parse::<f64>(), parts[0].parse::<f64>()) {
                m_results.push((i - 1, sh)); // index into CHAND_MULTS
            }
        }
    }
    m_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let top5_m_idx: Vec<usize> = m_results.iter().take(5).map(|(idx, _)| *idx).collect();

    eprintln!("\nTop 5 by Sharpe:");
    for &idx in &top5_m_idx {
        eprintln!("  M={:.2}  sh={:.4}", CHAND_MULTS[idx], m_results.iter().find(|(i,_)| *i==idx).map(|(_,s)| *s).unwrap_or(0.0));
    }

    // Write equity curves for top 5 M values
    let mut eq_f = File::create(OUT_EQUITY)?;
    writeln!(eq_f, "chand_mult,bar,mean_equity")?;
    for &m_idx in &top5_m_idx {
        let curves = equity_by_m.get(&m_idx).cloned().unwrap_or_default();
        if curves.is_empty() { continue; }

        // Find max bar length
        let max_bars = curves.iter().map(|c| c.len()).max().unwrap_or(0);
        for bar_idx in 0..max_bars {
            let vals: Vec<f64> = curves.iter()
                .filter_map(|c| c.get(bar_idx).copied())
                .collect();
            let mean_eq = if vals.is_empty() { 1.0 } else { vals.iter().sum::<f64>() / vals.len() as f64 };
            writeln!(eq_f, "{:.2},{},{:.6}", CHAND_MULTS[m_idx], bar_idx, mean_eq)?;
        }
    }
    eprintln!("Wrote {}", OUT_EQUITY);

    // Write top 5 detail CSV
    let mut det_f = File::create(OUT_TOP5_DETAIL)?;
    writeln!(det_f, "chand_mult,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;
    for &m_idx in &top5_m_idx {
        if let Some(details) = top5_detail.get(&m_idx) {
            for (label, wi, ret, sharpe, max_dd, trades, win_rate, pass) in details {
                writeln!(det_f, "{:.2},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    CHAND_MULTS[m_idx], label, wi, ret, sharpe, max_dd, trades, win_rate, pass)?;
            }
        }
    }
    eprintln!("Wrote {}", OUT_TOP5_DETAIL);

    eprintln!("\nTotal runtime: {:?}", t0.elapsed());
    Ok(())
}

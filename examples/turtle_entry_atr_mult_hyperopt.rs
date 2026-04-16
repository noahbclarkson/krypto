//! ============================================================
//! TURTLE ATR ENTRY MULTIPLIER HYPEROPT
//! ============================================================
//!
//! TARGET: Turtle ATR Entry Multiplier — the breakout quality filter
//! PRIOR:  NEVER tested. The turtle_signal only checks close > max_close.
//!          Classic Turtle requires: close > max_high + ATR_MULT × ATR_at_breakout
//!          This filter prevents entries during low-vol choppy markets.
//!
//! SWEEP:  {0.0, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 2.5, 3.0}
//!          0.0 = disabled (current behavior, baseline)
//!          >0  = requires breakout to exceed ATR_MULT × ATR_at_breakout
//!
//! STRATEGY: Turtle+Chandelier (current production config)
//!   - EP=21, CHAND(28,2.0), TURTLE_ATR(25,2.0) DUAL_EXIT
//!   - CAP=3, HM=45, MIN_TRADES=3, 0.1% taker
//!
//! METHOD: Walk-forward (252 train / 252 test) on all 9 universes
//! METRICS: OOS pass rate, avg Sharpe, worst DD, equity curves
//!
//! EXPORTS:
//!   snapshots/turtle_atr_entry_mult_results.csv  — aggregate per multiplier
//!   snapshots/turtle_atr_entry_mult_universe.csv — per-multiplier × universe
//!   snapshots/turtle_atr_entry_mult_equity.csv  — equity curves for chart generation

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
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

// ── ATR Entry Multiplier sweep values ──────────────────────────────────────────
const ATR_ENTRY_VALS: &[f64] = &[0.0, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 2.5, 3.0];

// ── 9 universes ────────────────────────────────────────────────────────────────
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

const CSV_RESULTS: &str = "snapshots/turtle_atr_entry_mult_results.csv";
const CSV_UNIVERSE: &str = "snapshots/turtle_atr_entry_mult_universe.csv";
const CSV_EQUITY: &str = "snapshots/turtle_atr_entry_mult_equity.csv";
const N_MULT: usize = 9; // number of ATR_MULT values

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
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
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

/// Returns true if Turtle breakout fires, optionally filtered by ATR entry threshold.
/// If atr_entry_mult == 0.0: standard close > max_close (baseline, no ATR filter)
/// If atr_entry_mult > 0.0: requires close > max_close + atr_entry_mult * ATR_at_breakout
fn turtle_signal_with_atr_filter(
    close: &[f64], high: &[f64], low: &[f64],
    entry_period: usize, atr_entry_mult: f64, atr_period: usize,
    idx: usize,
) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if !breakout { return false; }
        // ATR filter: if enabled, breakout must exceed ATR threshold
        if atr_entry_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            let threshold = max_close + atr_entry_mult * atr_val;
            return curr_close > threshold;
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

struct SimResult {
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
    atr_entry_mult: f64,
    test_start: usize,
    test_end: usize,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume at current bar
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
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
                    if turtle_signal_with_atr_filter(
                        &sd.close, &sd.high, &sd.low,
                        TURTLE_ENTRY, atr_entry_mult, TURTLE_ATR_PERIOD, bar
                    ) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // DUAL_EXIT: Chandelier(28,2.0) OR Turtle_ATR(25,2.0)
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

                            if equity > 1.0_f64 { /* peak tracking done in equity_curve */ }
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
    eprintln!("==== Turtle ATR Entry Multiplier Hyperopt ====");
    eprintln!("ATR_ENTRY_MULT values: {:?}", ATR_ENTRY_VALS);
    eprintln!("Strategy: Turtle EP={}, Chandelier({},{}), DUAL_EXIT ATR({})\n",
        TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD);

    // ── Load data ────────────────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
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

    // ── Per-multiplier results ───────────────────────────────────────────────────
    // Using indexed arrays to avoid f64 HashMap key issues
    let mut mult_pass: [usize; N_MULT] = [0; N_MULT];
    let mut mult_total: [usize; N_MULT] = [0; N_MULT];
    let mut mult_sum_ret: [f64; N_MULT] = [0.0; N_MULT];
    let mut mult_sum_sh: [f64; N_MULT] = [0.0; N_MULT];
    let mut mult_worst_dd: [f64; N_MULT] = [0.0; N_MULT];
    let mut mult_tot_trades: [usize; N_MULT] = [0; N_MULT];

    let mut universe_records: Vec<(String, usize, SimResult)> = Vec::new();
    // Equity curves — per multiplier aggregate
    let mut equity_curves: Vec<Vec<f64>> = vec![vec![1.0_f64]; N_MULT];

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

        eprintln!("\n==== {:<18} ====", label);

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            for (mi, &mult) in ATR_ENTRY_VALS.iter().enumerate() {
                let r = run_sim(&sym_data_map, &symbols, mult, test_start, test_end);

                mult_total[mi] += 1;
                mult_sum_ret[mi] += r.ret;
                mult_sum_sh[mi] += r.sharpe;
                mult_worst_dd[mi] = mult_worst_dd[mi].max(r.max_dd);
                mult_tot_trades[mi] += r.trades;
                if r.pass { mult_pass[mi] += 1; }

                // Aggregate equity curves: normalize each window then chain
                {
                    let last_val = *equity_curves[mi].last().unwrap_or(&1.0);
                    if let Some(&first_ev) = r.equity_curve.first() {
                        if first_ev > 0.0 {
                            for &ev in &r.equity_curve {
                                equity_curves[mi].push(last_val * ev / first_ev);
                            }
                        }
                    }
                }

                universe_records.push((format!("{}_{}", label, wi), mi, r));
            }

            // Print window results for all multipliers
            let thin = universe_records.iter()
                .filter(|(name, _, _)| name.starts_with(&format!("{}_{}", label, wi)))
                .map(|(_, mi, r)| (ATR_ENTRY_VALS[*mi], r.trades, r.pass))
                .collect::<Vec<_>>();
            eprintln!("  W{:02}: {}", wi,
                thin.iter()
                    .map(|(m, t, p)| format!("mult={:.2} t={} {}", m, t, if *p {"✓"} else {"✗"}))
                    .collect::<Vec<_>>()
                    .join(" | ")
            );
        }
    }

    // ── Write aggregate results CSV ──────────────────────────────────────────────
    let mut f = File::create(CSV_RESULTS)?;
    writeln!(f, "atr_entry_mult,pass,total,pass_pct,avg_ret,avg_sharpe,worst_dd,total_trades")?;
    #[derive(Debug)]
    struct Row {
        mi: usize,
        mult: f64,
        pass: usize,
        total: usize,
        pct: f64,
        avg_ret: f64,
        avg_sh: f64,
        worst_dd: f64,
        trades: usize,
    }
    let mut rows: Vec<Row> = (0..N_MULT)
        .map(|mi| {
            let mult = ATR_ENTRY_VALS[mi];
            let pass = mult_pass[mi];
            let total = mult_total[mi];
            let pct = pass as f64 / total.max(1) as f64 * 100.0;
            let avg_ret = mult_sum_ret[mi] / total.max(1) as f64;
            let avg_sh  = mult_sum_sh[mi] / total.max(1) as f64;
            let worst_dd = mult_worst_dd[mi];
            let trades = mult_tot_trades[mi];
            Row { mi, mult, pass, total, pct, avg_ret, avg_sh, worst_dd, trades }
        })
        .collect();
    rows.sort_by(|a, b| b.avg_sh.partial_cmp(&a.avg_sh).unwrap()); // sort by avg Sharpe desc
    for r in &rows {
        writeln!(f, "{:.2},{},{},{:.2},{:.2},{:.4},{:.2},{}",
            r.mult, r.pass, r.total, r.pct, r.avg_ret, r.avg_sh, r.worst_dd, r.trades)?;
        eprintln!("  mult={:.2}: {}/{} pass ({:.1}%), Sharpe={:.3}, ret={:.1}%, DD={:.1}%, {} trades",
            r.mult, r.pass, r.total, r.pct, r.avg_sh, r.avg_ret, r.worst_dd, r.trades);
    }

    // ── Write universe-level results CSV ────────────────────────────────────────
    let mut uf = File::create(CSV_UNIVERSE)?;
    writeln!(uf, "universe,atr_entry_mult,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;
    // Group by universe
    for &(uni_label, _symbols) in UNIVERSES {
        for mi in 0..N_MULT {
            let mult = ATR_ENTRY_VALS[mi];
            let recs: Vec<_> = universe_records.iter()
                .filter(|(name, m, _)| name.starts_with(&format!("{}_", uni_label)) && *m == mi)
                .collect();
            if recs.is_empty() { continue; }
            let avg_ret = recs.iter().map(|(_,_,r)| r.ret).sum::<f64>() / recs.len() as f64;
            let avg_sh  = recs.iter().map(|(_,_,r)| r.sharpe).sum::<f64>() / recs.len() as f64;
            let worst_dd= recs.iter().map(|(_,_,r)| r.max_dd).fold(0.0_f64, |a,b| a.max(b));
            let trades: usize = recs.iter().map(|(_,_,r)| r.trades).sum();
            let pass_cnt= recs.iter().filter(|(_,_,r)| r.pass).count();
            let total_trades: usize = recs.iter().map(|(_,_,r)| r.trades).sum();
            let total_win_trades: usize = recs.iter().map(|(_,_,r)| (r.ret > 0.0) as usize * r.trades).sum();
            let win_rate_pct = total_win_trades as f64 / total_trades.max(1) as f64 * 100.0;
            writeln!(uf, "{},{:.2},{:.2},{:.4},{:.2},{},{:.2},{}", uni_label, mult,
                avg_ret, avg_sh, worst_dd, total_trades,
                win_rate_pct, pass_cnt)?;
        }
    }

    // ── Write equity curves CSV ─────────────────────────────────────────────────
    let mut eq_f = File::create(CSV_EQUITY)?;
    // Header
    writeln!(eq_f, "step,{}", ATR_ENTRY_VALS.iter().map(|m| format!("mult_{:.2}", m)).collect::<Vec<_>>().join(","))?;
    // Find the maximum equity curve length
    let max_len = equity_curves.iter().map(|v| v.len()).max().unwrap_or(0);
    for i in 0..max_len {
        let vals: Vec<String> = (0..N_MULT).map(|mi| {
            equity_curves[mi].get(i).map(|&x| format!("{:.6}", x)).unwrap_or_default()
        }).collect();
        writeln!(eq_f, "{},{}", i, vals.join(","))?;
    }

    eprintln!("\n==== OUTPUT FILES ====");
    eprintln!("  Aggregate: {}", CSV_RESULTS);
    eprintln!("  Universe: {}", CSV_UNIVERSE);
    eprintln!("  Equity:   {}", CSV_EQUITY);
    eprintln!("  Runtime: {:?}", t0.elapsed());

    Ok(())
}
